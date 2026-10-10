#![forbid(unsafe_code)]

use minimalmonitor::{
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
            "minimalmonitor {} (WP-01 foundation; SQLite {})",
            env!("CARGO_PKG_VERSION"),
            rusqlite::version()
        );
        return Ok(());
    }
    let path = args.next().ok_or(Error::Configuration)?;
    if args.next().is_some() || (command != "serve" && command != "check-config") {
        return Err(Error::Configuration);
    }
    let config = Config::load(Path::new(&path))?;
    if command == "check-config" {
        return Ok(());
    }
    let owner = StateOwner::acquire(&config.state_dir)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .enable_all()
        .build()
        .map_err(|_| Error::WorkerStopped)?;
    let result = runtime.block_on(run(config, owner));
    runtime.shutdown_timeout(Duration::from_secs(1));
    result
}

async fn run(config: Config, owner: StateOwner) -> Result<()> {
    let storage = Storage::start(owner).await?;
    let outcome = match tokio::net::TcpListener::bind(config.listen).await {
        Ok(listener) => web::serve(listener, storage.client(), shutdown_signal()).await,
        Err(_) => Err(Error::Io),
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
