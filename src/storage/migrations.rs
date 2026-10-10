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
pub(super) const MIGRATIONS: &[(&str, &str)] = &[(
    "foundation",
    include_str!("../../migrations/0001_foundation.sql"),
)];
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
