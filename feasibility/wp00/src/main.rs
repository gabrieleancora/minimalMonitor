mod hashing;
mod ping;
mod storage;
mod transport;

use std::{error::Error, sync::Arc, time::Duration};
use tokio::sync::Semaphore;

type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

// A finite, local-only experiment, not a deployable monitor service.
#[tokio::main(worker_threads = 2)]
async fn main() -> Result<()> {
    let dir = std::path::PathBuf::from("target/wp00-state");
    std::fs::create_dir_all(&dir)?;
    let db = storage::Storage::start(dir.join("probe.db")).await?;
    println!("sqlite_version={}", db.version().await?);
    db.exercise().await?;
    let fixtures = transport::Fixtures::start().await?;
    let clients = transport::Clients::new(&fixtures)?;
    clients.verify_tls(&fixtures).await?;
    println!("tls=trusted_accept,untrusted_reject,wrong_hostname_reject");
    for address in ["127.0.0.1", "::1"] {
        let result =
            tokio::task::spawn_blocking(move || ping::echo(address.parse().unwrap())).await?;
        println!("ping_{address}={result:?}");
        result?;
    }
    // At most four tasks exist at once. There is no pending scheduler queue.
    let slots = Arc::new(Semaphore::new(4));
    for batch in 0..4 {
        let mut tasks = tokio::task::JoinSet::new();
        for index in (batch * 4)..((batch * 4 + 4).min(15)) {
            let permit = slots.clone().try_acquire_owned()?;
            let client = clients.trusted.clone();
            let fixtures = fixtures.addresses();
            tasks.spawn(async move {
                let _permit = permit;
                match index % 3 {
                    0 => transport::http_probe(&client, fixtures.0).await,
                    1 => transport::tcp_probe(fixtures.1).await,
                    _ => tokio::task::spawn_blocking(|| ping::echo("127.0.0.1".parse().unwrap()))
                        .await?
                        .map_err(Into::into),
                }
            });
        }
        while let Some(result) = tasks.join_next().await {
            result??;
        }
    }
    println!("local_probes=15_passed,slots=4");
    let hasher = hashing::Hasher::new();
    println!("phase=idle");
    tokio::time::sleep(Duration::from_secs(3)).await;
    let start = std::time::Instant::now();
    hasher.exercise().await?;
    println!("argon2_exercise_ms={}", start.elapsed().as_millis());
    println!("phase=after_hash");
    tokio::time::sleep(Duration::from_secs(3)).await;
    db.close().await?;
    let reopened = storage::Storage::start(dir.join("probe.db")).await?;
    assert_eq!(reopened.count().await?, 1);
    reopened.close().await?;
    println!("sqlite=commit,rollback,reopen_passed");
    fixtures.close().await;
    println!("wp00=passed");
    Ok(())
}
