use minimalmonitor::{
    bounds::WorkBounds,
    destination::{Policy, Rule},
    monitoring::*,
    probes::{ProbePermit, Probes, Resolution, Resolver},
};
use std::{
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer},
    },
};

struct Answers {
    ips: Vec<IpAddr>,
    calls: AtomicUsize,
}
impl Resolver for Answers {
    fn resolve(&self, _host: String, _permit: ProbePermit) -> Resolution {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let ips = self.ips.clone();
        Box::pin(async move { Ok(ips) })
    }
}
fn answers(ips: &[&str]) -> Arc<Answers> {
    Arc::new(Answers {
        ips: ips.iter().map(|s| s.parse().unwrap()).collect(),
        calls: AtomicUsize::new(0),
    })
}
fn config(kind: Kind, port: u16, tls: bool) -> MonitorConfig {
    MonitorConfig {
        name: "fixture".into(),
        kind,
        target: if kind == Kind::Http {
            format!(
                "{}://fixture.invalid:{port}/check?fixture=value",
                if tls { "https" } else { "http" }
            )
        } else {
            "fixture.invalid".into()
        },
        port: (kind == Kind::Tcp).then_some(port),
        interval_seconds: 60,
        deadline_seconds: 1,
        failure_threshold: 3,
        enabled: true,
        alerts: true,
        address_family: Family::Auto,
        accepted_statuses: vec![[200, 299]],
    }
}
fn policy() -> Policy {
    Policy::new(vec![
        Rule {
            cidr: "127.0.0.1/32".into(),
            host: None,
            ports: vec![],
            all_tcp_ports: true,
            ping: true,
        },
        Rule {
            cidr: "::1/128".into(),
            host: None,
            ports: vec![],
            all_tcp_ports: true,
            ping: true,
        },
    ])
    .unwrap()
}
fn runner(ips: Arc<Answers>, root: Option<CertificateDer<'static>>) -> Probes {
    Probes::new(policy(), WorkBounds::default(), root.into_iter().collect())
        .unwrap()
        .with_resolver(ips)
}
async fn write<T: AsyncRead + AsyncWrite + Unpin>(
    mut stream: T,
    bytes: &[u8],
    chunk: usize,
) -> Vec<u8> {
    let mut request = vec![];
    let mut block = [0u8; 512];
    while request.len() < 4096 && !request.ends_with(b"\r\n\r\n") {
        let n = stream.read(&mut block).await.unwrap_or(0);
        if n == 0 {
            break;
        }
        request.extend_from_slice(&block[..n]);
    }
    for part in bytes.chunks(chunk) {
        if stream.write_all(part).await.is_err() {
            break;
        }
        tokio::task::yield_now().await;
    }
    request
}
fn header(size: usize) -> Vec<u8> {
    let mut response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX-Fixture: ".to_vec();
    response.resize(size - 4, b'x');
    response.extend_from_slice(b"\r\n\r\n");
    response
}
async fn reply(bytes: Vec<u8>, tls: bool, chunk: usize) -> ProbeResult {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let key = rcgen::generate_simple_self_signed(vec!["fixture.invalid".into()]).unwrap();
    let runner = runner(answers(&["127.0.0.1"]), Some(key.cert.der().clone()));
    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![key.cert.der().clone()],
            PrivatePkcs8KeyDer::from(key.signing_key.serialize_der()).into(),
        )
        .unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        if tls {
            let stream = TlsAcceptor::from(Arc::new(cfg))
                .accept(stream)
                .await
                .unwrap();
            write(stream, &bytes, chunk).await
        } else {
            write(stream, &bytes, chunk).await
        }
    });
    let result = runner
        .run(&config(Kind::Http, port, tls), runner.admit().unwrap())
        .await;
    server.abort();
    let _ = server.await;
    result
}
#[tokio::test]
async fn exact_header_boundary_and_all_oversize_shapes_over_http_and_https() {
    let mut cumulative = b"HTTP/1.1 200 OK\r\n".to_vec();
    for n in 0..64 {
        cumulative.extend_from_slice(format!("X-{n}: {}\r\n", "x".repeat(600)).as_bytes());
    }
    cumulative.extend_from_slice(b"\r\n");
    let mut many = b"HTTP/1.1 200 OK\r\n".to_vec();
    for n in 0..101 {
        many.extend_from_slice(format!("X-{n}: x\r\n").as_bytes());
    }
    many.extend_from_slice(b"\r\n");
    for tls in [false, true] {
        assert_eq!(
            reply(header(32768), tls, usize::MAX).await.outcome,
            Outcome::Success
        );
        for bytes in [
            header(32769),
            header(256 * 1024),
            cumulative.clone(),
            many.clone(),
        ] {
            assert_eq!(
                reply(bytes, tls, usize::MAX).await.error,
                Some(ProbeError::Headers)
            );
        }
        assert_eq!(
            reply(header(32769), tls, 113).await.error,
            Some(ProbeError::Headers)
        );
        let mut partial = header(32776);
        partial.truncate(32768);
        assert_eq!(
            reply(partial, tls, 113).await.error,
            Some(ProbeError::Headers)
        );
    }
}
#[tokio::test]
async fn original_host_path_tls_name_redirect_status_and_no_body_consumption() {
    for tls in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let ips = answers(&["127.0.0.1"]);
        let key = rcgen::generate_simple_self_signed(vec!["fixture.invalid".into()]).unwrap();
        let probe = runner(ips.clone(), Some(key.cert.der().clone()));
        let cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![key.cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.signing_key.serialize_der()).into(),
            )
            .unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let response=b"HTTP/1.1 302 Found\r\nLocation: http://169.254.169.254/\r\nContent-Length: 999999999\r\n\r\n";
            if tls {
                write(
                    TlsAcceptor::from(Arc::new(cfg))
                        .accept(stream)
                        .await
                        .unwrap(),
                    response,
                    usize::MAX,
                )
                .await
            } else {
                write(stream, response, usize::MAX).await
            }
        });
        let mut cfg = config(Kind::Http, port, tls);
        cfg.accepted_statuses = vec![[302, 302]];
        let result = probe.run(&cfg, probe.admit().unwrap()).await;
        assert_eq!(
            (result.outcome, result.status_code),
            (Outcome::Success, Some(302))
        );
        assert_eq!(ips.calls.load(Ordering::Relaxed), 1);
        let request = String::from_utf8(server.await.unwrap()).unwrap();
        assert!(request.starts_with("GET /check?fixture=value HTTP/1.1\r\n"));
        assert!(request.contains(&format!("host: fixture.invalid:{port}\r\n")));
    }
    assert_eq!(
        reply(
            b"HTTP/1.1 503 Failed\r\nContent-Length: 0\r\n\r\n".to_vec(),
            false,
            usize::MAX
        )
        .await
        .error,
        Some(ProbeError::HttpStatus)
    );
    assert_eq!(
        reply(
            b"HTTP/1.1 200 OK\r\nContent-Length: 999999999\r\n\r\n".to_vec(),
            false,
            usize::MAX
        )
        .await
        .outcome,
        Outcome::Success
    );
}
#[tokio::test]
async fn every_protocol_rejects_whole_overlimit_mixed_mapped_and_metadata_before_connect() {
    for kind in [Kind::Http, Kind::Tcp, Kind::Ping] {
        for tls in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            for ips in [
                vec!["127.0.0.1"; 9],
                vec!["127.0.0.1", "169.254.169.254"],
                vec!["::ffff:127.0.0.1", "fd00:ec2::254"],
                vec!["127.0.0.1", "10.0.0.1"],
            ] {
                let probe = runner(answers(&ips), None);
                let mut cfg = config(kind, port, tls);
                cfg.address_family = Family::Ipv4;
                let result = probe.run(&cfg, probe.admit().unwrap()).await;
                assert_eq!(result.outcome, Outcome::Unknown);
                assert_eq!(
                    result.error,
                    Some(if ips.len() > 8 {
                        ProbeError::DnsLimit
                    } else {
                        ProbeError::Policy
                    })
                );
                assert!(
                    tokio::time::timeout(Duration::from_millis(10), listener.accept())
                        .await
                        .is_err()
                );
            }
        }
    }
}
#[tokio::test]
async fn tcp_numeric_fallback_resolves_once_sends_nothing_and_no_overlap_admission() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ips = answers(&["::1", "127.0.0.1"]);
    let probe = runner(ips.clone(), None);
    let permits: Vec<_> = (0..4).map(|_| probe.admit().unwrap()).collect();
    assert!(probe.admit().is_err());
    drop(permits);
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 1];
        assert_eq!(stream.read(&mut bytes).await.unwrap(), 0);
    });
    assert_eq!(
        probe
            .run(&config(Kind::Tcp, port, false), probe.admit().unwrap())
            .await
            .outcome,
        Outcome::Success
    );
    server.await.unwrap();
    assert_eq!(ips.calls.load(Ordering::Relaxed), 1);
    // Same port now refuses; no automatic transport retry.
    let probe = runner(answers(&["127.0.0.1"]), None);
    assert_eq!(
        probe
            .run(&config(Kind::Tcp, port, false), probe.admit().unwrap())
            .await
            .outcome,
        Outcome::Failure
    );
}
#[tokio::test]
async fn untrusted_wrong_hostname_and_expired_tls_are_rejected() {
    for mode in [0, 1, 2] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let mut params = rcgen::CertificateParams::new(vec![
            if mode == 1 {
                "other.invalid"
            } else {
                "fixture.invalid"
            }
            .into(),
        ])
        .unwrap();
        if mode == 2 {
            params.not_before = rcgen::date_time_ymd(2000, 1, 1);
            params.not_after = rcgen::date_time_ymd(2001, 1, 1);
        }
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key).unwrap();
        let probe = runner(
            answers(&["127.0.0.1"]),
            (mode != 0).then(|| cert.der().clone()),
        );
        let cfg = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _ = TlsAcceptor::from(Arc::new(cfg)).accept(stream).await;
        });
        let result = probe
            .run(&config(Kind::Http, port, true), probe.admit().unwrap())
            .await;
        assert_eq!(result.error, Some(ProbeError::Tls));
        server.await.unwrap();
    }
}
#[tokio::test]
async fn stalled_tls_deadline_closes_socket_and_resolver_cancellation_retains_capacity() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = [0; 4096];
        while stream.read(&mut bytes).await.unwrap() != 0 {}
    });
    let probe = runner(answers(&["127.0.0.1"]), None);
    let start = tokio::time::Instant::now();
    assert_eq!(
        probe
            .run(&config(Kind::Http, port, true), probe.admit().unwrap())
            .await
            .error,
        Some(ProbeError::Timeout)
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    server.await.unwrap();
    struct Blocked(Arc<std::sync::Barrier>);
    impl Resolver for Blocked {
        fn resolve(&self, _: String, permit: ProbePermit) -> Resolution {
            let barrier = self.0.clone();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    barrier.wait();
                    vec!["127.0.0.1".parse().unwrap()]
                })
                .await
                .map_err(|_| ProbeError::LocalCapacity)
            })
        }
    }
    let barrier = Arc::new(std::sync::Barrier::new(5));
    let probe = Probes::new(policy(), WorkBounds::default(), vec![])
        .unwrap()
        .with_resolver(Arc::new(Blocked(barrier.clone())));
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let runner = probe.clone();
        let permit = runner.admit().unwrap();
        jobs.spawn(async move { runner.run(&config(Kind::Tcp, port, false), permit).await });
    }
    while let Some(result) = jobs.join_next().await {
        assert_eq!(result.unwrap().error, Some(ProbeError::Timeout));
    }
    assert!(probe.admit().is_err());
    tokio::task::spawn_blocking(move || barrier.wait())
        .await
        .unwrap();
    // Native jobs release their permits before their joins are collected.
    for _ in 0..100 {
        if probe.admit().is_ok() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("native permit release");
}
#[tokio::test]
async fn native_ping_v4_v6_and_system_dns_are_real_platform_checks() {
    let probe = Probes::new(policy(), WorkBounds::default(), vec![]).unwrap();
    for ip in ["127.0.0.1", "::1"] {
        let mut cfg = config(Kind::Ping, 1, false);
        cfg.target = ip.into();
        let result = probe.run(&cfg, probe.admit().unwrap()).await;
        assert_eq!(result.outcome, Outcome::Success, "{ip}: {:?}", result.error);
    }
    let permit = probe.admit().unwrap();
    let resolver = minimalmonitor::probes::SystemResolver;
    let resolved = resolver.resolve("localhost".into(), permit).await.unwrap();
    assert!(!resolved.is_empty());
    assert!(resolved.iter().all(|ip| ip.is_loopback()));
}

#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "requires an isolated network namespace with ping permission denied"]
async fn native_ping_permission_is_a_local_fault() {
    let probe = Probes::new(policy(), WorkBounds::default(), vec![]).unwrap();
    for ip in ["127.0.0.1", "::1"] {
        let mut cfg = config(Kind::Ping, 1, false);
        cfg.target = ip.into();
        let result = probe.run(&cfg, probe.admit().unwrap()).await;
        assert_eq!(
            (result.outcome, result.error),
            (Outcome::Unknown, Some(ProbeError::Permission))
        );
    }
}
