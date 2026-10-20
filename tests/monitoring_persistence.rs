use minimalmonitor::{
    error::Error,
    monitoring::*,
    ownership::StateOwner,
    storage::{
        Storage, StorageClient,
        monitoring::{Operation, Value},
    },
};
use rusqlite::{Connection, params};

fn config() -> MonitorConfig {
    MonitorConfig {
        name: "fixture".into(),
        kind: Kind::Tcp,
        target: "localhost".into(),
        port: Some(443),
        interval_seconds: 60,
        deadline_seconds: 5,
        failure_threshold: 3,
        enabled: true,
        alerts: true,
        address_family: Family::Auto,
        accepted_statuses: vec![[200, 299]],
    }
}
async fn create(client: &StorageClient, now: i64) -> Monitor {
    match client
        .monitoring(Operation::Create {
            config: config(),
            now,
            jitter_ms: 0,
            key: None,
        })
        .await
        .unwrap()
    {
        Value::Monitor(m) => *m,
        _ => panic!("monitor"),
    }
}
async fn complete(client: &StorageClient, m: &Monitor, due: i64, result: ProbeResult) -> bool {
    match client
        .monitoring(Operation::Complete {
            id: m.id,
            revision: m.revision,
            epoch: m.epoch,
            due,
            completed: due + 5000,
            result,
        })
        .await
        .unwrap()
    {
        Value::Committed(v) => v,
        _ => panic!("commit"),
    }
}
fn counts(db: &Connection, id: i64) -> Counts {
    db.query_row("SELECT coalesce(sum(expected),0),coalesce(sum(successful),0),coalesce(sum(failed),0),coalesce(sum(unknown),0) FROM hourly_stats WHERE monitor_id=?1",[id],|r|Ok(Counts {expected:r.get::<_,i64>(0)? as u64,successful:r.get::<_,i64>(1)? as u64,failed:r.get::<_,i64>(2)? as u64,unknown:r.get::<_,i64>(3)? as u64})).unwrap()
}
#[tokio::test]
async fn transactional_confirmation_incidents_recovery_and_exact_coverage() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let db = Connection::open(root.join("monitor.db")).unwrap();
    db.execute("UPDATE settings SET telegram_enabled=1", [])
        .unwrap();
    let m = create(&client, 0).await;
    let failure = ProbeResult::fault(ProbeError::Timeout);
    assert!(complete(&client, &m, 0, ProbeResult::success()).await);
    for due in [60_000, 95_000, 130_000] {
        assert!(complete(&client, &m, due, failure).await);
    }
    assert!(!complete(&client, &m, 130_000, failure).await);
    let current = client.monitors().await.unwrap().remove(0);
    assert_eq!(
        (current.state.status, current.state.next_due_ms),
        (Status::Down, 190_000)
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM incidents", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT confirmed_at_ms FROM incidents", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        135_000
    );
    assert!(complete(&client, &m, 190_000, ProbeResult::success()).await);
    assert_eq!(counts(&db, m.id).availability(), Some(40.0));
    assert_eq!(counts(&db, m.id).coverage(), Some(100.0));
    assert_eq!(
        db.query_row("SELECT count(*) FROM notification_outbox", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        2
    );
    let reason: String = db
        .query_row("SELECT end_reason FROM incidents", [], |r| r.get(0))
        .unwrap();
    assert_eq!(reason, "recovered");
    // A missed slot enters only coverage; it resets a pending streak.
    client
        .monitoring(Operation::CatchUp { now: 320_000 })
        .await
        .unwrap();
    assert_eq!(counts(&db, m.id).unknown, 2);
    storage.shutdown().await.unwrap();
}
#[tokio::test]
async fn pending_restart_gap_no_confirmation_burst_and_no_duplicate_down() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let m = create(&client, 0).await;
    let failure = ProbeResult::fault(ProbeError::Timeout);
    complete(&client, &m, 0, failure).await;
    storage.shutdown().await.unwrap();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    client
        .monitoring(Operation::Reanchor {
            now: 200_000,
            reason: "restart",
        })
        .await
        .unwrap();
    let current = client.monitors().await.unwrap().remove(0);
    let db = Connection::open(root.join("monitor.db")).unwrap();
    // t=35,95,155, rather than invented t=35,70 confirmation chains.
    assert_eq!(
        counts(&db, m.id),
        Counts {
            expected: 4,
            successful: 0,
            failed: 1,
            unknown: 3
        }
    );
    assert_eq!(
        (current.state.status, current.state.failure_streak),
        (Status::Unknown, 0)
    );
    assert_ne!(current.epoch, m.epoch);
    assert!(!complete(&client, &m, 35_000, failure).await);
    let due = current.state.next_due_ms;
    for slot in [due, due + 35_000, due + 70_000] {
        complete(&client, &current, slot, failure).await;
    }
    client
        .monitoring(Operation::Reanchor {
            now: due + 90_000,
            reason: "restart",
        })
        .await
        .unwrap();
    let after = client.monitors().await.unwrap().remove(0);
    assert!(after.state.active_incident.is_some());
    assert_eq!(
        db.query_row("SELECT observation_gap FROM incidents", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    complete(&client, &after, after.state.next_due_ms, failure).await;
    assert_eq!(
        db.query_row("SELECT count(*) FROM incidents", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let after = client.monitors().await.unwrap().remove(0);
    complete(
        &client,
        &after,
        after.state.next_due_ms,
        ProbeResult::success(),
    )
    .await;
    assert_eq!(
        db.query_row("SELECT end_reason FROM incidents", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "recovered"
    );
    storage.shutdown().await.unwrap();
}
#[tokio::test]
async fn edits_pause_policy_faults_and_clock_epochs_discard_late_work() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let old = create(&client, 0).await;
    let failure = ProbeResult::fault(ProbeError::Timeout);
    for due in [0, 35_000, 70_000] {
        complete(&client, &old, due, failure).await;
    }
    let mut revision = 1;
    for n in 0..20 {
        let mut cfg = config();
        cfg.name = format!("fixture-{n}");
        client
            .monitoring(Operation::Edit {
                id: old.id,
                revision,
                config: cfg,
                now: 80_000 + n,
                jitter_ms: 0,
            })
            .await
            .unwrap();
        revision += 1;
    }
    assert!(!complete(&client, &old, 130_000, ProbeResult::success()).await);
    let db = Connection::open(root.join("monitor.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT end_reason FROM incidents", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "configuration_changed"
    );
    let latest = client.monitors().await.unwrap().remove(0);
    complete(
        &client,
        &latest,
        latest.state.next_due_ms,
        ProbeResult::fault(ProbeError::Permission),
    )
    .await;
    assert_eq!(
        client.monitors().await.unwrap()[0].state.status,
        Status::Unknown
    );
    assert!(matches!(
        client
            .monitoring(Operation::Pause {
                id: old.id,
                revision: 1,
                now: 90_000
            })
            .await,
        Err(Error::Conflict)
    ));
    client
        .monitoring(Operation::Pause {
            id: old.id,
            revision,
            now: 90_000,
        })
        .await
        .unwrap();
    let before = counts(&db, old.id);
    client
        .monitoring(Operation::CatchUp { now: 1_000_000 })
        .await
        .unwrap();
    assert_eq!(counts(&db, old.id), before);
    client
        .monitoring(Operation::Resume {
            id: old.id,
            revision: revision + 1,
            now: 1_000_000,
            jitter_ms: 0,
        })
        .await
        .unwrap();
    let resumed = client.monitors().await.unwrap().remove(0);
    client
        .monitoring(Operation::Reanchor {
            now: 1_000,
            reason: "clock_jump",
        })
        .await
        .unwrap();
    assert!(
        !complete(
            &client,
            &resumed,
            resumed.state.next_due_ms,
            ProbeResult::success()
        )
        .await
    );
    storage.shutdown().await.unwrap();
}
#[tokio::test]
async fn admission_idempotency_and_transaction_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let db = Connection::open(root.join("monitor.db")).unwrap();
    db.execute(
        "INSERT INTO admin VALUES(1,'fixture','noncredential-fixture',1,0,0)",
        [],
    )
    .unwrap();
    for _ in 0..18 {
        create(&client, 0).await;
    }
    assert!(matches!(
        client
            .monitoring(Operation::Create {
                config: config(),
                now: 0,
                jitter_ms: 0,
                key: None
            })
            .await,
        Err(Error::Conflict)
    ));
    let mut paused = config();
    paused.enabled = false;
    let operation = || Operation::Create {
        config: paused.clone(),
        now: 0,
        jitter_ms: 0,
        key: Some("fixture-key".into()),
    };
    client.monitoring(operation()).await.unwrap();
    client.monitoring(operation()).await.unwrap();
    assert_eq!(client.monitors().await.unwrap().len(), 19);
    assert!(matches!(
        client
            .monitoring(Operation::Create {
                config: config(),
                now: 0,
                jitter_ms: 0,
                key: Some("fixture-key".into())
            })
            .await,
        Err(Error::Conflict)
    ));
    let m = client
        .monitors()
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.config.enabled)
        .unwrap();
    db.execute_batch("CREATE TRIGGER reject_incident BEFORE INSERT ON incidents BEGIN SELECT RAISE(ABORT,'controlled'); END").unwrap();
    let failure = ProbeResult::fault(ProbeError::Timeout);
    complete(&client, &m, 0, failure).await;
    complete(&client, &m, 35_000, failure).await;
    assert!(matches!(
        client
            .monitoring(Operation::Complete {
                id: m.id,
                revision: m.revision,
                epoch: m.epoch,
                due: 70_000,
                completed: 75_000,
                result: failure
            })
            .await,
        Err(Error::Storage)
    ));
    assert_eq!(
        client
            .monitors()
            .await
            .unwrap()
            .iter()
            .find(|current| current.id == m.id)
            .unwrap()
            .state
            .failure_streak,
        2
    );
    assert_eq!(counts(&db, m.id).expected, 2);
    db.execute_batch("DROP TRIGGER reject_incident").unwrap();
    assert!(complete(&client, &m, 70_000, failure).await);
    storage.shutdown().await.unwrap();
}
#[tokio::test]
async fn retention_preserves_10001_recent_incidents_and_old_open_incident() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let m = create(&client, 0).await;
    let mut db = Connection::open(root.join("monitor.db")).unwrap();
    let now = 200 * DAY_MS;
    {
        let tx = db.transaction().unwrap();
        for n in 0..10001 {
            tx.execute("INSERT INTO incidents(monitor_id,revision,first_failure_at_ms,confirmed_at_ms,ended_at_ms,end_reason,last_evidence_at_ms) VALUES(?1,1,?2,?2,?2,'recovered',?2)",params![m.id,now-10*DAY_MS+n]).unwrap();
        }
        tx.commit().unwrap();
    }
    db.execute("INSERT INTO incidents(monitor_id,revision,first_failure_at_ms,confirmed_at_ms,last_evidence_at_ms) VALUES(?1,1,0,0,0)",[m.id]).unwrap();
    db.execute("INSERT INTO incidents(monitor_id,revision,first_failure_at_ms,confirmed_at_ms,ended_at_ms,end_reason,last_evidence_at_ms) VALUES(?1,1,0,0,0,'recovered',0)",[m.id]).unwrap();
    client
        .monitoring(Operation::Maintain { now })
        .await
        .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM incidents", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        10002
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM incidents WHERE ended_at_ms IS NULL",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    match client
        .monitoring(Operation::Incidents {
            before: i64::MAX,
            limit: 100,
        })
        .await
        .unwrap()
    {
        Value::Incidents(v) => assert_eq!(v.len(), 100),
        _ => panic!("incidents"),
    }
    storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn partial_recent_windows_exact_unknown_coverage_and_bounded_age_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let m = create(&client, 0).await;
    complete(&client, &m, 0, ProbeResult::success()).await;
    complete(&client, &m, 60_000, ProbeResult::success()).await;
    complete(&client, &m, 120_000, ProbeResult::success()).await;
    complete(
        &client,
        &m,
        180_000,
        ProbeResult::fault(ProbeError::Timeout),
    )
    .await;
    client
        .monitoring(Operation::CatchUp { now: 225_000 })
        .await
        .unwrap();
    let db = Connection::open(root.join("monitor.db")).unwrap();
    let c = counts(&db, m.id);
    assert_eq!((c.availability(), c.coverage()), (Some(75.0), Some(80.0)));
    match client
        .monitoring(Operation::History {
            id: m.id,
            start: 60_000,
            end: 180_000,
            now: 225_000,
        })
        .await
        .unwrap()
    {
        Value::Hours(v, _) => assert_eq!(
            v[0].counts,
            Counts {
                expected: 2,
                successful: 2,
                failed: 0,
                unknown: 0
            }
        ),
        _ => panic!("history"),
    }
    assert!(complete(&client, &m, 275_000, ProbeResult::success()).await);
    // Populate eligible raw rows; one maintenance command removes <=500.
    for due in -600..0 {
        db.execute("INSERT INTO probe_results(monitor_id,revision,scheduled_slot_ms,observed_at_ms,outcome) VALUES(?1,1,?2,0,'unknown')",params![m.id,due]).unwrap();
    }
    let now = 10 * DAY_MS;
    let before: i64 = db
        .query_row("SELECT count(*) FROM probe_results", [], |r| r.get(0))
        .unwrap();
    client
        .monitoring(Operation::Maintain { now })
        .await
        .unwrap();
    let after: i64 = db
        .query_row("SELECT count(*) FROM probe_results", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before - after, 500);
    assert_eq!(counts(&db, m.id).expected, 6); // Aggregate survives raw retention.
    storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn full_outbox_suppresses_one_event_without_losing_the_incident() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let m = create(&client, 0).await;
    let mut db = Connection::open(root.join("monitor.db")).unwrap();
    {
        let tx = db.transaction().unwrap();
        tx.execute("UPDATE settings SET telegram_enabled=1", [])
            .unwrap();
        for n in 0..1000 {
            tx.execute("INSERT INTO notification_outbox(event_identity,kind,destination_revision,payload,state,next_attempt_at_ms,created_at_ms,expires_at_ms) VALUES(?1,'test',1,'{}','pending',0,0,21600000)",[format!("fixture-{n}")]).unwrap();
        }
        tx.commit().unwrap();
    }
    for due in [0, 35_000, 70_000] {
        complete(&client, &m, due, ProbeResult::fault(ProbeError::Timeout)).await;
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM notification_outbox", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1000
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM incidents", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT dropped_alerts FROM health_state", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        client.monitors().await.unwrap()[0].state.status,
        Status::Down
    );
    storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn counter_capacity_faults_roll_back_samples_and_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let m = create(&client, 0).await;
    let db = Connection::open(root.join("monitor.db")).unwrap();
    // Controlled counter-pressure fixture, not a measured retained-history load.
    db.execute(
        "UPDATE monitoring_counts SET raw_rows=1010000,hourly_rows=108050,epoch_rows=10000",
        [],
    )
    .unwrap();
    assert!(matches!(
        client
            .monitoring(Operation::Create {
                config: config(),
                now: 0,
                jitter_ms: 0,
                key: None
            })
            .await,
        Err(Error::Storage)
    ));
    assert_eq!(client.monitors().await.unwrap().len(), 1);
    db.execute("UPDATE monitoring_counts SET epoch_rows=1", [])
        .unwrap();
    let operation = || Operation::Complete {
        id: m.id,
        revision: m.revision,
        epoch: m.epoch,
        due: 0,
        completed: 5000,
        result: ProbeResult::success(),
    };
    assert!(matches!(
        client.monitoring(operation()).await,
        Err(Error::Storage)
    ));
    db.execute("UPDATE monitoring_counts SET raw_rows=0", [])
        .unwrap();
    assert!(matches!(
        client.monitoring(operation()).await,
        Err(Error::Storage)
    ));
    assert_eq!(
        db.query_row("SELECT count(*) FROM probe_results", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(client.monitors().await.unwrap()[0].state.next_due_ms, 0);
    db.execute("UPDATE monitoring_counts SET hourly_rows=0", [])
        .unwrap();
    assert!(matches!(
        client.monitoring(operation()).await.unwrap(),
        Value::Committed(true)
    ));
    storage.shutdown().await.unwrap();
}
