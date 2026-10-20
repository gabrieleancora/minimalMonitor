//! WP03-R1/R2: real row-capacity recovery and durable monitor identity.
use minimalmonitor::{
    bounds::WorkBounds,
    destination::{Policy, Rule},
    error::{Error, Result},
    monitoring::*,
    ownership::StateOwner,
    probes::Probes,
    scheduler::{self, Clock, SchedulerHealth},
    storage::{
        Storage, StorageClient,
        monitoring::{Operation, Value},
    },
};
use rusqlite::{Connection, params};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn read(&self) -> Result<(i64, Duration)> {
        let now = self.0.load(Ordering::Acquire);
        Ok((now, Duration::from_millis(now as u64)))
    }
}
fn config(port: u16, enabled: bool) -> MonitorConfig {
    MonitorConfig {
        name: "review fixture".into(),
        kind: Kind::Tcp,
        target: "127.0.0.1".into(),
        port: Some(port),
        interval_seconds: 30,
        deadline_seconds: 1,
        failure_threshold: 3,
        enabled,
        alerts: false,
        address_family: Family::Auto,
        accepted_statuses: vec![[200, 299]],
    }
}
async fn create(
    client: &StorageClient,
    config: MonitorConfig,
    now: i64,
    key: Option<&str>,
) -> Monitor {
    match client
        .monitoring(Operation::Create {
            config,
            now,
            jitter_ms: 0,
            key: key.map(str::to_owned),
        })
        .await
        .unwrap()
    {
        Value::Monitor(m) => *m,
        _ => panic!("monitor"),
    }
}
async fn wait_for(client: &StorageClient, predicate: impl Fn(&Monitor) -> bool) -> Monitor {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let m = client.monitors().await.unwrap().remove(0);
            if predicate(&m) {
                break m;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}
fn scalar(db: &Connection, query: &str) -> i64 {
    db.query_row(query, [], |r| r.get(0)).unwrap()
}
fn fill_raw(root: &Path, id: i64) {
    let mut db = Connection::open(root.join("monitor.db")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA cache_size=-2048; PRAGMA wal_autocheckpoint=1000; PRAGMA synchronous=FULL").unwrap();
    // Actual rows and trigger-maintained counters. No counter overrides. All
    // fixture observations are older than both retained history windows.
    for batch in 0..101 {
        let tx = db.transaction().unwrap();
        tx.execute("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<9999) INSERT INTO probe_results(monitor_id,revision,scheduled_slot_ms,observed_at_ms,outcome) SELECT ?1,1,(?2+x)*30000,(?2+x)*30000,'unknown' FROM n",params![id,batch*10000]).unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(scalar(&db, "SELECT count(*) FROM probe_results"), 1_010_000);
    assert_eq!(
        scalar(&db, "SELECT raw_rows FROM monitoring_counts"),
        1_010_000
    );
    assert_eq!(scalar(&db, "PRAGMA wal_checkpoint(TRUNCATE)"), 0);
}
fn fill_epochs(root: &Path, id: i64, now: i64, expired: bool) {
    let end = now - if expired { 100 * DAY_MS } else { DAY_MS };
    fill_epochs_at(root, id, end);
}
fn fill_epochs_at(root: &Path, id: i64, end: i64) {
    let mut db = Connection::open(root.join("monitor.db")).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA cache_size=-2048")
        .unwrap();
    let tx = db.transaction().unwrap();
    tx.execute("WITH RECURSIVE n(x) AS (VALUES(0) UNION ALL SELECT x+1 FROM n WHERE x<9998) INSERT INTO schedule_epochs(monitor_id,revision,anchor_at_ms,interval_seconds,enabled_start_ms,enabled_end_ms,aggregated_through_ms) SELECT ?1,1,?2,30,?2,?2,?2 FROM n",params![id,end]).unwrap();
    tx.commit().unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM schedule_epochs"), 10_000);
    assert_eq!(
        scalar(&db, "SELECT epoch_rows FROM monitoring_counts"),
        10_000
    );
    assert_eq!(scalar(&db, "PRAGMA wal_checkpoint(TRUNCATE)"), 0);
}
#[tokio::test]
async fn clock_jump_reclaims_eligible_capacity_before_reanchoring_and_resumes_probes() {
    struct JumpClock {
        wall: AtomicI64,
        monotonic: AtomicI64,
    }
    impl Clock for JumpClock {
        fn read(&self) -> Result<(i64, Duration)> {
            Ok((
                self.wall.load(Ordering::Acquire),
                Duration::from_millis(self.monotonic.load(Ordering::Acquire) as u64),
            ))
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let now = 500 * DAY_MS;
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = target.local_addr().unwrap().port();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let health = SchedulerHealth::default();
    let clock = Arc::new(JumpClock {
        wall: AtomicI64::new(now),
        monotonic: AtomicI64::new(now),
    });
    let policy = Policy::new(vec![Rule {
        cidr: "127.0.0.1/32".into(),
        host: None,
        ports: vec![port],
        all_tcp_ports: false,
        ping: false,
    }])
    .unwrap();
    let (stop, receive) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(scheduler::run_with_clock(
        client.clone(),
        Probes::new(policy, WorkBounds::default(), vec![]).unwrap(),
        health.clone(),
        clock.clone(),
        async {
            let _ = receive.await;
        },
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !health.ready(now) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let m = match client
        .monitoring(Operation::Create {
            config: config(port, true),
            now,
            jitter_ms: 29_999,
            key: None,
        })
        .await
        .unwrap()
    {
        Value::Monitor(m) => *m,
        _ => panic!("monitor"),
    };
    // These real epochs become eligible only after the forward wall-clock jump.
    let fixture = root.clone();
    let id = m.id;
    tokio::task::spawn_blocking(move || fill_epochs_at(&fixture, id, now - 90 * DAY_MS + 2000))
        .await
        .unwrap();
    clock.wall.store(now + 5000, Ordering::Release);
    let resumed = wait_for(&client, |current| current.epoch != m.epoch).await;
    let db = Connection::open(root.join("monitor.db")).unwrap();
    assert_eq!(
        scalar(&db, "SELECT epoch_rows FROM monitoring_counts"),
        9501
    );
    assert_eq!(
        db.query_row(
            "SELECT discontinuity FROM schedule_epochs WHERE id=?1",
            [resumed.epoch],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "clock_jump"
    );
    let delta = resumed.state.next_due_ms - (now + 5000);
    clock.monotonic.store(now + delta, Ordering::Release);
    clock
        .wall
        .store(resumed.state.next_due_ms, Ordering::Release);
    tokio::time::timeout(Duration::from_secs(3), target.accept())
        .await
        .unwrap()
        .unwrap();
    let up = wait_for(&client, |current| current.state.status == Status::Up).await;
    assert!(health.ready(up.state.last_completed_ms.unwrap()));
    stop.send(()).unwrap();
    worker.await.unwrap().unwrap();
    storage.shutdown().await.unwrap();
}
async fn recovery_case(raw: bool) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let now = 500 * DAY_MS;
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = target.local_addr().unwrap().port();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let mut target_config = config(port, true);
    target_config.kind = Kind::Http;
    target_config.target = format!("http://127.0.0.1:{port}/");
    target_config.port = None;
    let m = create(&storage.client(), target_config, now - 120_000, None).await;
    storage.shutdown().await.unwrap();
    let fixture = root.clone();
    let id = m.id;
    tokio::task::spawn_blocking(move || {
        if raw {
            fill_raw(&fixture, id)
        } else {
            fill_epochs(&fixture, id, now, true)
        }
    })
    .await
    .unwrap();
    let db = Connection::open(root.join("monitor.db")).unwrap();
    // Retention must not sacrifice an old open incident or a young closed one.
    db.execute("INSERT INTO incidents(monitor_id,revision,first_failure_at_ms,confirmed_at_ms,last_evidence_at_ms) VALUES(?1,1,0,0,0)",[m.id]).unwrap();
    let incident = db.last_insert_rowid();
    db.execute(
        "UPDATE monitor_state SET active_incident_id=?1 WHERE monitor_id=?2",
        params![incident, m.id],
    )
    .unwrap();
    db.execute("INSERT INTO incidents(monitor_id,revision,first_failure_at_ms,confirmed_at_ms,ended_at_ms,end_reason,last_evidence_at_ms) VALUES(?1,1,?2,?2,?2,'recovered',?2)",params![m.id,now-DAY_MS]).unwrap();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let clock = Arc::new(TestClock(AtomicI64::new(now)));
    let health = SchedulerHealth::default();
    let policy = Policy::new(vec![Rule {
        cidr: "127.0.0.1/32".into(),
        host: None,
        ports: vec![port],
        all_tcp_ports: false,
        ping: false,
    }])
    .unwrap();
    let (stop, receive) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(scheduler::run_with_clock(
        client.clone(),
        Probes::new(policy, WorkBounds::default(), vec![]).unwrap(),
        health.clone(),
        clock.clone(),
        async {
            let _ = receive.await;
        },
    ));
    let resumed = wait_for(&client, |current| current.epoch != m.epoch).await;
    // One bounded cleanup transaction precedes catch-up (4 unknown slots).
    if raw {
        assert_eq!(
            scalar(&db, "SELECT raw_rows FROM monitoring_counts"),
            1_009_504
        );
    } else {
        assert_eq!(
            scalar(&db, "SELECT epoch_rows FROM monitoring_counts"),
            9_501
        );
    }
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM probe_results WHERE epoch_id!=0 AND outcome='unknown'"
        ),
        4
    );
    assert_eq!(scalar(&db, "SELECT sum(expected) FROM hourly_stats"), 4);
    assert_eq!(scalar(&db, "SELECT sum(unknown) FROM hourly_stats"), 4);
    assert_eq!(scalar(&db, "SELECT count(*) FROM incidents"), 2);
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM incidents WHERE ended_at_ms IS NULL AND observation_gap=1"
        ),
        1
    );
    clock.0.store(resumed.state.next_due_ms, Ordering::Release);
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(3), target.accept())
        .await
        .unwrap()
        .unwrap();
    // Even zero jitter cannot complete a result until the assertions above run.
    let mut request = [0; 4096];
    assert!(stream.read(&mut request).await.unwrap() > 0);
    stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let up = wait_for(&client, |current| current.state.status == Status::Up).await;
    assert_eq!(up.epoch, resumed.epoch);
    assert!(health.ready(up.state.last_completed_ms.unwrap()));
    assert_eq!(scalar(&db, "SELECT sum(successful) FROM hourly_stats"), 1);
    assert_eq!(
        scalar(
            &db,
            "SELECT count(*) FROM incidents WHERE id=1 AND end_reason='recovered'"
        ),
        1
    );
    stop.send(()).unwrap();
    worker.await.unwrap().unwrap();
    storage.shutdown().await.unwrap();
}
#[tokio::test]
async fn restart_reclaims_real_raw_capacity_and_resumes_without_manual_intervention() {
    recovery_case(true).await;
}
#[tokio::test]
async fn restart_reclaims_real_epoch_capacity_and_preserves_protected_incidents() {
    recovery_case(false).await;
}
#[tokio::test]
async fn unreclaimable_epoch_pressure_stays_degraded_without_probes_or_pruning_young_data() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let now = 500 * DAY_MS;
    let target = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = target.local_addr().unwrap().port();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let m = create(&storage.client(), config(port, true), now - 120_000, None).await;
    storage.shutdown().await.unwrap();
    let fixture = root.clone();
    tokio::task::spawn_blocking(move || fill_epochs(&fixture, m.id, now, false))
        .await
        .unwrap();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let health = SchedulerHealth::default();
    let clock = Arc::new(TestClock(AtomicI64::new(now)));
    let policy = Policy::new(vec![Rule {
        cidr: "127.0.0.1/32".into(),
        host: None,
        ports: vec![port],
        all_tcp_ports: false,
        ping: false,
    }])
    .unwrap();
    let (stop, receive) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(scheduler::run_with_clock(
        client.clone(),
        Probes::new(policy, WorkBounds::default(), vec![]).unwrap(),
        health.clone(),
        clock,
        async {
            let _ = receive.await;
        },
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !health.degraded.load(Ordering::Acquire) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(700), target.accept())
            .await
            .is_err()
    );
    assert!(!health.ready(now));
    let current = client.monitors().await.unwrap().remove(0);
    assert_eq!(current.epoch, m.epoch);
    assert_eq!(
        current.state.displayed_status(&current.config, now),
        Status::Unknown
    );
    let db = Connection::open(root.join("monitor.db")).unwrap();
    assert_eq!(
        scalar(&db, "SELECT epoch_rows FROM monitoring_counts"),
        10_000
    );
    assert_eq!(scalar(&db, "SELECT raw_rows FROM monitoring_counts"), 0);
    stop.send(()).unwrap();
    worker.await.unwrap().unwrap();
    storage.shutdown().await.unwrap();
}
#[tokio::test]
async fn deleted_highest_identity_and_replay_never_resolve_to_a_replacement_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    client
        .security(minimalmonitor::storage::security::Operation::Local {
            user: "fixture".into(),
            hash: "noncredential-fixture".into(),
            reset: false,
            now: 0,
        })
        .await
        .unwrap();
    let a_config = config(443, false);
    let a = create(&client, a_config.clone(), 0, Some("original-key")).await;
    client
        .monitoring(Operation::Delete {
            id: a.id,
            revision: 1,
            now: 1,
        })
        .await
        .unwrap();
    client
        .monitoring(Operation::Maintain { now: 1 })
        .await
        .unwrap();
    assert!(client.monitors().await.unwrap().is_empty());
    storage.shutdown().await.unwrap();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let mut b_config = config(80, false);
    b_config.name = "replacement fixture".into();
    let b = create(&client, b_config, 2, Some("replacement-key")).await;
    assert_ne!(a.id, b.id);
    assert_eq!(b.revision, 1);
    assert!(matches!(
        client
            .monitoring(Operation::Create {
                config: a_config.clone(),
                now: 3,
                jitter_ms: 0,
                key: Some("original-key".into())
            })
            .await,
        Err(Error::Conflict)
    ));
    assert!(matches!(
        client
            .monitoring(Operation::Edit {
                id: a.id,
                revision: 1,
                config: a_config,
                now: 3,
                jitter_ms: 0
            })
            .await,
        Err(Error::NotFound)
    ));
    assert!(matches!(
        client
            .monitoring(Operation::Delete {
                id: a.id,
                revision: 1,
                now: 3
            })
            .await,
        Err(Error::NotFound)
    ));
    let current = client.monitors().await.unwrap().remove(0);
    assert_eq!((current.id, current.revision), (b.id, 1));
    assert_eq!(current.config.name, "replacement fixture");
    let db = Connection::open(root.join("monitor.db")).unwrap();
    assert_eq!(scalar(&db, "SELECT count(*) FROM request_keys"), 2);
    storage.shutdown().await.unwrap();
}
