// Development-only browser review fixture: real HTTP/auth/storage, no probes or sender.
use minimalmonitor::{
    auth::{Auth, now_ms},
    bounds::WorkBounds,
    config::{Config, Mode},
    destination::Policy,
    monitoring::{Family, Kind, MonitorConfig, ProbeError, ProbeResult},
    notifications::SenderHealth,
    ownership::StateOwner,
    scheduler::SchedulerHealth,
    storage::{
        Storage,
        monitoring::{Operation, Value},
    },
    web,
};
use std::sync::atomic::Ordering;
#[tokio::main]
async fn main() {
    let password =
        std::env::var("MM_GUI_REVIEW_PASSWORD").expect("runner supplies ephemeral password");
    let root = tempfile::Builder::new()
        .prefix("wp05-browser-")
        .tempdir_in("target")
        .unwrap();
    let state_dir = root.path().join("state");
    let storage = Storage::start(StateOwner::acquire(&state_dir).unwrap())
        .await
        .unwrap();
    let auth = Auth::new(storage.client(), WorkBounds::default())
        .await
        .unwrap();
    auth.local_account("review".into(), zeroize::Zeroizing::new(password), false)
        .await
        .unwrap();
    let now = now_ms().unwrap();
    for (name, mode) in [
        ("Photos", "up"),
        ("Home gateway", "down"),
        ("Media library", "pending"),
        ("Backup archive", "paused"),
        ("Private DNS", "unknown"),
    ] {
        let config = MonitorConfig {
            name: name.into(),
            kind: Kind::Http,
            target: "https://service.example.test/".into(),
            port: None,
            interval_seconds: 60,
            deadline_seconds: 5,
            failure_threshold: 3,
            enabled: mode != "paused",
            alerts: true,
            address_family: Family::Auto,
            accepted_statuses: vec![[200, 299]],
        };
        let mut m = match storage
            .client()
            .monitoring(Operation::Create {
                config,
                now: now - 1_200_000,
                jitter_ms: 0,
                key: None,
            })
            .await
            .unwrap()
        {
            Value::Monitor(m) => *m,
            _ => panic!("create"),
        };
        if mode == "paused" {
            continue;
        }
        for step in 0..20 {
            let mut result = if mode == "unknown" {
                ProbeResult::fault(ProbeError::Permission)
            } else if mode == "down" && step > 14 || mode == "pending" && step == 19 {
                ProbeResult::fault(ProbeError::Timeout)
            } else {
                ProbeResult::success()
            };
            result.elapsed_ms = 32 + step * 2;
            storage
                .client()
                .monitoring(Operation::Complete {
                    id: m.id,
                    revision: m.revision,
                    epoch: m.epoch,
                    due: m.state.next_due_ms,
                    completed: m.state.next_due_ms + 80,
                    result,
                })
                .await
                .unwrap();
            m = storage
                .client()
                .monitors()
                .await
                .unwrap()
                .into_iter()
                .find(|n| n.id == m.id)
                .unwrap();
        }
    }
    // Put the current observation near fixture startup while preserving all stored history.
    let db = rusqlite::Connection::open(state_dir.join("monitor.db")).unwrap();
    db.execute("UPDATE monitor_state SET last_completed_at_ms=?1,next_due_ms=?1+30000 WHERE last_completed_at_ms IS NOT NULL",[now-10_000]).unwrap();
    drop(db);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = Config {
        mode: Mode::Development,
        listen: address,
        state_dir,
        public_origin: url::Url::parse(&format!("http://{address}")).unwrap(),
        destination_policy: Policy::new(vec![]).unwrap(),
        trusted_proxy: None,
        extra_trust_roots: vec![],
        telegram_token: None,
    };
    let health = SchedulerHealth::default();
    let heartbeat = health.clone();
    let progress = tokio::spawn(async move {
        loop {
            heartbeat
                .last_progress_ms
                .store(now_ms().unwrap() as u64, Ordering::Release);
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    println!(
        "review-state={}",
        config.state_dir.join("monitor.db").display()
    );
    println!("http://{address}");
    web::serve_backend(
        listener,
        auth,
        &config,
        health,
        SenderHealth {
            token_present: true,
            ..Default::default()
        },
        async {
            let _ = tokio::signal::ctrl_c().await;
        },
    )
    .await
    .unwrap();
    progress.abort();
    storage.shutdown().await.unwrap();
}
