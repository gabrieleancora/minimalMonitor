use std::{path::PathBuf, thread::JoinHandle, time::Duration};

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};

use crate::Result;

enum Command {
    Version(oneshot::Sender<String>),
    Exercise(oneshot::Sender<rusqlite::Result<()>>),
    Count(oneshot::Sender<rusqlite::Result<i64>>),
}

pub struct Storage {
    tx: mpsc::Sender<Command>,
    worker: JoinHandle<rusqlite::Result<()>>,
}

impl Storage {
    pub async fn start(path: PathBuf) -> Result<Self> {
        let (tx, mut rx) = mpsc::channel(128);
        let (ready_tx, ready_rx) = oneshot::channel();
        let worker = std::thread::Builder::new().name("wp00-sqlite".into()).spawn(move || {
            let connection = Connection::open(path).and_then(|c| {
                c.busy_timeout(Duration::from_secs(2))?;
                c.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;
                    PRAGMA synchronous=FULL; PRAGMA cache_size=-2048;
                    CREATE TABLE IF NOT EXISTS sample(id INTEGER PRIMARY KEY, value TEXT NOT NULL);")?;
                let page_size: u64 = c.pragma_query_value(None, "page_size", |r| r.get(0))?;
                c.pragma_update(None, "max_page_count", (512 * 1024 * 1024u64) / page_size)?;
                Ok(c)
            });
            let mut connection = match connection {
                Ok(c) => { let _ = ready_tx.send(Ok(())); c }
                Err(e) => { let _ = ready_tx.send(Err(e)); return Ok(()); }
            };
            while let Some(command) = rx.blocking_recv() {
                match command {
                    Command::Version(reply) => { let _ = reply.send(rusqlite::version().to_owned()); }
                    Command::Exercise(reply) => { let _ = reply.send(exercise(&mut connection)); }
                    Command::Count(reply) => {
                        let _ = reply.send(connection.query_row("SELECT count(*) FROM sample", [], |r| r.get(0)));
                    }
                }
            }
            connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        })?;
        ready_rx.await??;
        Ok(Self { tx, worker })
    }

    pub async fn version(&self) -> Result<String> {
        let (tx, rx) = oneshot::channel();
        self.tx.try_send(Command::Version(tx))?;
        Ok(rx.await?)
    }

    pub async fn exercise(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.tx.try_send(Command::Exercise(tx))?;
        rx.await??;
        Ok(())
    }

    pub async fn count(&self) -> Result<i64> {
        let (tx, rx) = oneshot::channel();
        self.tx.try_send(Command::Count(tx))?;
        Ok(rx.await??)
    }

    pub async fn close(self) -> Result<()> {
        drop(self.tx);
        tokio::task::spawn_blocking(move || self.worker.join().expect("sqlite worker panicked"))
            .await??;
        Ok(())
    }
}

fn exercise(c: &mut Connection) -> rusqlite::Result<()> {
    c.execute("DELETE FROM sample", [])?;
    {
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO sample(id,value) VALUES (?1,?2)",
            (1, "committed"),
        )?;
        tx.commit()?;
    }
    {
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO sample(id,value) VALUES (?1,?2)",
            (2, "rolled back"),
        )?;
        // Drop rolls back; no acknowledgement before commit.
    }
    let count: i64 = c.query_row("SELECT count(*) FROM sample", [], |r| r.get(0))?;
    assert_eq!(count, 1);
    let value: String = c.query_row("SELECT value FROM sample WHERE id=?1", [1], |r| r.get(0))?;
    assert_eq!(value, "committed");
    let foreign_keys: i64 = c.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
    assert_eq!(foreign_keys, 1);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn worker_commits_and_rolls_back() -> Result<()> {
        let db = Storage::start(PathBuf::from(":memory:")).await?;
        db.exercise().await?;
        db.close().await
    }

    #[test]
    fn saturated_channel_rejects_instead_of_queueing() {
        let (tx, _rx) = mpsc::channel(128);
        for _ in 0..128 {
            let (reply, _) = oneshot::channel();
            assert!(tx.try_send(Command::Version(reply)).is_ok());
        }
        let (reply, _) = oneshot::channel();
        assert!(matches!(
            tx.try_send(Command::Version(reply)),
            Err(mpsc::error::TrySendError::Full(_))
        ));
    }
}
