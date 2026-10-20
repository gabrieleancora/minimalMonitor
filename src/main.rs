#![forbid(unsafe_code)]

use minimalmonitor::{
    auth::Auth,
    bounds::WorkBounds,
    config::Config,
    error::{Error, Result},
    ownership::StateOwner,
    storage::Storage,
    web,
};
use std::{path::Path, time::Duration};

fn main() {
    if let Err(error) = entry() {
        // No raw OS/SQL/config text, paths, credentials or request access logs.
        eprintln!("minimalmonitor: {error}");
        std::process::exit(1);
    }
}

fn entry() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let command = args.next().ok_or(Error::Configuration)?;
    if command == "--version" && args.next().is_none() {
        println!(
            "minimalmonitor {} (WP-04 notifications; SQLite {})",
            env!("CARGO_PKG_VERSION"),
            rusqlite::version()
        );
        return Ok(());
    }
    let path = args.next().ok_or(Error::Configuration)?;
    if args.next().is_some()
        || !matches!(
            command.to_str(),
            Some("serve" | "check-config" | "admin-bootstrap" | "admin-reset")
        )
    {
        return Err(Error::Configuration);
    }
    let config = Config::load(Path::new(&path))?;
    if command == "check-config" {
        return Ok(());
    }
    let owner = StateOwner::acquire(&config.state_dir)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        // Four probe calls, one sender DNS, one hash and one shutdown helper.
        .max_blocking_threads(7)
        .enable_all()
        .build()
        .map_err(|_| Error::WorkerStopped)?;
    let local = if command == "admin-bootstrap" || command == "admin-reset" {
        use std::io::IsTerminal;
        if !std::io::stdin().is_terminal() {
            return Err(Error::Configuration);
        }
        let user = rpassword::prompt_password("Administrator username: ").map_err(|_| Error::Io)?;
        let password = zeroize::Zeroizing::new(
            rpassword::prompt_password("New password: ").map_err(|_| Error::Io)?,
        );
        let confirmation = zeroize::Zeroizing::new(
            rpassword::prompt_password("Confirm password: ").map_err(|_| Error::Io)?,
        );
        if *password != *confirmation {
            return Err(Error::InvalidInput);
        }
        Some((user, password, command == "admin-reset"))
    } else {
        None
    };
    let result = runtime.block_on(run(config, owner, local));
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}

async fn run(
    config: Config,
    owner: StateOwner,
    local: Option<(String, zeroize::Zeroizing<String>, bool)>,
) -> Result<()> {
    let storage = Storage::start(owner).await?;
    let bounds = WorkBounds::default();
    let outcome = match Auth::new(storage.client(), bounds.clone()).await {
        Ok(auth) => match local {
            Some((user, password, reset)) => auth.local_account(user, password, reset).await,
            None => match tokio::net::TcpListener::bind(config.listen).await {
                Ok(listener) => {
                    match minimalmonitor::probes::Probes::new(
                        config.destination_policy.clone(),
                        bounds.clone(),
                        config.extra_trust_roots.clone(),
                    ) {
                        Ok(probes) => {
                            let health = minimalmonitor::scheduler::SchedulerHealth::default();
                            let sender_health = minimalmonitor::notifications::SenderHealth {
                                token_present: config.telegram_token.is_some(),
                                ..Default::default()
                            };
                            let sender = minimalmonitor::notifications::Sender::new(
                                storage.client(),
                                minimalmonitor::notifications::Telegram::new(
                                    config.telegram_token.clone(),
                                ),
                                bounds,
                                sender_health.clone(),
                                &config.public_origin,
                            );
                            sender
                                .initialize(
                                    config.telegram_token.as_ref().map(|t| t.fingerprint()),
                                    minimalmonitor::auth::now_ms()?,
                                )
                                .await?;
                            let (stop, receive) = tokio::sync::oneshot::channel();
                            let (sender_stop, sender_receive) = tokio::sync::oneshot::channel();
                            let notification = sender.run(async {
                                let _ = sender_receive.await;
                            });
                            tokio::pin!(notification);
                            let scheduler = minimalmonitor::scheduler::run(
                                storage.client(),
                                probes,
                                health.clone(),
                                async {
                                    let _ = receive.await;
                                },
                            );
                            tokio::pin!(scheduler);
                            tokio::select! {
                                result=web::serve_backend(listener,auth,&config,health,sender_health,shutdown_signal())=>{
                                    let _=stop.send(());
                                    let _=sender_stop.send(());
                                    result.and(scheduler.await).and(notification.await)
                                }
                                result=&mut scheduler=>{
                                    let _=sender_stop.send(());
                                    result.and(notification.await).and(Err(Error::WorkerStopped))
                                },
                                result=&mut notification=>{
                                    let _=stop.send(());
                                    result.and(scheduler.await).and(Err(Error::WorkerStopped))
                                },
                            }
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(_) => Err(Error::Io),
            },
        },
        Err(err) => Err(err),
    };
    let closed = storage.shutdown().await;
    outcome.and(closed)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! { _ = terminate.recv() => (), _ = tokio::signal::ctrl_c() => () }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
