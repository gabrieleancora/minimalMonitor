use minimalmonitor::{
    bounds::WorkBounds,
    error::Error,
    monitoring::{Monitor, MonitorConfig, ProbeError, ProbeResult},
    notifications::{Batch, Delivery, SendFuture, Sender, SenderHealth, Transport},
    ownership::StateOwner,
    storage::{
        Storage, StorageClient,
        monitoring::{Operation as MonitorOp, Value as MonitorValue},
        notifications::{Operation, Settings, Value},
    },
};
use rusqlite::{Connection, params};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};

struct Fixture {
    _dir: tempfile::TempDir,
    storage: Storage,
    db: Connection,
}
impl Fixture {
    async fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        let storage = Storage::start(StateOwner::acquire(&root).unwrap())
            .await
            .unwrap();
        let db = Connection::open(root.join("monitor.db")).unwrap();
        let f = Self {
            _dir: dir,
            storage,
            db,
        };
        f.client()
            .notifications(Operation::Startup {
                fingerprint: Some([1; 32]),
                now: 0,
            })
            .await
            .unwrap();
        f.configure(true, Some(i64::MIN), 0).await;
        f
    }
    fn client(&self) -> StorageClient {
        self.storage.client()
    }
    async fn settings(&self) -> Settings {
        match self
            .client()
            .notifications(Operation::Settings)
            .await
            .unwrap()
        {
            Value::Settings(s) => s,
            _ => panic!("settings"),
        }
    }
    async fn configure(&self, enabled: bool, chat_id: Option<i64>, now: i64) {
        let revision = self.settings().await.revision;
        self.client()
            .notifications(Operation::Configure {
                revision,
                enabled,
                chat_id,
                token_present: true,
                now,
            })
            .await
            .unwrap();
    }
    async fn test(&self, now: i64) -> i64 {
        match self
            .client()
            .notifications(Operation::Test {
                token_present: true,
                now,
            })
            .await
            .unwrap()
        {
            Value::Queued(id) => id,
            _ => panic!("test"),
        }
    }
    async fn claim(&self, now: i64) -> Option<Batch> {
        match self
            .client()
            .notifications(Operation::Claim {
                now,
                origin: "https://dashboard.invalid/".into(),
            })
            .await
            .unwrap()
        {
            Value::Batch(b) => b,
            _ => panic!("batch"),
        }
    }
    async fn finish(&self, b: &Batch, outcome: Delivery, now: i64) {
        self.client()
            .notifications(Operation::Finish {
                request_id: b.request_id,
                jobs: b.jobs.clone(),
                outcome,
                now,
            })
            .await
            .unwrap();
    }
    fn state(&self, id: i64) -> (String, u8, i64, Option<i64>) {
        self.db.query_row("SELECT state,attempts,next_attempt_at_ms,message_id FROM notification_outbox WHERE id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).unwrap()
    }
    async fn down(&self, name: &str, now: i64) -> Monitor {
        let config:MonitorConfig=serde_json::from_value(serde_json::json!({"name":name,"kind":"http","target":"https://target.invalid/private?credential=SYNTHETIC","failure_threshold":1})).unwrap();
        let m = match self
            .client()
            .monitoring(MonitorOp::Create {
                config,
                now,
                jitter_ms: 0,
                key: None,
            })
            .await
            .unwrap()
        {
            MonitorValue::Monitor(m) => *m,
            _ => panic!("monitor"),
        };
        self.complete(&m, ProbeResult::fault(ProbeError::Timeout))
            .await;
        self.client()
            .monitors()
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == m.id)
            .unwrap()
    }
    async fn complete(&self, m: &Monitor, result: ProbeResult) {
        self.client()
            .monitoring(MonitorOp::Complete {
                id: m.id,
                revision: m.revision,
                epoch: m.epoch,
                due: m.state.next_due_ms,
                completed: m.state.next_due_ms + 1,
                result,
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn five_second_digest_groups_fifteen_transitions_and_sanitizes_content() {
    let f = Fixture::new().await;
    for n in 0..15 {
        f.down(&format!("service {n}"), 0).await;
    }
    assert!(f.claim(5000).await.is_none());
    let b = f.claim(5001).await.unwrap();
    assert_eq!(b.jobs.len(), 15);
    assert_eq!(b.chat, i64::MIN);
    assert!(b.text.encode_utf16().count() <= 3000);
    assert!(b.text.contains("1970-01-01 00:00:00 UTC"));
    assert!(b.text.contains("incident #") && b.text.contains("timeout"));
    for forbidden in ["target.invalid", "credential", "SYNTHETIC", "10.8."] {
        assert!(!b.text.contains(forbidden));
    }
    f.finish(&b, Delivery::Delivered(123), 5002).await;
    for j in &b.jobs {
        assert_eq!(f.state(j.id).0, "delivered");
        assert_eq!(f.state(j.id).3, Some(123));
    }
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn transient_retry_is_sixty_seconds_from_completion_and_replay_is_idempotent() {
    let f = Fixture::new().await;
    let id = f.test(0).await;
    let b = f.claim(5000).await.unwrap();
    f.finish(&b, Delivery::Transient, 15000).await;
    assert_eq!(f.state(id), ("pending".into(), 1, 75000, None));
    f.finish(&b, Delivery::Transient, 30000).await;
    assert_eq!(f.state(id).2, 75000);
    assert!(f.claim(74999).await.is_none());
    assert_eq!(f.claim(75000).await.unwrap().jobs[0].attempt, 2);
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn durable_bot_wide_cooldown_blocks_new_alerts_tests_and_retries_after_restart() {
    let f = Fixture::new().await;
    let id = f.test(0).await;
    let b = f.claim(5000).await.unwrap();
    f.finish(
        &b,
        Delivery::RateLimited {
            retry_after_seconds: 120,
        },
        10000,
    )
    .await;
    f.down("new failure", 20000).await;
    f.test(60000).await;
    assert!(f.claim(129999).await.is_none());
    assert_eq!(f.state(id).1, 1);
    let root = f._dir.path().join("state");
    f.storage.shutdown().await.unwrap();
    let storage = Storage::start(StateOwner::acquire(&root).unwrap())
        .await
        .unwrap();
    storage
        .client()
        .notifications(Operation::Startup {
            fingerprint: Some([1; 32]),
            now: 70000,
        })
        .await
        .unwrap();
    for now in [70000, 129999] {
        assert!(matches!(
            storage
                .client()
                .notifications(Operation::Claim {
                    now,
                    origin: "https://dashboard.invalid/".into()
                })
                .await
                .unwrap(),
            Value::Batch(None)
        ));
    }
    let b = match storage
        .client()
        .notifications(Operation::Claim {
            now: 130000,
            origin: "https://dashboard.invalid/".into(),
        })
        .await
        .unwrap()
    {
        Value::Batch(Some(b)) => b,
        _ => panic!("ready"),
    };
    assert_eq!(b.jobs[0].attempt, 2);
    storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn short_cooldown_uses_floor_and_unrepresentably_long_wait_never_wraps() {
    for (seconds, expected) in [(1, 70000), (u64::MAX, i64::MAX)] {
        let f = Fixture::new().await;
        f.test(0).await;
        let b = f.claim(5000).await.unwrap();
        f.finish(
            &b,
            Delivery::RateLimited {
                retry_after_seconds: seconds,
            },
            10000,
        )
        .await;
        assert_eq!(f.settings().await.cooldown_until_ms, expected);
        assert!(f.claim(69999).await.is_none());
        f.storage.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn resolved_pending_pair_is_one_dated_digest_and_recovery_discloses_expired_down() {
    let f = Fixture::new().await;
    let m = f.down("pair", 0).await;
    f.complete(&m, ProbeResult::success()).await;
    f.db.execute("UPDATE incidents SET observation_gap=1", [])
        .unwrap();
    let b = f.claim(70000).await.unwrap();
    assert_eq!(b.jobs.len(), 2);
    assert!(
        b.text.contains("RESOLVED")
            && b.text.contains("DOWN")
            && b.text.contains("RECOVERED")
            && b.text.contains("coverage gaps")
    );
    assert_eq!(b.text.matches("incident #").count(), 1);
    f.finish(&b, Delivery::Delivered(42), 70001).await;
    let m = f.down("expired prior", 100000).await;
    f.complete(&m, ProbeResult::success()).await;
    f.db.execute("UPDATE notification_outbox SET state='expired',terminal_at_ms=170000 WHERE kind='down' AND state='pending'",[]).unwrap();
    let b = f.claim(170000).await.unwrap();
    assert_eq!(b.jobs.len(), 1);
    assert!(b.text.contains("Earlier outage alert was not delivered"));
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn recovery_cannot_overtake_retrying_down() {
    let f = Fixture::new().await;
    let m = f.down("ordered", 0).await;
    let down = f.claim(5001).await.unwrap();
    f.finish(&down, Delivery::Transient, 6000).await;
    f.complete(&m, ProbeResult::success()).await;
    assert!(f.claim(65000).await.is_none());
    let resolved = f.claim(66000).await.unwrap();
    assert_eq!(resolved.jobs.len(), 2);
    assert_eq!(resolved.jobs[0].id, down.jobs[0].id);
    assert!(resolved.text.contains("RESOLVED"));
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn permanent_error_stops_all_requests_until_explicit_settings_correction() {
    let f = Fixture::new().await;
    let id = f.test(0).await;
    let b = f.claim(5000).await.unwrap();
    f.finish(&b, Delivery::Configuration, 6000).await;
    assert_eq!(f.state(id).0, "failed");
    f.down("still checking", 10000).await;
    assert!(f.settings().await.configuration_failed);
    assert!(f.claim(100000).await.is_none());
    assert!(matches!(
        f.client()
            .notifications(Operation::Test {
                token_present: true,
                now: 100000
            })
            .await,
        Err(Error::Conflict)
    ));
    f.configure(true, Some(i64::MIN), 100000).await;
    assert!(f.claim(100000).await.is_some());
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn settings_cancel_old_destinations_never_replay_disabled_history_and_preserve_provider_wait()
{
    let f = Fixture::new().await;
    let id = f.test(0).await;
    let b = f.claim(5000).await.unwrap();
    f.configure(true, Some(-1009876543210), 6000).await;
    assert_eq!(f.state(id).0, "canceled");
    f.finish(
        &b,
        Delivery::RateLimited {
            retry_after_seconds: 120,
        },
        7000,
    )
    .await;
    assert_eq!(f.settings().await.cooldown_until_ms, 127000);
    f.finish(
        &b,
        Delivery::RateLimited {
            retry_after_seconds: 120,
        },
        10000,
    )
    .await;
    assert_eq!(f.settings().await.cooldown_until_ms, 127000);
    let new = f.test(60000).await;
    assert!(f.claim(126999).await.is_none());
    let b = f.claim(127000).await.unwrap();
    assert_eq!(b.chat, -1009876543210);
    f.finish(&b, Delivery::Delivered(50), 127001).await;
    f.configure(false, Some(-1009876543210), 130000).await;
    f.down("disabled", 130000).await;
    f.configure(true, Some(-1009876543210), 140000).await;
    assert!(f.claim(200000).await.is_none());
    assert_eq!(f.state(new).0, "delivered");
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn ten_attempts_and_six_hour_expiry_count_only_admitted_requests() {
    let f = Fixture::new().await;
    let id = f.test(0).await;
    let mut now = 5000;
    for attempt in 1..=10 {
        let b = f.claim(now).await.unwrap();
        assert_eq!(b.jobs[0].attempt, attempt);
        f.finish(&b, Delivery::Transient, now + 1).await;
        assert!(f.claim(now + 60000).await.is_none());
        now += 60001;
    }
    assert_eq!(f.state(id).0, "failed");
    let expired = f.test(1_000_000).await;
    assert!(f.claim(22_600_000).await.is_none());
    assert_eq!(f.state(expired).0, "expired");
    assert_eq!(f.state(expired).1, 0);
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn interrupted_remote_success_remains_durable_and_can_duplicate_with_stable_identity() {
    let f = Fixture::new().await;
    let m = f.down("crash window", 0).await;
    let b = f.claim(5001).await.unwrap();
    assert_eq!(f.state(b.jobs[0].id).0, "sending");
    // Simulate remote acceptance followed by lost local acknowledgement.
    f.client()
        .notifications(Operation::Startup {
            fingerprint: Some([1; 32]),
            now: 10000,
        })
        .await
        .unwrap();
    assert!(f.claim(69999).await.is_none());
    let retry = f.claim(70000).await.unwrap();
    assert_eq!(b.jobs[0].id, retry.jobs[0].id);
    assert_eq!(retry.jobs[0].attempt, 2);
    assert_eq!(retry.text, b.text);
    f.finish(&b, Delivery::Delivered(999), 70000).await;
    assert_eq!(f.state(retry.jobs[0].id).0, "sending");
    f.finish(&retry, Delivery::Delivered(1000), 70001).await;
    assert_eq!(f.client().monitors().await.unwrap()[0].id, m.id);
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn test_action_and_configuration_validation_are_durable_and_revision_safe() {
    let f = Fixture::new().await;
    f.test(0).await;
    for now in [0, 59999] {
        assert!(matches!(
            f.client()
                .notifications(Operation::Test {
                    token_present: true,
                    now
                })
                .await,
            Err(Error::RateLimited)
        ));
    }
    f.test(60000).await;
    let s = f.settings().await;
    for (rev, token, chat, expected) in [
        (s.revision - 1, true, Some(-100), Error::Conflict),
        (s.revision, false, Some(-100), Error::InvalidInput),
        (s.revision, true, Some(0), Error::InvalidInput),
        (s.revision, true, None, Error::InvalidInput),
    ] {
        assert!(
            matches!(f.client().notifications(Operation::Configure {revision:rev,enabled:true,chat_id:chat,token_present:token,now:61000}).await,Err(e) if e==expected)
        );
    }
    assert_eq!(f.settings().await.revision, s.revision);
    f.storage.shutdown().await.unwrap();
}

#[tokio::test]
async fn row_and_content_overflow_preserve_incidents_and_enqueue_a_dated_suppression_summary() {
    for content_limit in [false, true] {
        let f = Fixture::new().await;
        let revision = f.settings().await.destination_revision;
        let count = if content_limit { 128 } else { 1000 };
        let payload = if content_limit {
            " ".repeat(16384)
        } else {
            "{}".to_owned()
        };
        let tx = f.db.unchecked_transaction().unwrap();
        for n in 0..count {
            tx.execute("INSERT INTO notification_outbox(event_identity,kind,destination_revision,payload,state,next_attempt_at_ms,created_at_ms,expires_at_ms) VALUES(?1,'test',?2,?3,'pending',0,0,21600000)",params![format!("capacity-{n}"),revision,payload]).unwrap();
        }
        tx.commit().unwrap();
        f.down("suppressed incident", 100).await;
        assert_eq!(f.settings().await.dropped_alerts, 1);
        assert_eq!(
            f.db.query_row("SELECT count(*) FROM incidents", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            f.db.query_row("SELECT count(*) FROM notification_outbox", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            count
        );
        f.db.execute("UPDATE notification_outbox SET state='delivered',terminal_at_ms=200 WHERE id IN (SELECT id FROM notification_outbox LIMIT 1)",[]).unwrap();
        f.client()
            .notifications(Operation::Maintain { now: 1000 })
            .await
            .unwrap();
        f.client()
            .notifications(Operation::Maintain { now: 1001 })
            .await
            .unwrap();
        assert_eq!(
            f.db.query_row(
                "SELECT count(*) FROM notification_outbox WHERE kind='suppression'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        // Existing arbitrary capacity fixtures cannot block digest rendering.
        f.db.execute(
            "UPDATE notification_outbox SET next_attempt_at_ms=100000 WHERE kind='test'",
            [],
        )
        .unwrap();
        let b = f.claim(6000).await.unwrap();
        assert!(b.text.contains("1 events were dropped") && b.text.contains("UTC"));
        f.storage.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn terminal_metadata_is_bounded_by_count_and_age_of_completion() {
    let f = Fixture::new().await;
    let tx = f.db.unchecked_transaction().unwrap();
    for n in 0..2200 {
        tx.execute("INSERT INTO notification_outbox(event_identity,kind,destination_revision,payload,state,next_attempt_at_ms,created_at_ms,expires_at_ms) VALUES(?1,'test',1,'{}','pending',0,0,21600000)",[format!("terminal-{n}")]).unwrap();
        tx.execute("UPDATE notification_outbox SET state='delivered',terminal_at_ms=1000000 WHERE event_identity=?1",[format!("terminal-{n}")]).unwrap();
    }
    tx.commit().unwrap();
    let count = || {
        f.db.query_row("SELECT count(*) FROM notification_outbox", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap()
    };
    assert_eq!(count(), 2000);
    f.client()
        .notifications(Operation::Maintain {
            now: 7 * 86400000 + 999999,
        })
        .await
        .unwrap();
    assert_eq!(count(), 2000);
    for _ in 0..4 {
        f.client()
            .notifications(Operation::Maintain {
                now: 7 * 86400000 + 1000001,
            })
            .await
            .unwrap();
    }
    assert_eq!(count(), 0);
    f.storage.shutdown().await.unwrap();
}

struct Mock {
    sends: Arc<AtomicUsize>,
    outcomes: Mutex<Vec<Delivery>>,
    clock: Arc<AtomicI64>,
    client: StorageClient,
}
#[tokio::test]
async fn recovery_remembers_delivered_down_after_seven_day_receipt_cleanup() {
    let f = Fixture::new().await;
    let m = f.down("long incident", 0).await;
    let b = f.claim(5001).await.unwrap();
    f.finish(&b, Delivery::Delivered(123), 5002).await;
    f.client()
        .notifications(Operation::Maintain { now: 8 * 86400000 })
        .await
        .unwrap();
    assert_eq!(
        f.db.query_row("SELECT count(*) FROM notification_outbox", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    for _ in 0..24 {
        if matches!(
            f.client()
                .monitoring(MonitorOp::CatchUp { now: 8 * 86400000 })
                .await
                .unwrap(),
            MonitorValue::Committed(true)
        ) {
            break;
        }
    }
    // Catch-up is deliberately bounded; use the persisted next slot to recover.
    let current = f
        .client()
        .monitors()
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    f.complete(&current, ProbeResult::success()).await;
    let created: i64 =
        f.db.query_row(
            "SELECT created_at_ms FROM notification_outbox WHERE kind='recovery'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let b = f.claim(created + 5000).await.unwrap();
    assert!(created >= 8 * 86400000);
    assert!(b.text.contains("RECOVERED"));
    assert!(!b.text.contains("Earlier outage alert was not delivered"));
    f.storage.shutdown().await.unwrap();
}

struct Pressure {
    path: std::path::PathBuf,
    reader: Arc<Mutex<Option<Connection>>>,
    sends: Arc<AtomicUsize>,
}
#[tokio::test]
async fn credential_removal_and_replacement_cancel_queued_destinations_without_clearing_cooldown() {
    let f = Fixture::new().await;
    f.test(0).await;
    let b = f.claim(5000).await.unwrap();
    f.finish(
        &b,
        Delivery::RateLimited {
            retry_after_seconds: 120,
        },
        6000,
    )
    .await;
    f.client()
        .notifications(Operation::Startup {
            fingerprint: None,
            now: 10000,
        })
        .await
        .unwrap();
    assert_eq!(f.state(b.jobs[0].id).0, "canceled");
    let queued = f.test(60000).await;
    f.client()
        .notifications(Operation::Startup {
            fingerprint: Some([2; 32]),
            now: 70000,
        })
        .await
        .unwrap();
    assert_eq!(f.state(queued).0, "canceled");
    assert_eq!(f.settings().await.cooldown_until_ms, 126000);
    assert!(f.claim(150000).await.is_none());
    f.storage.shutdown().await.unwrap();
}
impl Transport for Pressure {
    fn send<'a>(
        &'a self,
        _chat: i64,
        _text: &'a str,
        _permit: minimalmonitor::probes::ProbePermit,
    ) -> SendFuture<'a> {
        Box::pin(async move {
            self.sends.fetch_add(1, Ordering::SeqCst);
            let path = self.path.clone();
            let reader=tokio::task::spawn_blocking(move || {
                let writer=Connection::open(&path).unwrap();
                writer.execute_batch("PRAGMA wal_autocheckpoint=0; CREATE TABLE notification_wal_fixture(payload BLOB); INSERT INTO notification_wal_fixture VALUES(zeroblob(524288));").unwrap();
                let reader=Connection::open(path).unwrap();reader.execute_batch("BEGIN").unwrap();
                reader.query_row("SELECT length(payload) FROM notification_wal_fixture",[],|r|r.get::<_,i64>(0)).unwrap();
                for _ in 0..72 {writer.execute("UPDATE notification_wal_fixture SET payload=randomblob(524288)",[]).unwrap();}
                reader
            }).await.unwrap();
            *self.reader.lock().unwrap() = Some(reader);
            Delivery::Delivered(77)
        })
    }
}
#[tokio::test]
async fn failed_ack_persistence_keeps_one_receipt_and_recovers_without_resending() {
    let f = Fixture::new().await;
    let id = f.test(0).await;
    let reader = Arc::new(Mutex::new(None));
    let sends = Arc::new(AtomicUsize::new(0));
    let transport = Pressure {
        path: f._dir.path().join("state/monitor.db"),
        reader: reader.clone(),
        sends: sends.clone(),
    };
    let mut sender = Sender::new(
        f.client(),
        transport,
        WorkBounds::default(),
        SenderHealth {
            token_present: true,
            ..Default::default()
        },
        &url::Url::parse("https://dashboard.invalid/").unwrap(),
    )
    .with_clock(Arc::new(|| Ok(6000)));
    assert!(matches!(sender.tick(5000).await, Err(Error::DiskPressure)));
    assert!(matches!(sender.tick(7000).await, Err(Error::DiskPressure)));
    assert_eq!(f.state(id).0, "sending");
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    reader
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .execute_batch("ROLLBACK")
        .unwrap();
    assert!(f.client().health().await.unwrap().writable);
    sender.tick(7000).await.unwrap();
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert_eq!(f.state(id).0, "delivered");
    assert_eq!(f.state(id).3, Some(77));
    f.storage.shutdown().await.unwrap();
}
impl Transport for Mock {
    fn send<'a>(
        &'a self,
        _chat: i64,
        _text: &'a str,
        _permit: minimalmonitor::probes::ProbePermit,
    ) -> SendFuture<'a> {
        Box::pin(async move {
            self.sends.fetch_add(1, Ordering::SeqCst);
            // A probe result can commit while the sender request is outstanding.
            assert!(self.client.monitors().await.is_ok());
            self.clock.fetch_add(10000, Ordering::SeqCst);
            self.outcomes.lock().unwrap().remove(0)
        })
    }
}
#[tokio::test]
async fn sender_clock_and_transport_are_injected_and_notification_failure_does_not_change_target_state()
 {
    let f = Fixture::new().await;
    let m = f.down("independent", 0).await;
    let clock = Arc::new(AtomicI64::new(5001));
    let sends = Arc::new(AtomicUsize::new(0));
    let mock = Mock {
        sends: sends.clone(),
        outcomes: Mutex::new(vec![Delivery::Transient, Delivery::Delivered(1)]),
        clock: clock.clone(),
        client: f.client(),
    };
    let read = clock.clone();
    let mut sender = Sender::new(
        f.client(),
        mock,
        WorkBounds::default(),
        SenderHealth {
            token_present: true,
            ..Default::default()
        },
        &url::Url::parse("https://dashboard.invalid/").unwrap(),
    )
    .with_clock(Arc::new(move || Ok(read.load(Ordering::SeqCst))));
    sender.tick(5001).await.unwrap();
    assert_eq!(
        f.db.query_row(
            "SELECT next_attempt_at_ms FROM notification_outbox",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        75001
    );
    sender.tick(75000).await.unwrap();
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    clock.store(75001, Ordering::SeqCst);
    sender.tick(75001).await.unwrap();
    assert_eq!(sends.load(Ordering::SeqCst), 2);
    assert_eq!(
        f.client().monitors().await.unwrap()[0].state.status,
        m.state.status
    );
    f.storage.shutdown().await.unwrap();
}
