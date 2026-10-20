use minimalmonitor::{
    bounds::WorkBounds,
    destination::{Policy, Rule},
    error::Result,
    monitoring::*,
    ownership::StateOwner,
    probes::Probes,
    scheduler::{self, Clock, SchedulerHealth},
    storage::{Storage, StorageClient, monitoring::Operation},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
struct TestClock(AtomicI64);
impl Clock for TestClock {
    fn read(&self) -> Result<(i64, Duration)> {
        let now = self.0.load(Ordering::Acquire);
        Ok((now, Duration::from_millis(now as u64)))
    }
}
async fn wait_for(client: &StorageClient, streak: u8, status: Status) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let monitors = client.monitors().await.unwrap();
            if monitors.len() == 15
                && monitors
                    .iter()
                    .all(|m| m.state.failure_streak == streak && m.state.status == status)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn fifteen_synchronized_failures_confirmation_recovery_and_bounded_concurrency() {
    let dir = tempfile::tempdir().unwrap();
    let storage = Storage::start(StateOwner::acquire(&dir.path().join("state")).unwrap())
        .await
        .unwrap();
    let client = storage.client();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let inflight = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let code = Arc::new(AtomicUsize::new(503));
    let (load, high, status) = (inflight.clone(), peak.clone(), code.clone());
    let server = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        loop {
            tokio::select! {Some(_)=tasks.join_next(),if !tasks.is_empty()=>(),accepted=listener.accept()=> {let (mut stream,_)=accepted.unwrap();let (load,high,status)=(load.clone(),high.clone(),status.clone());tasks.spawn(async move {
                let live=load.fetch_add(1,Ordering::SeqCst)+1;high.fetch_max(live,Ordering::SeqCst);
                let mut request=[0;4096];let _=stream.read(&mut request).await;
                tokio::time::sleep(Duration::from_millis(25)).await;
                let response=format!("HTTP/1.1 {} Fixture\r\nContent-Length: 0\r\n\r\n",status.load(Ordering::Acquire));let _=stream.write_all(response.as_bytes()).await;load.fetch_sub(1,Ordering::SeqCst);
            });}}
        }
    });
    let policy = Policy::new(vec![Rule {
        cidr: "127.0.0.1/32".into(),
        host: None,
        ports: vec![port],
        all_tcp_ports: false,
        ping: false,
    }])
    .unwrap();
    let probes = Probes::new(policy, WorkBounds::default(), vec![]).unwrap();
    let health = SchedulerHealth::default();
    let clock = Arc::new(TestClock(AtomicI64::new(100_000)));
    let (stop, receive) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(scheduler::run_with_clock(
        client.clone(),
        probes,
        health.clone(),
        clock.clone(),
        async {
            let _ = receive.await;
        },
    ));
    tokio::time::timeout(Duration::from_secs(3), async {
        while !health.ready(100_000) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    for n in 0..15 {
        let config = MonitorConfig {
            name: format!("fixture-{n}"),
            kind: Kind::Http,
            target: format!("http://127.0.0.1:{port}/"),
            port: None,
            interval_seconds: 60,
            deadline_seconds: 5,
            failure_threshold: 3,
            enabled: true,
            alerts: false,
            address_family: Family::Auto,
            accepted_statuses: vec![[200, 299]],
        };
        client
            .monitoring(Operation::Create {
                config,
                now: 100_000,
                jitter_ms: 0,
                key: None,
            })
            .await
            .unwrap();
    }
    wait_for(&client, 1, Status::PendingFailure).await;
    assert!(
        client
            .monitors()
            .await
            .unwrap()
            .iter()
            .all(|m| m.state.next_due_ms == 130_000)
    );
    clock.0.store(130_000, Ordering::Release);
    wait_for(&client, 2, Status::PendingFailure).await;
    clock.0.store(160_000, Ordering::Release);
    wait_for(&client, 3, Status::Down).await;
    let db = rusqlite::Connection::open(dir.path().join("state/monitor.db")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM incidents", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        15
    );
    assert_eq!(
        db.query_row("SELECT sum(expected) FROM hourly_stats", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        45
    );
    code.store(200, Ordering::Release);
    clock.0.store(220_000, Ordering::Release);
    wait_for(&client, 0, Status::Up).await;
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM incidents WHERE end_reason='recovered'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        15
    );
    assert!(peak.load(Ordering::SeqCst) <= 4);
    assert!(health.ready(220_000));
    stop.send(()).unwrap();
    worker.await.unwrap().unwrap();
    server.abort();
    let _ = server.await;
    storage.shutdown().await.unwrap();
}
