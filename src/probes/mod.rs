//! One resolution, whole-answer policy, numeric transports, one total deadline.
mod http;
mod ping;
use crate::{
    bounds::WorkBounds,
    destination::{Policy, Target},
    monitoring::{Family, Kind, MonitorConfig, Outcome, ProbeError, ProbeResult},
};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::{sync::OwnedSemaphorePermit, time::Instant};
use tokio_rustls::rustls::{ClientConfig, RootCertStore, pki_types::CertificateDer};

pub type ProbePermit = Arc<OwnedSemaphorePermit>;
pub type Resolution =
    Pin<Box<dyn Future<Output = std::result::Result<Vec<IpAddr>, ProbeError>> + Send>>;
/// A resolver must retain the supplied permit for any uncancellable work.
pub trait Resolver: Send + Sync {
    fn resolve(&self, host: String, permit: ProbePermit) -> Resolution;
}
pub struct SystemResolver;
impl Resolver for SystemResolver {
    fn resolve(&self, host: String, permit: ProbePermit) -> Resolution {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                // The OS resolver may outlive a canceled caller. Retaining admission
                // bounds all such jobs to four, including timeouts and shutdown.
                let _permit = permit;
                let answers = (host.as_str(), 0)
                    .to_socket_addrs()
                    .map_err(|_| ProbeError::Dns)?;
                let mut ips = Vec::with_capacity(8);
                for answer in answers {
                    if ips.len() == 8 {
                        return Err(ProbeError::DnsLimit);
                    }
                    ips.push(answer.ip());
                }
                if ips.is_empty() {
                    Err(ProbeError::Dns)
                } else {
                    Ok(ips)
                }
            })
            .await
            .map_err(|_| ProbeError::LocalCapacity)?
        })
    }
}
#[derive(Clone)]
pub struct Probes {
    policy: Arc<Policy>,
    resolver: Arc<dyn Resolver>,
    tls: Arc<ClientConfig>,
    bounds: WorkBounds,
}
impl Probes {
    pub fn new(
        policy: Policy,
        bounds: WorkBounds,
        extra_roots: Vec<CertificateDer<'static>>,
    ) -> crate::error::Result<Self> {
        if extra_roots.len() > 16
            || extra_roots.iter().any(|r| r.len() > 16384)
            || extra_roots.iter().map(|r| r.len()).sum::<usize>() > 128 * 1024
        {
            return Err(crate::error::Error::Configuration);
        }
        let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        for root in extra_roots {
            roots
                .add(root)
                .map_err(|_| crate::error::Error::Configuration)?;
        }
        let tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            policy: Arc::new(policy),
            resolver: Arc::new(SystemResolver),
            tls: Arc::new(tls),
            bounds,
        })
    }
    pub fn with_resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = resolver;
        self
    }
    pub fn admit(&self) -> crate::error::Result<ProbePermit> {
        self.bounds.probe().map(Arc::new)
    }
    pub async fn run(&self, config: &MonitorConfig, permit: ProbePermit) -> ProbeResult {
        let start = Instant::now();
        let end = start + Duration::from_secs(u64::from(config.deadline_seconds));
        let mut result = match tokio::time::timeout_at(end, self.execute(config, end, permit)).await
        {
            Ok(result) => result,
            Err(_) => ProbeResult::fault(ProbeError::Timeout),
        };
        result.elapsed_ms = start.elapsed().as_millis().min(30_000) as u32;
        result
    }
    async fn execute(
        &self,
        config: &MonitorConfig,
        end: Instant,
        permit: ProbePermit,
    ) -> ProbeResult {
        let target = match config.validate() {
            Ok(t) => t,
            Err(_) => return ProbeResult::fault(ProbeError::Policy),
        };
        let answers = if let Ok(literal) = target.host().parse::<IpAddr>() {
            vec![literal]
        } else {
            match self
                .resolver
                .resolve(target.host().to_owned(), permit.clone())
                .await
            {
                Ok(a) => a,
                Err(e) => return ProbeResult::fault(e),
            }
        };
        // Count the entire answer set, including duplicates and families that will
        // not be selected. Never truncate/filter before enforcing this boundary.
        if answers.len() > 8 {
            return ProbeResult::fault(ProbeError::DnsLimit);
        }
        let capability = match self.policy.authorize(&target, &answers) {
            Ok(v) => v,
            Err(_) => return ProbeResult::fault(ProbeError::Policy),
        };
        let addresses: Vec<_> = capability
            .addresses()
            .iter()
            .copied()
            .filter(|ip| match config.address_family {
                Family::Auto => true,
                Family::Ipv4 => ip.is_ipv4(),
                Family::Ipv6 => ip.is_ipv6(),
            })
            .take(if config.address_family == Family::Auto {
                2
            } else {
                1
            })
            .collect();
        if addresses.is_empty() {
            return ProbeResult::fault(ProbeError::Dns);
        }
        let mut result = ProbeResult::fault(ProbeError::Connection);
        for (index, ip) in addresses.iter().enumerate() {
            let remaining = end.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return ProbeResult::fault(ProbeError::Timeout);
            }
            let attempt_end = Instant::now() + remaining / (addresses.len() - index) as u32;
            let attempt = self.attempt(config, &target, *ip, attempt_end, permit.clone());
            result = match tokio::time::timeout_at(attempt_end, attempt).await {
                Ok(v) => v,
                Err(_) => ProbeResult::fault(ProbeError::Timeout),
            };
            result.destination_family = Some(if ip.is_ipv4() {
                Family::Ipv4
            } else {
                Family::Ipv6
            });
            if result.outcome != Outcome::Failure {
                break;
            }
        }
        result
    }
    async fn attempt(
        &self,
        config: &MonitorConfig,
        target: &Target,
        ip: IpAddr,
        end: Instant,
        permit: ProbePermit,
    ) -> ProbeResult {
        match config.kind {
            Kind::Tcp => match tokio::net::TcpStream::connect(SocketAddr::new(
                ip,
                target.port().expect("validated TCP port"),
            ))
            .await
            {
                Ok(stream) => {
                    drop(stream);
                    ProbeResult::success()
                }
                Err(e) => ProbeResult::fault(io_error(&e)),
            },
            Kind::Http => {
                match http::status(
                    SocketAddr::new(ip, target.port().expect("validated HTTP port")),
                    target.url().expect("validated URL"),
                    self.tls.clone(),
                )
                .await
                {
                    Ok(code) => ProbeResult {
                        outcome: if config.accepts(code) {
                            Outcome::Success
                        } else {
                            Outcome::Failure
                        },
                        status_code: Some(code),
                        error: (!config.accepts(code)).then_some(ProbeError::HttpStatus),
                        ..ProbeResult::success()
                    },
                    Err(e) => ProbeResult::fault(e),
                }
            }
            Kind::Ping => match ping::echo(ip, end, permit).await {
                Ok(()) => ProbeResult::success(),
                Err(e) => ProbeResult::fault(e),
            },
        }
    }
}
pub(super) fn io_error(error: &std::io::Error) -> ProbeError {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => ProbeError::Permission,
        std::io::ErrorKind::Unsupported => ProbeError::Unsupported,
        std::io::ErrorKind::OutOfMemory => ProbeError::LocalCapacity,
        std::io::ErrorKind::InvalidData => ProbeError::Echo,
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => ProbeError::Timeout,
        _ => ProbeError::Connection,
    }
}
