use rusqlite::{
    Connection, TransactionBehavior,
    backup::{Backup, StepResult},
};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crate::{
    bounds::{DATABASE_BYTES, DISK_BYTES, WAL_BYTES, WARN_FREE_BYTES},
    error::{Error, Result},
    ownership::private_file,
};

pub(super) const APPLICATION_ID: i64 = 0x4d4d4f4e;
pub(super) const MIGRATIONS: &[(&str, &str)] = &[
    (
        "foundation",
        include_str!("../../migrations/0001_foundation.sql"),
    ),
    (
        "monitoring",
        include_str!("../../migrations/0002_monitoring.sql"),
    ),
    (
        "monitor_identity",
        include_str!("../../migrations/0003_monitor_identity.sql"),
    ),
    (
        "notifications",
        include_str!("../../migrations/0004_notifications.sql"),
    ),
];
const RESERVED_BYTES: u64 = (128 + 64 + 800) * 1024 * 1024;

/// Conservative worst case: database ceiling, WAL allowance, one recoverable
/// snapshot, binaries/logs/headroom. No automatic deletion or migration downgrade.
pub(super) fn preflight(used: u64, db: u64, working: u64, free: u64, backup: bool) -> Result<()> {
    let growth = DATABASE_BYTES.saturating_sub(db) + WAL_BYTES.saturating_sub(working);
    // Committed pages may still be in WAL: a snapshot can be larger than the
    // current main file. Reserve its full ceiling rather than copying db.len().
    let backup_bytes = if backup { DATABASE_BYTES } else { 0 };
    let predicted = used
        .checked_add(growth)
        .and_then(|v| v.checked_add(backup_bytes))
        .and_then(|v| v.checked_add(RESERVED_BYTES))
        .ok_or(Error::DiskPressure)?;
    let required = growth
        .checked_add(backup_bytes)
        .and_then(|v| v.checked_add(WARN_FREE_BYTES))
        .ok_or(Error::DiskPressure)?;
    if db > DATABASE_BYTES || working > WAL_BYTES || predicted > DISK_BYTES || free < required {
        return Err(Error::DiskPressure);
    }
    Ok(())
}

/// Validate identity before changing journal mode or applying any migration.
pub(super) fn inspect(connection: &Connection) -> Result<usize> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .map_err(|_| Error::Schema)?;
    let identity: i64 = connection
        .pragma_query_value(None, "application_id", |r| r.get(0))
        .map_err(|_| Error::Schema)?;
    if version < 0
        || version as usize > MIGRATIONS.len()
        || (version > 0 && identity != APPLICATION_ID)
        || (version == 0 && identity != 0)
    {
        return Err(Error::Schema);
    }
    if version == 0 {
        let tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )
            .map_err(|_| Error::Schema)?;
        if tables != 0 {
            return Err(Error::Schema);
        }
    } else {
        let mut stmt = connection
            .prepare(
                "SELECT version, substr(name,1,101) FROM schema_versions ORDER BY version LIMIT ?1",
            )
            .map_err(|_| Error::Schema)?;
        let rows: Vec<(i64, String)> = stmt
            .query_map([MIGRATIONS.len() as i64 + 1], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .map_err(|_| Error::Schema)?
            .collect::<rusqlite::Result<_>>()
            .map_err(|_| Error::Schema)?;
        if rows.len() != version as usize
            || rows
                .iter()
                .enumerate()
                .any(|(index, (v, name))| *v != index as i64 + 1 || name != MIGRATIONS[index].0)
        {
            return Err(Error::Schema);
        }
    }
    let integrity: String = connection
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .map_err(|_| Error::Schema)?;
    if integrity != "ok" {
        return Err(Error::Schema);
    }
    if version > 0 {
        let tables: i64 = connection.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('schema_versions','admin','sessions','monitors','schedule_epochs','monitor_state','probe_results','hourly_stats','incidents','notification_outbox','settings','request_keys','health_state','audit_events')", [], |r| r.get(0)).map_err(|_| Error::Schema)?;
        if tables != 14 {
            return Err(Error::Schema);
        }
        if version >= 2 {
            let consistent:bool=connection.query_row("SELECT raw_rows=(SELECT count(*) FROM probe_results) AND hourly_rows=(SELECT count(*) FROM hourly_stats) AND epoch_rows=(SELECT count(*) FROM schedule_epochs) FROM monitoring_counts WHERE id=1",[],|r|r.get(0)).map_err(|_| Error::Schema)?;
            if !consistent {
                return Err(Error::Schema);
            }
            connection.prepare("SELECT epoch_id,next_due_ms,cadence,first_failure_ms,last_result_json,system_error FROM monitor_state LIMIT 0").map_err(|_| Error::Schema)?;
            connection
                .prepare("SELECT epoch_id FROM probe_results LIMIT 0")
                .map_err(|_| Error::Schema)?;
        }
        if version >= 3 {
            let valid:bool=connection.query_row("SELECT last_id<=min(0,coalesce((SELECT min(id) FROM monitors),0),coalesce((SELECT min(CAST(result_reference AS INTEGER)) FROM request_keys WHERE operation='monitor_create'),0)) FROM monitor_identity_allocator WHERE id=1",[],|r|r.get(0)).map_err(|_| Error::Schema)?;
            if !valid {
                return Err(Error::Schema);
            }
        }
        if version >= 4 {
            connection
                .prepare("SELECT telegram_down_delivered FROM incidents LIMIT 0")
                .map_err(|_| Error::Schema)?;
            connection
                .query_row(
                    "SELECT cooldown_until_ms FROM telegram_state WHERE id=1",
                    [],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(|_| Error::Schema)?;
            connection
                .prepare("SELECT terminal_at_ms,error_category FROM notification_outbox LIMIT 0")
                .map_err(|_| Error::Schema)?;
        }
        for query in [
            "SELECT revision FROM settings WHERE id=1",
            "SELECT updated_at_ms FROM health_state WHERE id=1",
        ] {
            connection
                .query_row(query, [], |r| r.get::<_, i64>(0))
                .map_err(|_| Error::Schema)?;
        }
        let mut statement = connection
            .prepare("PRAGMA foreign_key_check")
            .map_err(|_| Error::Schema)?;
        if statement
            .query([])
            .map_err(|_| Error::Schema)?
            .next()
            .map_err(|_| Error::Schema)?
            .is_some()
        {
            return Err(Error::Schema);
        }
    }
    Ok(version as usize)
}

pub(super) fn migrate(connection: &mut Connection, root: &Path, version: usize) -> Result<()> {
    if version == MIGRATIONS.len() {
        return Ok(());
    }
    let used = directory_bytes(root)?;
    let db = file_bytes(&root.join("monitor.db"))?;
    let working = file_bytes(&root.join("monitor.db-wal"))?
        .checked_add(file_bytes(&root.join("monitor.db-shm"))?)
        .ok_or(Error::DiskPressure)?;
    preflight(
        used,
        db,
        working,
        fs2::available_space(root).map_err(|_| Error::Io)?,
        version > 0,
    )?;
    if version > 0 {
        snapshot(connection, &root.join("pre-migration.db"))?;
    }
    for (index, (name, sql)) in MIGRATIONS.iter().enumerate().skip(version) {
        apply(connection, index as i64 + 1, name, sql)?;
    }
    Ok(())
}

pub(super) fn apply(
    connection: &mut Connection,
    version: i64,
    name: &str,
    sql: &str,
) -> Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::Schema)?
        .as_millis();
    let stamp = i64::try_from(stamp).map_err(|_| Error::Schema)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| Error::Storage)?;
    tx.execute_batch(sql).map_err(|_| Error::Schema)?;
    tx.execute(
        "INSERT INTO schema_versions(version,name,applied_at_ms) VALUES (?1,?2,?3)",
        (version, name, stamp),
    )
    .map_err(|_| Error::Schema)?;
    tx.pragma_update(None, "application_id", APPLICATION_ID)
        .map_err(|_| Error::Schema)?;
    tx.pragma_update(None, "user_version", version)
        .map_err(|_| Error::Schema)?;
    tx.commit().map_err(|_| Error::Storage)
}

fn snapshot(source: &Connection, path: &Path) -> Result<()> {
    // Refuse to replace an operator's existing recovery point.
    if path.try_exists().map_err(|_| Error::Io)? {
        return Err(Error::DiskPressure);
    }
    drop(private_file(path)?);
    let mut target = Connection::open(path).map_err(|_| Error::Storage)?;
    let backup = Backup::new(source, &mut target).map_err(|_| Error::Storage)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if Instant::now() >= deadline {
            return Err(Error::Deadline);
        }
        match backup.step(128).map_err(|_| Error::Storage)? {
            StepResult::Done => break,
            StepResult::More => (),
            StepResult::Busy | StepResult::Locked => return Err(Error::Busy),
            _ => return Err(Error::Storage),
        }
    }
    drop(backup);
    let integrity: String = target
        .query_row("PRAGMA quick_check(1)", [], |r| r.get(0))
        .map_err(|_| Error::Storage)?;
    if integrity != "ok" {
        return Err(Error::Storage);
    }
    Ok(())
}

pub(super) fn file_bytes(path: &Path) -> Result<u64> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => Ok(meta.len()),
        Ok(_) => Err(Error::Io),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(_) => Err(Error::Io),
    }
}

/// Bounded accounting, including unknown files; no symlink traversal. A state
/// directory with excessive entries is refused rather than scanned indefinitely.
pub(super) fn directory_bytes(root: &Path) -> Result<u64> {
    fn walk(root: &Path, depth: usize, entries: &mut usize) -> Result<u64> {
        if depth > 8 {
            return Err(Error::DiskPressure);
        }
        let mut bytes = 0u64;
        for entry in fs::read_dir(root).map_err(|_| Error::Io)? {
            *entries += 1;
            if *entries > 4096 {
                return Err(Error::DiskPressure);
            }
            let path = entry.map_err(|_| Error::Io)?.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| Error::Io)?;
            let size = if metadata.file_type().is_symlink() {
                return Err(Error::Io);
            } else if metadata.is_dir() {
                walk(&path, depth + 1, entries)?
            } else if metadata.is_file() {
                metadata.len()
            } else {
                return Err(Error::Io);
            };
            bytes = bytes.checked_add(size).ok_or(Error::DiskPressure)?;
        }
        Ok(bytes)
    }
    walk(root, 0, &mut 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn identity_upgrade_reserves_deleted_legacy_ids_and_existing_replay_references() {
        use crate::{
            monitoring::MonitorConfig,
            storage::{
                Storage,
                monitoring::{Operation, Value},
            },
        };
        use sha2::Digest;
        let dir = tempfile::tempdir().unwrap();
        let owner = crate::ownership::StateOwner::acquire(&dir.path().join("state")).unwrap();
        let root = owner.root().to_path_buf();
        let path = root.join("monitor.db");
        drop(private_file(&path).unwrap());
        let mut db = Connection::open(&path).unwrap();
        for (index, (name, sql)) in MIGRATIONS.iter().take(2).enumerate() {
            apply(&mut db, index as i64 + 1, name, sql).unwrap();
        }
        db.execute(
            "INSERT INTO admin VALUES(1,'fixture','noncredential-fixture',1,0,0)",
            [],
        )
        .unwrap();
        db.execute("INSERT INTO monitors(id,name,kind,target,port,interval_seconds,deadline_seconds,failure_threshold,enabled,alerts,address_family,revision) VALUES(10,'legacy','tcp','fixture.invalid',443,60,5,3,0,0,'auto',1)",[]).unwrap();
        db.execute("INSERT INTO schedule_epochs(monitor_id,revision,anchor_at_ms,interval_seconds,enabled_start_ms,enabled_end_ms) VALUES(10,1,0,60,0,0)",[]).unwrap();
        let epoch = db.last_insert_rowid();
        db.execute("INSERT INTO monitor_state(monitor_id,status,failure_streak,epoch_id,next_due_ms) VALUES(10,'paused',0,?1,0)",[epoch]).unwrap();
        let original:MonitorConfig=serde_json::from_str(r#"{"name":"deleted legacy","kind":"tcp","target":"fixture.invalid","port":443,"enabled":false}"#).unwrap();
        let digest = sha2::Sha256::digest(serde_json::to_vec(&original).unwrap());
        db.execute("INSERT INTO request_keys(admin_id,key,operation,request_digest,result_reference,created_at_ms) VALUES(1,'legacy-key','monitor_create',?1,'11',0)",[digest.as_slice()]).unwrap();
        // ID 11 was the deleted highest ID, while only ID 10 remains live.
        drop(db);
        let storage = Storage::start(owner).await.unwrap();
        let client = storage.client();
        assert!(matches!(
            client
                .monitoring(Operation::Create {
                    config: original.clone(),
                    now: 1,
                    jitter_ms: 0,
                    key: Some("legacy-key".into())
                })
                .await,
            Err(Error::Conflict)
        ));
        let mut replacement = original.clone();
        replacement.name = "replacement".into();
        let b = match client
            .monitoring(Operation::Create {
                config: replacement,
                now: 1,
                jitter_ms: 0,
                key: None,
            })
            .await
            .unwrap()
        {
            Value::Monitor(m) => m,
            _ => panic!("monitor"),
        };
        assert!(b.id < 0);
        for id in [10, 11] {
            assert_ne!(b.id, id);
        }
        assert!(matches!(
            client
                .monitoring(Operation::Edit {
                    id: 11,
                    revision: 1,
                    config: original,
                    now: 2,
                    jitter_ms: 0
                })
                .await,
            Err(Error::NotFound)
        ));
        assert!(matches!(
            client
                .monitoring(Operation::Delete {
                    id: 11,
                    revision: 1,
                    now: 2
                })
                .await,
            Err(Error::NotFound)
        ));
        storage.shutdown().await.unwrap();
        let snapshot = Connection::open(root.join("pre-migration.db")).unwrap();
        assert_eq!(inspect(&snapshot).unwrap(), 2);
        let storage = Storage::start(crate::ownership::StateOwner::acquire(&root).unwrap())
            .await
            .unwrap();
        let client = storage.client();
        let next: MonitorConfig = serde_json::from_str(
            r#"{"name":"next","kind":"tcp","target":"fixture.invalid","port":443,"enabled":false}"#,
        )
        .unwrap();
        let next = match client
            .monitoring(Operation::Create {
                config: next,
                now: 3,
                jitter_ms: 0,
                key: None,
            })
            .await
            .unwrap()
        {
            Value::Monitor(m) => m,
            _ => panic!("monitor"),
        };
        assert!(next.id < b.id);
        assert!(
            client
                .monitors()
                .await
                .unwrap()
                .iter()
                .any(|m| m.id == 10 && m.config.name == "legacy")
        );
        storage.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn monitoring_upgrade_preserves_definitions_and_keeps_v1_recovery_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let owner = crate::ownership::StateOwner::acquire(&dir.path().join("state")).unwrap();
        let root = owner.root().to_path_buf();
        let path = root.join("monitor.db");
        drop(private_file(&path).unwrap());
        let mut db = Connection::open(&path).unwrap();
        apply(&mut db, 1, MIGRATIONS[0].0, MIGRATIONS[0].1).unwrap();
        db.execute("INSERT INTO monitors(id,name,kind,target,port,interval_seconds,deadline_seconds,failure_threshold,enabled,alerts,address_family,revision) VALUES(1,'legacy','tcp','fixture.invalid',443,60,5,3,0,0,'auto',1)",[]).unwrap();
        db.execute("INSERT INTO probe_results(monitor_id,revision,scheduled_slot_ms,observed_at_ms,outcome) VALUES(1,1,0,0,'unknown')",[]).unwrap();
        drop(db);
        let storage = crate::storage::Storage::start(owner).await.unwrap();
        let monitors = storage.client().monitors().await.unwrap();
        assert_eq!(monitors.len(), 1);
        assert_eq!(monitors[0].config.name, "legacy");
        assert_eq!(monitors[0].state.status, crate::monitoring::Status::Paused);
        storage.shutdown().await.unwrap();
        let old = Connection::open(root.join("pre-migration.db")).unwrap();
        assert_eq!(inspect(&old).unwrap(), 1);
        let new = Connection::open(path).unwrap();
        assert_eq!(inspect(&new).unwrap(), MIGRATIONS.len());
        assert_eq!(
            new.query_row("SELECT count(*) FROM probe_results", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn migration_failure_rolls_back_ddl_data_and_version() {
        let mut connection = Connection::open_in_memory().unwrap();
        apply(&mut connection, 1, MIGRATIONS[0].0, MIGRATIONS[0].1).unwrap();
        assert_eq!(
            apply(
                &mut connection,
                2,
                "bad",
                "CREATE TABLE transient(id INTEGER); INSERT INTO settings(id,revision) VALUES(2,1);"
            ),
            Err(Error::Schema)
        );
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        let tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='transient'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let migrations: i64 = connection
            .query_row("SELECT count(*) FROM schema_versions", [], |r| r.get(0))
            .unwrap();
        assert_eq!((version, tables, migrations), (1, 0, 1));
    }

    #[test]
    fn preflight_includes_backup_and_firm_disk_reserve() {
        // A small main file does not justify a small recovery reservation when
        // committed pages can still live in WAL.
        assert_eq!(
            preflight(256 * 1024 * 1024 + 4096, 4096, 0, DISK_BYTES, true),
            Err(Error::DiskPressure)
        );
        assert!(
            preflight(
                DATABASE_BYTES + WAL_BYTES,
                DATABASE_BYTES,
                WAL_BYTES,
                DISK_BYTES,
                true
            )
            .is_ok()
        );
        assert_eq!(
            preflight(
                DATABASE_BYTES + WAL_BYTES + 1,
                DATABASE_BYTES,
                WAL_BYTES,
                DISK_BYTES,
                true
            ),
            Err(Error::DiskPressure)
        );
        assert_eq!(
            preflight(0, 0, 0, WARN_FREE_BYTES, false),
            Err(Error::DiskPressure)
        );
        assert_eq!(
            preflight(u64::MAX, 0, 0, u64::MAX, false),
            Err(Error::DiskPressure)
        );
    }

    #[tokio::test]
    async fn notifications_upgrade_preserves_v3_jobs_and_supported_recovery_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let owner = crate::ownership::StateOwner::acquire(&dir.path().join("state")).unwrap();
        let root = owner.root().to_path_buf();
        let path = root.join("monitor.db");
        drop(private_file(&path).unwrap());
        let mut db = Connection::open(&path).unwrap();
        for (index, (name, sql)) in MIGRATIONS.iter().take(3).enumerate() {
            apply(&mut db, index as i64 + 1, name, sql).unwrap();
        }
        db.execute("INSERT INTO notification_outbox(event_identity,kind,destination_revision,payload,state,attempts,next_attempt_at_ms,created_at_ms,expires_at_ms,message_id) VALUES('legacy-pending','test',1,'{}','pending',2,61000,1000,21601000,NULL),('legacy-delivered','test',1,'{}','delivered',1,2000,1000,21601000,42)",[]).unwrap();
        drop(db);
        let storage = crate::storage::Storage::start(owner).await.unwrap();
        storage.shutdown().await.unwrap();
        let old = Connection::open(root.join("pre-migration.db")).unwrap();
        assert_eq!(inspect(&old).unwrap(), 3);
        let new = Connection::open(path).unwrap();
        assert_eq!(inspect(&new).unwrap(), 4);
        let row:(String,i64,i64)=new.query_row("SELECT state,attempts,next_attempt_at_ms FROM notification_outbox WHERE event_identity='legacy-pending'",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!(row, ("pending".into(), 2, 61000));
        let row:(i64,i64)=new.query_row("SELECT message_id,terminal_at_ms FROM notification_outbox WHERE event_identity='legacy-delivered'",[],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
        assert_eq!(row, (42, 1000));
        assert_eq!(
            new.query_row("SELECT cooldown_until_ms FROM telegram_state", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }

    #[test]
    fn supported_snapshot_survives_later_changes_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("source.db");
        drop(private_file(&source_path).unwrap());
        let mut source = Connection::open(&source_path).unwrap();
        source
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0")
            .unwrap();
        apply(&mut source, 1, MIGRATIONS[0].0, MIGRATIONS[0].1).unwrap();
        let path = dir.path().join("backup.db");
        snapshot(&source, &path).unwrap();
        assert!(file_bytes(&path).unwrap() > file_bytes(&source_path).unwrap());
        source
            .execute("UPDATE settings SET revision=2", [])
            .unwrap();
        assert_eq!(snapshot(&source, &path), Err(Error::DiskPressure));
        let saved = Connection::open(&path).unwrap();
        let revision: i64 = saved
            .query_row("SELECT revision FROM settings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(revision, 1);
    }
}
