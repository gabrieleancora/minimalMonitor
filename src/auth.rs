//! Account secrets have no Debug/Display implementation. Hash tasks retain their
//! permit through completion, even if the requesting future is canceled.
use crate::{
    bounds::WorkBounds,
    error::{Error, Result},
    storage::{
        StorageClient,
        security::{Operation, Value},
    },
};
use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const IDLE_MS: i64 = 2 * 60 * 60 * 1000;
pub const ABSOLUTE_MS: i64 = 24 * 60 * 60 * 1000;
pub const PREAUTH_MS: i64 = 5 * 60 * 1000;

pub fn now_ms() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_millis(),
    )
    .map_err(|_| Error::Unavailable)
}
pub fn random() -> Result<[u8; 32]> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok(bytes)
}
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex<const N: usize>(text: &str) -> Result<[u8; N]> {
    if text.len() != 2 * N
        || !text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::Forbidden);
    }
    let mut bytes = [0; N];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).map_err(|_| Error::Forbidden)?;
    }
    Ok(bytes)
}
pub fn username(text: &str) -> Result<String> {
    let text = text.trim();
    if text.is_empty()
        || text.chars().count() > 64
        || text.len() > 256
        || text.chars().any(char::is_control)
    {
        return Err(Error::InvalidInput);
    }
    // ASCII case folding is explicit; non-ASCII names retain their spelling.
    Ok(text.to_ascii_lowercase())
}
pub fn password(text: &str) -> Result<()> {
    if !(15..=128).contains(&text.chars().count())
        || text.len() > 1024
        || text.chars().any(char::is_control)
    {
        return Err(Error::InvalidInput);
    }
    Ok(())
}
fn algorithm() -> Result<Argon2<'static>> {
    Ok(Argon2::new(
        Algorithm::Argon2id,
        Version::V0x13,
        Params::new(19 * 1024, 2, 1, Some(32)).map_err(|_| Error::Unavailable)?,
    ))
}
pub fn hash_password(text: &str) -> Result<String> {
    password(text)?;
    let salt = SaltString::encode_b64(&random()?[..16]).map_err(|_| Error::Unavailable)?;
    algorithm()?
        .hash_password(text.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| Error::Unavailable)
}
fn verify(text: &str, hash: &str) -> Result<bool> {
    let parsed = PasswordHash::new(hash).map_err(|_| Error::Storage)?;
    // Corrupt database parameters must not induce arbitrary allocations/work.
    if parsed.algorithm.as_str() != "argon2id"
        || parsed.version != Some(19)
        || parsed.params.get_decimal("m") != Some(19 * 1024)
        || parsed.params.get_decimal("t") != Some(2)
        || parsed.params.get_decimal("p") != Some(1)
        || parsed.hash.is_none_or(|h| h.len() != 32)
    {
        return Err(Error::Storage);
    }
    Ok(algorithm()?
        .verify_password(text.as_bytes(), &parsed)
        .is_ok())
}
async fn hash_work<T: Send + 'static>(
    permit: tokio::sync::OwnedSemaphorePermit,
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|_| Error::WorkerStopped)?
}

struct Bucket {
    tokens: f64,
    last: Instant,
}
impl Bucket {
    fn new(now: Instant, capacity: f64) -> Self {
        Self {
            tokens: capacity,
            last: now,
        }
    }
    fn take(&mut self, now: Instant, capacity: f64, per_minute: f64) -> bool {
        self.tokens = (self.tokens
            + now.saturating_duration_since(self.last).as_secs_f64() * per_minute / 60.)
            .min(capacity);
        self.last = now;
        if self.tokens < 1. {
            false
        } else {
            self.tokens -= 1.;
            true
        }
    }
}
struct Limits {
    global: Bucket,
    clients: HashMap<IpAddr, Bucket>,
    mutations: Bucket,
}
pub struct LoginLimits {
    inner: Mutex<Limits>,
    denied: AtomicU64,
}
impl Default for LoginLimits {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            inner: Mutex::new(Limits {
                global: Bucket::new(now, 2.),
                clients: HashMap::new(),
                mutations: Bucket::new(now, 30.),
            }),
            denied: AtomicU64::new(0),
        }
    }
}
impl LoginLimits {
    pub fn login(&self, ip: IpAddr, now: Instant) -> Result<()> {
        let mut l = self.inner.lock().map_err(|_| Error::Unavailable)?;
        l.clients
            .retain(|_, b| now.saturating_duration_since(b.last).as_secs() < 60);
        let client_ok = if l.clients.contains_key(&ip) || l.clients.len() < 2048 {
            l.clients
                .entry(ip)
                .or_insert_with(|| Bucket::new(now, 5.))
                .take(now, 5., 5.)
        } else {
            true
        };
        if !client_ok || !l.global.take(now, 2., 6.) {
            self.denied.fetch_add(1, Ordering::Relaxed);
            return Err(Error::RateLimited);
        }
        Ok(())
    }
    pub fn mutation(&self) -> Result<()> {
        if self
            .inner
            .lock()
            .map_err(|_| Error::Unavailable)?
            .mutations
            .take(Instant::now(), 30., 30.)
        {
            Ok(())
        } else {
            Err(Error::RateLimited)
        }
    }
    pub fn hash_start(&self) -> Result<()> {
        if self
            .inner
            .lock()
            .map_err(|_| Error::Unavailable)?
            .global
            .take(Instant::now(), 2., 6.)
        {
            Ok(())
        } else {
            self.denied.fetch_add(1, Ordering::Relaxed);
            Err(Error::RateLimited)
        }
    }
    pub fn denied(&self) -> u64 {
        self.denied.load(Ordering::Relaxed)
    }
}

#[derive(Clone)]
pub struct Auth {
    pub storage: StorageClient,
    pub limits: Arc<LoginLimits>,
    bounds: WorkBounds,
    signing_key: Arc<Zeroizing<[u8; 32]>>,
    dummy_hash: Arc<String>,
}
pub struct Session {
    pub token: String,
    pub csrf: String,
}
impl Auth {
    pub async fn new(storage: StorageClient, bounds: WorkBounds) -> Result<Self> {
        let permit = bounds.password()?;
        let dummy_hash = hash_work(permit, move || hash_password(&hex(&random()?))).await?;
        Ok(Self {
            storage,
            limits: Arc::new(LoginLimits::default()),
            bounds,
            signing_key: Arc::new(Zeroizing::new(random()?)),
            dummy_hash: Arc::new(dummy_hash),
        })
    }
    fn mac(&self, bytes: &[u8]) -> Result<[u8; 32]> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.signing_key.as_ref().as_ref())
            .map_err(|_| Error::Unavailable)?;
        mac.update(bytes);
        Ok(mac.finalize().into_bytes().into())
    }
    pub fn preauth(&self, now: i64) -> Result<String> {
        let mut payload = Vec::with_capacity(72);
        payload.extend_from_slice(&now.to_be_bytes());
        payload.extend_from_slice(&random()?);
        payload.extend_from_slice(&self.mac(&payload)?);
        Ok(hex(&payload))
    }
    pub fn check_preauth(&self, cookie: &str, header: &str, now: i64) -> Result<()> {
        let bytes = unhex::<72>(cookie)?;
        if !bool::from(cookie.as_bytes().ct_eq(header.as_bytes())) {
            return Err(Error::Forbidden);
        }
        let issued = i64::from_be_bytes(bytes[..8].try_into().map_err(|_| Error::Forbidden)?);
        if issued < 0
            || now < issued
            || now - issued >= PREAUTH_MS
            || !bool::from(bytes[40..].ct_eq(&self.mac(&bytes[..40])?))
        {
            return Err(Error::Forbidden);
        }
        Ok(())
    }
    pub async fn login(
        &self,
        user: String,
        text: Zeroizing<String>,
        ip: IpAddr,
    ) -> Result<Session> {
        self.limits.login(ip, Instant::now())?;
        let user = username(&user)?;
        password(&text)?;
        let permit = self.bounds.password()?;
        let account = match self.storage.security(Operation::Account).await? {
            Value::Account(a) => a,
            _ => return Err(Error::Storage),
        };
        let matching = account.as_ref().is_some_and(|a| a.username == user);
        let hash = account
            .as_ref()
            .map(|a| a.hash.clone())
            .unwrap_or_else(|| (*self.dummy_hash).clone());
        let verified = hash_work(permit, move || verify(&text, &hash)).await?;
        if !verified || !matching {
            return Err(Error::Unauthorized);
        }
        let account = account.ok_or(Error::Unauthorized)?;
        let token = hex(&random()?);
        let csrf = hex(&random()?);
        self.storage
            .security(Operation::Login {
                generation: account.generation,
                token: digest(token.as_bytes()),
                csrf: unhex(&csrf)?,
                now: now_ms()?,
            })
            .await?;
        Ok(Session { token, csrf })
    }
    pub async fn session(&self, token: &str) -> Result<[u8; 32]> {
        unhex::<32>(token).map_err(|_| Error::Unauthorized)?;
        match self
            .storage
            .security(Operation::Session {
                token: digest(token.as_bytes()),
                now: now_ms()?,
            })
            .await?
        {
            Value::Session(csrf) => Ok(csrf),
            _ => Err(Error::Storage),
        }
    }
    pub async fn logout(&self, token: &str) -> Result<()> {
        self.storage
            .security(Operation::Logout {
                token: digest(token.as_bytes()),
                now: now_ms()?,
            })
            .await?;
        Ok(())
    }
    pub async fn change(
        &self,
        token: &str,
        current: Zeroizing<String>,
        new: Zeroizing<String>,
    ) -> Result<()> {
        password(&current)?;
        password(&new)?;
        self.limits.hash_start()?;
        let permit = self.bounds.password()?;
        let account = match self.storage.security(Operation::Account).await? {
            Value::Account(Some(a)) => a,
            _ => return Err(Error::Unauthorized),
        };
        let generation = account.generation;
        let hash = hash_work(permit, move || {
            if !verify(&current, &account.hash)? {
                return Err(Error::Unauthorized);
            }
            hash_password(&new)
        })
        .await?;
        self.storage
            .security(Operation::Change {
                token: digest(token.as_bytes()),
                generation,
                hash,
                now: now_ms()?,
            })
            .await?;
        Ok(())
    }
    pub async fn local_account(
        &self,
        user: String,
        text: Zeroizing<String>,
        reset: bool,
    ) -> Result<()> {
        let user = username(&user)?;
        password(&text)?;
        let permit = self.bounds.password()?;
        let hash = hash_work(permit, move || hash_password(&text)).await?;
        self.storage
            .security(Operation::Local {
                user,
                hash,
                reset,
                now: now_ms()?,
            })
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn canceled_hash_caller_retains_permit_until_blocking_work_finishes() {
        let bounds = WorkBounds::default();
        let permit = bounds.password().unwrap();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let task = tokio::spawn(hash_work(permit, move || {
            let _ = started_tx.send(());
            release_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            Ok(())
        }));
        started_rx.await.unwrap();
        task.abort();
        let _ = task.await;
        assert!(matches!(bounds.password(), Err(Error::Busy)));
        release_tx.send(()).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if let Ok(permit) = bounds.password() {
                    drop(permit);
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
    #[test]
    fn rate_limits_cap_map_expire_and_refill_without_username_lockout() {
        let limiter = LoginLimits::default();
        let now = Instant::now();
        let ip = "127.0.0.1".parse().unwrap();
        assert!(limiter.login(ip, now).is_ok());
        assert!(limiter.login(ip, now).is_ok());
        assert_eq!(limiter.login(ip, now), Err(Error::RateLimited));
        assert!(
            limiter
                .login(ip, now + std::time::Duration::from_secs(10))
                .is_ok()
        );
        for i in 0..2100 {
            let addr = IpAddr::V4(std::net::Ipv4Addr::from(0x08080000 + i));
            let _ = limiter.login(addr, now);
        }
        assert_eq!(limiter.inner.lock().unwrap().clients.len(), 2048);
        let _ = limiter.login(ip, now + std::time::Duration::from_secs(61));
        assert_eq!(limiter.inner.lock().unwrap().clients.len(), 1);
        assert!(limiter.denied() > 0);
    }
    #[test]
    fn password_boundaries_and_corrupt_hash_work_bound() {
        assert!(password(&"é".repeat(128)).is_ok());
        assert!(password(&"a".repeat(14)).is_err());
        assert!(password(&"a".repeat(129)).is_err());
        assert_eq!(
            verify(
                "unused",
                "$argon2id$v=19$m=999999999,t=2,p=1$c2FsdHNhbHRzYWx0c2FsdA$YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXowMTIzNDU"
            ),
            Err(Error::Storage)
        );
    }
}
