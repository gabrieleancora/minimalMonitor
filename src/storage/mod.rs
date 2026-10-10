mod migrations;
pub mod security;

use rusqlite::Connection;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, SyncSender, TrySendError},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

use crate::{
    bounds::{
        DATABASE_BYTES, DATABASE_QUEUE, DISK_BYTES, WAL_BYTES, WARN_FREE_BYTES, WRITE_FREE_BYTES,
    },
    error::{Error, Result},
    ownership::{StateOwner, private_file, verify_private},
};

const QUERY_TIME: Duration = Duration::from_secs(2);
const REPLY_TIME: Duration = Duration::from_secs(3);

struct Command {
    deadline: Instant,
    reply: oneshot::Sender<Result<StorageHealth>>,
}
enum Work {
    Health(Command),
    Security(security::Command),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StorageHealth {
    pub writable: bool,
    pub disk_warning: bool,
}

#[derive(Clone)]
pub struct StorageClient {
    tx: SyncSender<Work>,
    stopping: Arc<AtomicBool>,
}

pub struct Storage {
    client: StorageClient,
    worker: Option<JoinHandle<Result<()>>>,
}

impl Storage {
    pub async fn start(owner: StateOwner) -> Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Work>(DATABASE_QUEUE);
        let (ready_tx, ready_rx) = oneshot::channel();
        let stopping = Arc::new(AtomicBool::new(false));
        let worker_stopping = stopping.clone();
        let worker = std::thread::Builder::new()
            .name("minimalmonitor-sqlite".into())
            .spawn(move || {
                // Ownership remains here until all SQLite work and close complete.
                let root = owner.root();
                let mut connection = match open(root) {
                    Ok(connection) => connection,
                    Err(err) => {
                        let _ = ready_tx.send(Err(err));
                        return Err(err);
                    }
                };
                let _ = ready_tx.send(Ok(()));
                loop {
                    let command = if worker_stopping.load(Ordering::Acquire) {
                        match rx.try_recv() {
                            Ok(command) => Some(command),
                            Err(_) => break,
                        }
                    } else {
                        match rx.recv_timeout(Duration::from_secs(1)) {
                            Ok(command) => Some(command),
                            Err(mpsc::RecvTimeoutError::Timeout) => None,
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    };
                    if let Some(command) = command {
                        let command = match command {
                            Work::Health(command) => command,
                            Work::Security(command) => {
                                security::dispatch(&mut connection, root, command);
                                continue;
                            }
                        };
                        if command.reply.is_closed() {
                            continue;
                        }
                        let result = if Instant::now() >= command.deadline {
                            Err(Error::Deadline)
                        } else {
                            health(&mut connection, root, command.deadline)
                        };
                        let _ = command.reply.send(result);
                    } else {
                        // Short periodic maintenance; readiness always performs a
                        // fresh round-trip, so a dead/stalled worker cannot look ready.
                        if health(&mut connection, root, Instant::now() + QUERY_TIME).is_ok_and(|h| h.writable)
                            && let Ok(now)=crate::auth::now_ms() {
                            let _=connection.execute("DELETE FROM sessions WHERE expires_at_ms<=?1 OR last_used_at_ms<=?2 OR created_at_ms>?1 OR last_used_at_ms>?1",rusqlite::params![now,now-crate::auth::IDLE_MS]);
                        }
                    }
                }
                connection
                    .progress_handler(0, None::<fn() -> bool>)
                    .map_err(|_| Error::Storage)?;
                let busy: i64 = connection
                    .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
                    .map_err(|_| Error::Storage)?;
                if busy != 0 {
                    return Err(Error::Busy);
                }
                connection.close().map_err(|_| Error::Storage)?;
                drop(owner);
                Ok(())
            })
            .map_err(|_| Error::WorkerStopped)?;
        match ready_rx.await {
            Ok(Ok(())) => Ok(Self {
                client: StorageClient { tx, stopping },
                worker: Some(worker),
            }),
            result => {
                stopping.store(true, Ordering::Release);
                drop(tx);
                let _ = tokio::task::spawn_blocking(move || worker.join()).await;
                Err(result
                    .ok()
                    .and_then(std::result::Result::err)
                    .unwrap_or(Error::WorkerStopped))
            }
        }
    }

    pub fn client(&self) -> StorageClient {
        self.client.clone()
    }

    pub async fn shutdown(mut self) -> Result<()> {
        self.client.stopping.store(true, Ordering::Release);
        let worker = self.worker.take().ok_or(Error::WorkerStopped)?;
        // If timed out, the worker still holds ownership; never unlock a live DB.
        tokio::time::timeout(
            Duration::from_secs(10),
            tokio::task::spawn_blocking(move || worker.join()),
        )
        .await
        .map_err(|_| Error::Deadline)?
        .map_err(|_| Error::WorkerStopped)?
        .map_err(|_| Error::WorkerStopped)?
    }
}

impl Drop for Storage {
    fn drop(&mut self) {
        // Cancellation/drop must not leave a worker running behind surviving
        // client clones. The thread closes on its next bounded receive timeout.
        self.client.stopping.store(true, Ordering::Release);
    }
}

impl StorageClient {
    pub async fn health(&self) -> Result<StorageHealth> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Unavailable);
        }
        let (reply, receive) = oneshot::channel();
        enqueue(
            &self.tx,
            Command {
                deadline: Instant::now() + QUERY_TIME,
                reply,
            },
        )?;
        tokio::time::timeout(REPLY_TIME, receive)
            .await
            .map_err(|_| Error::Deadline)?
            .map_err(|_| Error::WorkerStopped)?
    }
}

fn enqueue(tx: &SyncSender<Work>, command: Command) -> Result<()> {
    tx.try_send(Work::Health(command)).map_err(|err| match err {
        TrySendError::Full(_) => Error::Busy,
        TrySendError::Disconnected(_) => Error::WorkerStopped,
    })
}

fn open(root: &Path) -> Result<Connection> {
    if fs2::available_space(root).map_err(|_| Error::Io)? < WRITE_FREE_BYTES
        || migrations::file_bytes(&root.join("monitor.db"))? > DATABASE_BYTES
    {
        return Err(Error::DiskPressure);
    }
    for name in ["monitor.db-wal", "monitor.db-shm", "monitor.db-journal"] {
        let path = root.join(name);
        if std::fs::symlink_metadata(&path).is_ok() {
            verify_private(&path, false)?;
        }
    }
    let path = root.join("monitor.db");
    drop(private_file(&path)?);
    let mut connection = Connection::open(&path).map_err(|_| Error::Storage)?;
    connection
        .set_db_config(rusqlite::config::DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)
        .map_err(|_| Error::Storage)?;
    use rusqlite::limits::Limit;
    for (limit, value) in [
        (Limit::SQLITE_LIMIT_LENGTH, 1024 * 1024),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, 64 * 1024),
        (Limit::SQLITE_LIMIT_COLUMN, 128),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 128),
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
    ] {
        connection
            .set_limit(limit, value)
            .map_err(|_| Error::Storage)?;
    }
    connection
        .busy_timeout(QUERY_TIME)
        .map_err(|_| Error::Storage)?;
    let deadline = Instant::now() + Duration::from_secs(30);
    connection
        .progress_handler(1000, Some(move || Instant::now() >= deadline))
        .map_err(|_| Error::Storage)?;
    let version = migrations::inspect(&connection)?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA cache_size=-2048; PRAGMA wal_autocheckpoint=1000; PRAGMA journal_size_limit=33554432;").map_err(|_| Error::Storage)?;
    let page_size: i64 = connection
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(|_| Error::Storage)?;
    if !(512..=65536).contains(&page_size) {
        return Err(Error::Schema);
    }
    let pages = DATABASE_BYTES as i64 / page_size;
    let actual: i64 = connection
        .query_row(&format!("PRAGMA max_page_count={pages}"), [], |r| r.get(0))
        .map_err(|_| Error::Storage)?;
    if actual < 1 || actual > pages {
        return Err(Error::DiskPressure);
    }
    migrations::migrate(&mut connection, root, version)?;
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| Error::Storage)?;
    Ok(connection)
}

fn health(connection: &mut Connection, root: &Path, deadline: Instant) -> Result<StorageHealth> {
    connection
        .progress_handler(1000, Some(move || Instant::now() >= deadline))
        .map_err(|_| Error::Storage)?;
    let result = (|| {
        verify_private(root, true)?;
        verify_private(&root.join("monitor.db"), false)?;
        connection
            .query_row("SELECT revision FROM settings WHERE id=1", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(|_| Error::Storage)?;
        let (mut busy, _, _): (i64, i64, i64) = connection
            .query_row("PRAGMA wal_checkpoint(PASSIVE)", [], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .map_err(|_| Error::Storage)?;
        if migrations::file_bytes(&root.join("monitor.db-wal"))? >= WAL_BYTES {
            if Instant::now() >= deadline {
                return Err(Error::Deadline);
            }
            // PASSIVE may finish without shrinking the allocated WAL. One
            // SQLite-managed reclamation attempt avoids permanently blocking
            // admission, but never waits for a pinned reader/writer or retries
            // here. Later maintenance can retry after that transaction ends.
            connection
                .busy_timeout(Duration::ZERO)
                .map_err(|_| Error::Storage)?;
            let recovery = connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                row.get::<_, i64>(0)
            });
            // Restore the normal timeout even when reclamation fails.
            connection
                .busy_timeout(QUERY_TIME)
                .map_err(|_| Error::Storage)?;
            busy = recovery.map_err(|_| Error::Storage)?;
        }
        let free = fs2::available_space(root).map_err(|_| Error::Io)?;
        let db = migrations::file_bytes(&root.join("monitor.db"))?;
        let wal = migrations::file_bytes(&root.join("monitor.db-wal"))?;
        let used = migrations::directory_bytes(root)?;
        let writable = write_admitted(free, db, wal, used, busy == 0);
        if Instant::now() >= deadline {
            return Err(Error::Deadline);
        }
        Ok(StorageHealth {
            writable,
            disk_warning: free < WARN_FREE_BYTES || db >= 400 * 1024 * 1024 || !writable,
        })
    })();
    connection
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| Error::Storage)?;
    result
}

fn write_admitted(free: u64, db: u64, wal: u64, used: u64, checkpoint_ok: bool) -> bool {
    checkpoint_ok
        && free >= WRITE_FREE_BYTES
        && db < DATABASE_BYTES
        && wal < WAL_BYTES
        && used <= DISK_BYTES.saturating_sub((128 + 64 + 800) * 1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn worker_ready_shutdown_reopen_and_old_clients_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let owner = StateOwner::acquire(&root).unwrap();
        let storage = Storage::start(owner).await.unwrap();
        let client = storage.client();
        assert!(client.health().await.unwrap().writable);
        let path = root.join("monitor.db");
        tokio::task::spawn_blocking(move || {
            let db = Connection::open(path).unwrap();
            db.execute("DELETE FROM settings", []).unwrap();
        })
        .await
        .unwrap();
        assert_eq!(client.health().await, Err(Error::Storage));
        let path = root.join("monitor.db");
        tokio::task::spawn_blocking(move || {
            let db = Connection::open(path).unwrap();
            db.execute("INSERT INTO settings(id,revision) VALUES(1,1)", [])
                .unwrap();
        })
        .await
        .unwrap();
        assert!(client.health().await.unwrap().writable);
        assert!(matches!(
            StateOwner::acquire(&root),
            Err(Error::AlreadyOwned)
        ));
        storage.shutdown().await.unwrap();
        assert_eq!(client.health().await, Err(Error::Unavailable));
        let storage = Storage::start(StateOwner::acquire(&root).unwrap())
            .await
            .unwrap();
        assert!(storage.client().health().await.unwrap().writable);
        storage.shutdown().await.unwrap();
        let db = Connection::open(root.join("monitor.db")).unwrap();
        assert_eq!(
            db.query_row("SELECT count(*) FROM schema_versions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM admin", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn queue_full_and_disconnected_reject_without_waiting() {
        let (tx, rx) = mpsc::sync_channel(DATABASE_QUEUE);
        for _ in 0..DATABASE_QUEUE {
            let (reply, _) = oneshot::channel();
            enqueue(
                &tx,
                Command {
                    deadline: Instant::now() + QUERY_TIME,
                    reply,
                },
            )
            .unwrap();
        }
        let (reply, _) = oneshot::channel();
        assert_eq!(
            enqueue(
                &tx,
                Command {
                    deadline: Instant::now(),
                    reply
                }
            ),
            Err(Error::Busy)
        );
        drop(rx);
        let (reply, _) = oneshot::channel();
        assert_eq!(
            enqueue(
                &tx,
                Command {
                    deadline: Instant::now(),
                    reply
                }
            ),
            Err(Error::WorkerStopped)
        );
    }

    #[test]
    fn schema_constraints_foreign_keys_and_page_ceiling() {
        let temp = tempfile::tempdir().unwrap();
        let owner = StateOwner::acquire(&temp.path().join("state")).unwrap();
        let db = open(owner.root()).unwrap();
        assert!(db.execute("INSERT INTO monitor_state(monitor_id,status,failure_streak) VALUES (42,'unknown',0)", []).is_err());
        assert!(
            db.execute("INSERT INTO settings(id,revision) VALUES (2,1)", [])
                .is_err()
        );
        assert!(
            db.execute(
                "INSERT INTO health_state(id,updated_at_ms) VALUES (2,0)",
                []
            )
            .is_err()
        );
        let pages: i64 = db
            .pragma_query_value(None, "max_page_count", |r| r.get(0))
            .unwrap();
        let size: i64 = db
            .pragma_query_value(None, "page_size", |r| r.get(0))
            .unwrap();
        assert!(pages * size <= DATABASE_BYTES as i64);
        let journal: String = db
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        let sync: i64 = db
            .pragma_query_value(None, "synchronous", |r| r.get(0))
            .unwrap();
        assert_eq!((journal.as_str(), sync), ("wal", 2));
    }

    #[test]
    fn unknown_newer_and_inconsistent_schema_rejected() {
        for setup in [
            "CREATE TABLE foreign_data(id INTEGER)",
            "PRAGMA user_version=2",
            "PRAGMA user_version=1; PRAGMA application_id=1296912206",
            "CREATE TABLE schema_versions(version INTEGER, name TEXT); PRAGMA user_version=1; PRAGMA application_id=1296912206",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let owner = StateOwner::acquire(&temp.path().join("state")).unwrap();
            let path = owner.root().join("monitor.db");
            drop(private_file(&path).unwrap());
            let db = Connection::open(path).unwrap();
            db.execute_batch(setup).unwrap();
            drop(db);
            assert!(matches!(open(owner.root()), Err(Error::Schema)));
        }
    }

    #[test]
    fn query_progress_cancels_expensive_work() {
        let db = Connection::open_in_memory().unwrap();
        db.progress_handler(1000, Some(|| true)).unwrap();
        let result = db.query_row("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000000) SELECT sum(x) FROM n", [], |r| r.get::<_, i64>(0));
        assert!(
            matches!(result, Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::OperationInterrupted)
        );
    }

    #[test]
    fn disk_pressure_blocks_and_only_healthy_checkpoint_restores_admission() {
        assert!(write_admitted(WRITE_FREE_BYTES, 1, WAL_BYTES - 1, 1, true));
        for (free, db, wal, used, checkpoint) in [
            (WRITE_FREE_BYTES - 1, 1, 0, 1, true),
            (WRITE_FREE_BYTES, DATABASE_BYTES, 0, 1, true),
            (WRITE_FREE_BYTES, 1, WAL_BYTES, 1, true),
            (WRITE_FREE_BYTES, 1, 0, DISK_BYTES, true),
            (WRITE_FREE_BYTES, 1, 0, 1, false),
        ] {
            assert!(!write_admitted(free, db, wal, used, checkpoint));
        }
    }

    #[tokio::test]
    async fn expired_work_is_rejected_and_drop_stops_worker_with_surviving_clients() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("state");
        let mut storage = Storage::start(StateOwner::acquire(&root).unwrap())
            .await
            .unwrap();
        let client = storage.client();
        let (reply, receive) = oneshot::channel();
        enqueue(
            &client.tx,
            Command {
                deadline: Instant::now() - Duration::from_secs(1),
                reply,
            },
        )
        .unwrap();
        assert_eq!(receive.await.unwrap(), Err(Error::Deadline));
        let worker = storage.worker.take().unwrap();
        drop(storage);
        assert_eq!(client.health().await, Err(Error::Unavailable));
        tokio::time::timeout(
            Duration::from_secs(4),
            tokio::task::spawn_blocking(move || worker.join()),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .unwrap();
        assert!(StateOwner::acquire(&root).is_ok());
    }

    #[test]
    fn schema_caps_sessions_monitors_and_duplicate_slots_without_cascading_history() {
        let temp = tempfile::tempdir().unwrap();
        let owner = StateOwner::acquire(&temp.path().join("state")).unwrap();
        let db = open(owner.root()).unwrap();
        db.execute("INSERT INTO admin(id,username,password_hash,credential_generation,created_at_ms,updated_at_ms) VALUES(1,'fixture','noncredential-fixture',1,0,0)", []).unwrap();
        for value in 0..5u8 {
            db.execute("INSERT INTO sessions(token_hash,admin_id,csrf_binding,created_at_ms,last_used_at_ms,expires_at_ms) VALUES(?1,1,?1,0,0,1)", [vec![value;32]]).unwrap();
        }
        assert!(db.execute("INSERT INTO sessions(token_hash,admin_id,csrf_binding,created_at_ms,last_used_at_ms,expires_at_ms) VALUES(?1,1,?1,0,0,1)", [vec![6u8;32]]).is_err());
        for id in 1..=50 {
            db.execute("INSERT INTO monitors(id,name,kind,target,port,interval_seconds,deadline_seconds,failure_threshold,enabled,alerts,address_family,revision) VALUES(?1,'fixture','tcp','example.invalid',443,60,5,3,0,0,'auto',1)", [id]).unwrap();
        }
        assert!(db.execute("INSERT INTO monitors(id,name,kind,target,port,interval_seconds,deadline_seconds,failure_threshold,enabled,alerts,address_family,revision) VALUES(51,'fixture','tcp','example.invalid',443,60,5,3,0,0,'auto',1)", []).is_err());
        let insert = "INSERT INTO probe_results(monitor_id,revision,scheduled_slot_ms,observed_at_ms,outcome) VALUES(1,1,0,0,'unknown')";
        db.execute(insert, []).unwrap();
        assert!(db.execute(insert, []).is_err());
        assert!(db.execute("DELETE FROM monitors WHERE id=1", []).is_err());
        db.execute("INSERT INTO hourly_stats(monitor_id,hour_ms,expected,successful,failed,unknown,latency_sum_ms,latency_count) VALUES(1,0,5,3,1,1,0,0)", []).unwrap();
        assert!(db.execute("UPDATE hourly_stats SET unknown=0", []).is_err());
    }

    #[test]
    fn sqlite_full_rolls_back_without_partial_configuration() {
        let temp = tempfile::tempdir().unwrap();
        let owner = StateOwner::acquire(&temp.path().join("state")).unwrap();
        let mut db = open(owner.root()).unwrap();
        let pages: i64 = db
            .pragma_query_value(None, "page_count", |r| r.get(0))
            .unwrap();
        db.pragma_update(None, "max_page_count", pages + 1).unwrap();
        {
            let tx = db.transaction().unwrap();
            tx.execute("UPDATE settings SET revision=2", []).unwrap();
            tx.execute_batch("CREATE TABLE capacity_fixture(value BLOB)")
                .unwrap();
            let result = tx.execute(
                "INSERT INTO capacity_fixture(value) VALUES(zeroblob(262144))",
                [],
            );
            assert!(
                matches!(result, Err(rusqlite::Error::SqliteFailure(err, _)) if err.code == rusqlite::ErrorCode::DiskFull)
            );
        }
        assert_eq!(
            db.query_row("SELECT revision FROM settings", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
