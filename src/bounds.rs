use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::error::{Error, Result};

pub const DATABASE_QUEUE: usize = 128;
pub const WEB_CONNECTIONS: usize = 16;
pub const PROBE_SLOTS: usize = 4;
pub const BODY_BYTES: usize = 16 * 1024;
pub const HEADER_BYTES: usize = 16 * 1024;
pub const RESPONSE_BYTES: usize = 256 * 1024;
pub const DATABASE_BYTES: u64 = 512 * 1024 * 1024;
pub const WAL_BYTES: u64 = 32 * 1024 * 1024;
pub const DISK_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const WRITE_FREE_BYTES: u64 = 128 * 1024 * 1024;
pub const WARN_FREE_BYTES: u64 = 256 * 1024 * 1024;

/// Shared admission primitives for later workers. No unbounded permit waiters.
#[derive(Clone)]
pub struct WorkBounds {
    probes: Arc<Semaphore>,
    password: Arc<Semaphore>,
    notification: Arc<Semaphore>,
}

impl Default for WorkBounds {
    fn default() -> Self {
        Self {
            probes: Arc::new(Semaphore::new(PROBE_SLOTS)),
            password: Arc::new(Semaphore::new(1)),
            notification: Arc::new(Semaphore::new(1)),
        }
    }
}

impl WorkBounds {
    pub fn probe(&self) -> Result<OwnedSemaphorePermit> {
        self.probes
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    }

    pub fn password(&self) -> Result<OwnedSemaphorePermit> {
        self.password
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    }

    pub fn notification(&self) -> Result<OwnedSemaphorePermit> {
        self.notification
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_admission_rejects_excess_and_releases_on_drop() {
        let bounds = WorkBounds::default();
        let other = bounds.clone();
        let permits: Vec<_> = (0..PROBE_SLOTS).map(|_| bounds.probe().unwrap()).collect();
        assert_eq!(other.probe().unwrap_err(), Error::Busy);
        let password = bounds.password().unwrap();
        assert_eq!(other.password().unwrap_err(), Error::Busy);
        let alert = bounds.notification().unwrap();
        assert_eq!(other.notification().unwrap_err(), Error::Busy);
        drop((permits, password, alert));
        assert!(other.probe().is_ok());
        assert!(other.password().is_ok());
        assert!(other.notification().is_ok());
    }
}
