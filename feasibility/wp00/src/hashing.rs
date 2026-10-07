use std::sync::Arc;

use argon2::{Algorithm, Argon2, Params, Version};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::Result;

pub struct Hasher(Arc<Semaphore>);

impl Hasher {
    pub fn new() -> Self {
        Self(Arc::new(Semaphore::new(1)))
    }

    fn admit(&self) -> Result<OwnedSemaphorePermit> {
        Ok(self.0.clone().try_acquire_owned()?)
    }

    async fn run(permit: OwnedSemaphorePermit) -> Result<[u8; 32]> {
        Ok(tokio::task::spawn_blocking(move || {
            // Permit stays in the blocking closure even if its async caller is canceled.
            let _permit = permit;
            let params = Params::new(19 * 1024, 2, 1, Some(32)).expect("valid fixed parameters");
            let algorithm = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
            let mut output = [0; 32];
            // Synthetic test input only. Production must generate a unique random salt
            // and use the PHC storage/verification interface; no account exists here.
            algorithm
                .hash_password_into(
                    b"synthetic feasibility input",
                    b"wp00-test-salt!!",
                    &mut output,
                )
                .expect("valid fixed inputs");
            output
        })
        .await?)
    }

    pub async fn exercise(&self) -> Result<()> {
        let permit = self.admit()?;
        assert!(
            self.admit().is_err(),
            "excess hashing must reject immediately"
        );
        let first = Self::run(permit).await?;
        assert_eq!(first, Self::run(self.admit()?).await?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn only_one_hash_is_admitted_and_permit_is_released() -> Result<()> {
        Hasher::new().exercise().await
    }
}
