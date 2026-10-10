use minimalmonitor::{
    bounds::WAL_BYTES,
    error::Error,
    ownership::StateOwner,
    storage::{Storage, security::Operation},
};
use rusqlite::Connection;

#[tokio::test]
async fn pinned_wal_pressure_recovers_on_the_same_worker_without_restart() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    assert!(client.health().await.unwrap().writable);

    // Controlled, bounded fixture connections stand in for a long read and
    // committed writes. WP-02 account mutations use the real worker admission.
    let path = root.join("monitor.db");
    let reader = tokio::task::spawn_blocking(move || {
        let writer = Connection::open(&path).unwrap();
        writer
            .execute_batch(
                "PRAGMA wal_autocheckpoint=0; PRAGMA cache_size=-2048;
                 CREATE TABLE wal_fixture(id INTEGER PRIMARY KEY, payload BLOB);
                 INSERT INTO wal_fixture VALUES(1, zeroblob(524288));",
            )
            .unwrap();
        let reader = Connection::open(path).unwrap();
        reader.execute_batch("BEGIN").unwrap();
        assert_eq!(
            reader
                .query_row(
                    "SELECT hex(substr(payload,1,1)) FROM wal_fixture",
                    [],
                    |r| { r.get::<_, String>(0) }
                )
                .unwrap(),
            "00"
        );
        // At most 36 MiB of payload writes; the old snapshot prevents recycling.
        for _ in 0..72 {
            writer
                .execute("UPDATE wal_fixture SET payload=randomblob(524288)", [])
                .unwrap();
        }
        writer
            .execute("UPDATE settings SET revision=72 WHERE id=1", [])
            .unwrap();
        reader
    })
    .await
    .unwrap();
    assert!(
        std::fs::metadata(root.join("monitor.db-wal"))
            .unwrap()
            .len()
            >= WAL_BYTES
    );
    for _ in 0..3 {
        let health = client.health().await.unwrap();
        assert!(!health.writable);
        assert!(health.disk_warning);
        assert!(matches!(
            client
                .security(Operation::Local {
                    user: "fixture".into(),
                    hash: "noncredential-fixture".into(),
                    reset: false,
                    now: minimalmonitor::auth::now_ms().unwrap()
                })
                .await,
            Err(Error::DiskPressure)
        ));
    }
    assert!(
        std::fs::metadata(root.join("monitor.db-wal"))
            .unwrap()
            .len()
            >= WAL_BYTES
    );

    tokio::task::spawn_blocking(move || {
        // The blocked reclamation attempts preserved the reader's snapshot.
        assert_eq!(
            reader
                .query_row(
                    "SELECT hex(substr(payload,1,1)) FROM wal_fixture",
                    [],
                    |r| { r.get::<_, String>(0) }
                )
                .unwrap(),
            "00"
        );
        reader.execute_batch("ROLLBACK").unwrap();
        reader.close().unwrap();
    })
    .await
    .unwrap();

    // No Storage restart or connection replacement: this same client recovers.
    assert!(client.health().await.unwrap().writable);
    client
        .security(Operation::Local {
            user: "fixture".into(),
            hash: "noncredential-fixture".into(),
            reset: false,
            now: minimalmonitor::auth::now_ms().unwrap(),
        })
        .await
        .unwrap();
    assert!(
        std::fs::metadata(root.join("monitor.db-wal"))
            .unwrap()
            .len()
            < WAL_BYTES
    );
    let path = root.join("monitor.db");
    tokio::task::spawn_blocking(move || {
        let db = Connection::open(path).unwrap();
        assert_eq!(
            db.query_row("SELECT revision FROM settings WHERE id=1", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            72
        );
        db.execute("UPDATE settings SET revision=73 WHERE id=1", [])
            .unwrap();
        assert_eq!(
            db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "ok"
        );
    })
    .await
    .unwrap();
    assert!(client.health().await.unwrap().writable);
    storage.shutdown().await.unwrap();
}
