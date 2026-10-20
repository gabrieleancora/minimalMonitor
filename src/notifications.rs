//! Bounded Telegram sender, separate from probe and storage health.
mod transport;
use crate::{
    bounds::WorkBounds,
    error::{Error, Result},
    storage::{
        StorageClient,
        notifications::{Operation, Value},
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    io::Read,
    path::Path,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
pub use transport::Telegram;
use zeroize::Zeroizing;

/// Deliberately no Debug/Serialize; credentials never cross reporting boundaries.
pub struct Token(Zeroizing<String>);
impl Token {
    pub fn load(path: &Path) -> Result<Self> {
        crate::ownership::verify_private(path, false).map_err(|_| Error::Configuration)?;
        let mut value = Zeroizing::new(String::new());
        std::fs::File::open(path)
            .map_err(|_| Error::Configuration)?
            .take(257)
            .read_to_string(&mut value)
            .map_err(|_| Error::Configuration)?;
        if value.len() > 256 {
            return Err(Error::Configuration);
        }
        let trimmed = value
            .strip_suffix("\r\n")
            .or_else(|| value.strip_suffix('\n'))
            .unwrap_or(&value);
        let value = Zeroizing::new(trimmed.to_owned());
        let (id, secret) = value.split_once(':').ok_or(Error::Configuration)?;
        if id.is_empty()
            || id.len() > 20
            || !id.bytes().all(|b| b.is_ascii_digit())
            || !(20..=128).contains(&secret.len())
            || !secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::Configuration);
        }
        Ok(Self(value))
    }
    pub fn fingerprint(&self) -> [u8; 32] {
        Sha256::digest(self.0.as_bytes()).into()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Delivered(i64),
    Transient,
    RateLimited { retry_after_seconds: u64 },
    Configuration,
}
pub type SendFuture<'a> = Pin<Box<dyn Future<Output = Delivery> + Send + 'a>>;
pub trait Transport: Send + Sync {
    fn timeout_outcome(&self) -> Delivery {
        Delivery::Transient
    }
    fn send<'a>(
        &'a self,
        chat: i64,
        text: &'a str,
        permit: crate::probes::ProbePermit,
    ) -> SendFuture<'a>;
}

#[derive(Clone, Default)]
pub struct SenderHealth {
    pub token_present: bool,
    pub persistence_degraded: Arc<AtomicBool>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventContent {
    pub name: String,
    pub observed_at_ms: i64,
    pub reason: Option<crate::monitoring::ProbeError>,
}
#[derive(Clone, Debug, Serialize)]
pub struct JobRef {
    pub id: i64,
    pub attempt: u8,
}
pub struct Batch {
    pub request_id: i64,
    pub jobs: Vec<JobRef>,
    pub chat: i64,
    pub text: String,
}

/// One in-memory acknowledgement only. Failed persistence never resends that batch.
pub struct Sender<T> {
    storage: StorageClient,
    transport: T,
    bounds: WorkBounds,
    pub health: SenderHealth,
    origin: String,
    clock: Arc<dyn Fn() -> Result<i64> + Send + Sync>,
    ack: Option<(i64, Vec<JobRef>, Delivery, i64)>,
    // The claim may commit without its reply reaching us. Retain admission
    // until a worker-ordered reconciliation confirms that no send is reserved.
    unsubmitted_claim: Option<crate::probes::ProbePermit>,
}
impl<T: Transport> Sender<T> {
    pub fn new(
        storage: StorageClient,
        transport: T,
        bounds: WorkBounds,
        health: SenderHealth,
        origin: &url::Url,
    ) -> Self {
        Self {
            storage,
            transport,
            bounds,
            health,
            origin: origin.as_str().to_owned(),
            clock: Arc::new(crate::auth::now_ms),
            ack: None,
            unsubmitted_claim: None,
        }
    }
    pub fn with_clock(mut self, clock: Arc<dyn Fn() -> Result<i64> + Send + Sync>) -> Self {
        self.clock = clock;
        self
    }
    pub async fn initialize(&self, fingerprint: Option<[u8; 32]>, now: i64) -> Result<()> {
        self.storage
            .notifications(Operation::Startup { fingerprint, now })
            .await
            .map(|_| ())
    }
    pub async fn tick(&mut self, now: i64) -> Result<()> {
        let result = self.tick_inner(now).await;
        self.health
            .persistence_degraded
            .store(result.is_err(), Ordering::Relaxed);
        result
    }
    async fn tick_inner(&mut self, now: i64) -> Result<()> {
        if self.unsubmitted_claim.is_some() {
            // This command is serialized after the uncertain claim on the same
            // worker. Only this sender, before entering transport, can use it.
            match self
                .storage
                .notifications(Operation::ReconcileUnsubmittedClaim { now })
                .await?
            {
                Value::Done => self.unsubmitted_claim = None,
                _ => return Err(Error::Storage),
            }
        }
        if let Some((request_id, jobs, outcome, completed)) = &self.ack {
            self.storage
                .notifications(Operation::Finish {
                    request_id: *request_id,
                    jobs: jobs.clone(),
                    outcome: *outcome,
                    now: *completed,
                })
                .await?;
            self.ack = None;
        }
        // Missing credentials must not reserve jobs or consume attempts.
        if !self.health.token_present {
            self.storage
                .notifications(Operation::Maintain { now })
                .await?;
            return Ok(());
        }
        let permit = Arc::new(self.bounds.notification()?);
        // Set before awaiting: a lost reply (or canceled claim wait) retains the
        // evidence that transport was never entered and prevents another sender.
        self.unsubmitted_claim = Some(permit.clone());
        let batch = match self
            .storage
            .notifications(Operation::Claim {
                now,
                origin: self.origin.clone(),
            })
            .await?
        {
            Value::Batch(batch) => batch,
            _ => return Err(Error::Storage),
        };
        self.unsubmitted_claim = None;
        if let Some(batch) = batch {
            let outcome = tokio::time::timeout(
                Duration::from_secs(10),
                self.transport.send(batch.chat, &batch.text, permit.clone()),
            )
            .await
            .unwrap_or_else(|_| self.transport.timeout_outcome());
            // Use wall completion, not start time, for D-22 retries. Controlled
            // storage tests supply explicit completion timestamps to Finish.
            let completed = (self.clock)()?.max(now);
            self.ack = Some((batch.request_id, batch.jobs, outcome, completed));
            let (request_id, jobs, outcome, completed) =
                self.ack.as_ref().expect("assigned acknowledgement");
            self.storage
                .notifications(Operation::Finish {
                    request_id: *request_id,
                    jobs: jobs.clone(),
                    outcome: *outcome,
                    now: *completed,
                })
                .await?;
            self.ack = None;
        }
        drop(permit);
        Ok(())
    }
    pub async fn run(mut self, shutdown: impl Future<Output = ()>) -> Result<()> {
        tokio::pin!(shutdown);
        let mut ticks = tokio::time::interval(Duration::from_millis(200));
        ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                biased;
                _=&mut shutdown=>return Ok(()),
                _=ticks.tick()=>{
                    let attempt=async { self.tick(crate::auth::now_ms()?).await };
                    tokio::select! {
                        biased;
                        _=&mut shutdown=>return Ok(()),
                        result=attempt=>self.health.persistence_degraded.store(result.is_err(),Ordering::Relaxed),
                    }
                }
            }
        }
    }
}

pub(crate) fn clean_name(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(100).collect()
}
/// Gregorian civil date from Unix days; UTC is explicit in every observation.
pub(crate) fn stamp(ms: i64) -> String {
    let seconds = ms.max(0) / 1000;
    let days = seconds / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secret_file_is_bounded_validated_and_never_serializable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        let valid = format!("12345:{}\n", "X".repeat(32));
        std::fs::write(&path, &valid).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
        assert!(Token::load(&path).is_ok());
        for invalid in [
            "x".repeat(257),
            "no colon".into(),
            "1:short".into(),
            format!("123:{} /sendMessage", "X".repeat(32)),
            format!(" 123:{}", "X".repeat(32)),
        ] {
            std::fs::write(&path, invalid).unwrap();
            assert!(Token::load(&path).is_err());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            std::fs::write(&path, valid).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(Token::load(&path).is_err());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let linked = dir.path().join("linked");
            symlink(&path, &linked).unwrap();
            assert!(Token::load(&linked).is_err());
        }
    }
    #[test]
    fn dates_and_controls_are_sanitized() {
        assert_eq!(stamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(stamp(951827696000), "2000-02-29 12:34:56 UTC");
        assert_eq!(clean_name("a\r\nb\0"), "ab");
    }
}
