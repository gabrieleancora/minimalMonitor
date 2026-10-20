//! One owner, no probe queue, four admitted futures, revision/epoch cancellation.
use crate::{
    auth::now_ms,
    error::{Error, Result},
    monitoring::Monitor,
    probes::Probes,
    storage::{
        StorageClient,
        monitoring::{Operation, Value},
    },
};
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    task::{AbortHandle, JoinSet},
    time::Instant,
};

#[derive(Clone, Default)]
pub struct SchedulerHealth {
    pub degraded: Arc<AtomicBool>,
    pub missed_persistence: Arc<AtomicU64>,
    pub last_progress_ms: Arc<AtomicU64>,
}
impl SchedulerHealth {
    pub fn ready(&self, now: i64) -> bool {
        !self.degraded.load(Ordering::Acquire)
            && now >= 0
            && (now as u64).saturating_sub(self.last_progress_ms.load(Ordering::Acquire)) < 5000
    }
}
struct Active {
    revision: i64,
    epoch: i64,
    abort: AbortHandle,
}
/// Both readings are injected for deterministic cadence and clock-jump tests.
pub trait Clock: Send + Sync {
    fn read(&self) -> Result<(i64, Duration)>;
}
struct SystemClock(Instant);
impl Clock for SystemClock {
    fn read(&self) -> Result<(i64, Duration)> {
        Ok((now_ms()?, self.0.elapsed()))
    }
}
pub async fn run(
    storage: StorageClient,
    probes: Probes,
    health: SchedulerHealth,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    run_with_clock(
        storage,
        probes,
        health,
        Arc::new(SystemClock(Instant::now())),
        shutdown,
    )
    .await
}
pub async fn run_with_clock(
    storage: StorageClient,
    probes: Probes,
    health: SchedulerHealth,
    clock: Arc<dyn Clock>,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    tokio::pin!(shutdown);
    let mut tasks = JoinSet::new();
    let mut active = HashMap::<i64, Active>::with_capacity(50);
    let mut pending_reanchor = Some("restart");
    let (mut anchor_wall, mut anchor_mono) = clock.read()?;
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_maintenance = Instant::now() - Duration::from_secs(60);
    let mut persisted_faults = 0;
    loop {
        tokio::select! {
            biased;
            _=&mut shutdown=>break,
            joined=tasks.join_next(),if !tasks.is_empty()=>{
                if let Some(Ok((id,revision,epoch,due,completed,result)))=joined {
                    if active.get(&id).is_some_and(|a|a.revision==revision && a.epoch==epoch) {active.remove(&id);}
                    if storage.monitoring(Operation::Complete {id,revision,epoch,due,completed,result}).await.is_err() {
                        health.degraded.store(true,Ordering::Release);
                        // This scheduler is the sole writer; the API only reads.
                        let count=health.missed_persistence.load(Ordering::Relaxed);
                        health.missed_persistence.store(count.saturating_add(1),Ordering::Relaxed);
                    }
                } else if joined.is_some_and(|r|r.is_err_and(|e|e.is_panic())) {return Err(Error::WorkerStopped);}
            }
            _=tick.tick()=>{
                let (wall,monotonic)=clock.read()?;
                let expected=anchor_wall+monotonic.saturating_sub(anchor_mono).as_millis() as i64;
                if (wall-expected).unsigned_abs()>2000 {
                    for (_,work) in active.drain() {work.abort.abort();}
                    pending_reanchor=Some("clock_jump");anchor_wall=wall;anchor_mono=monotonic;
                }
                // Retention must progress even when catch-up/new-epoch inserts
                // hit a row ceiling. Keep this separate, admitted transaction
                // before every recovery attempt, with the same 500-row bound.
                if pending_reanchor.is_some() || last_maintenance.elapsed()>=Duration::from_secs(1) {
                    if storage.monitoring(Operation::Maintain{now:wall}).await.is_err() {
                        health.degraded.store(true,Ordering::Release);
                        continue;
                    }
                    last_maintenance=Instant::now();
                }
                if let Some(reason)=pending_reanchor {
                    health.degraded.store(true,Ordering::Release);
                    // No probes until all bounded restart catch-up chunks commit.
                    match storage.monitoring(Operation::Reanchor{now:wall,reason}).await {
                        Ok(Value::Committed(true))=>{pending_reanchor=None;anchor_wall=wall;anchor_mono=monotonic;},
                        Ok(_)=>continue,
                        Err(_)=>{health.degraded.store(true,Ordering::Release);continue;}
                    }
                }
                let snapshot=match storage.monitors().await {Ok(m)=>m,Err(_)=>{health.degraded.store(true,Ordering::Release);continue;}};
                for (id,work) in &active {
                    if !snapshot.iter().any(|m|m.id==*id && m.revision==work.revision && m.epoch==work.epoch && m.config.enabled) {work.abort.abort();}
                }
                active.retain(|id,work|snapshot.iter().any(|m|m.id==*id && m.revision==work.revision && m.epoch==work.epoch && m.config.enabled));
                let monotonic_wall=anchor_wall+monotonic.saturating_sub(anchor_mono).as_millis() as i64;
                // Due ordering with ID tie-break keeps synchronized bursts fair.
                let mut due:Vec<Monitor>=snapshot.into_iter().filter(|m|m.config.enabled&&!active.contains_key(&m.id)&&m.state.next_due_ms<=monotonic_wall).collect();
                due.sort_by_key(|m|(m.state.next_due_ms,m.id));
                for monitor in due {
                    if monotonic_wall-monitor.state.next_due_ms>2000 {continue;}
                    let Ok(permit)=probes.admit() else {break;};
                    let runner=probes.clone();let clock=clock.clone();let id=monitor.id;let revision=monitor.revision;let epoch=monitor.epoch;let slot=monitor.state.next_due_ms;
                    let abort=tasks.spawn(async move {
                        let result=runner.run(&monitor.config,permit).await;
                        (id,revision,epoch,slot,clock.read().map(|(wall,_)|wall).unwrap_or(slot),result)
                    });
                    active.insert(id,Active {revision,epoch,abort});
                }
                // Catch-up never touches an in-flight slot before deadline+grace.
                let healthy=storage.monitoring(Operation::CatchUp{now:wall}).await.is_ok();
                health.degraded.store(!healthy,Ordering::Release);
                health.last_progress_ms.store(wall as u64,Ordering::Release);
                let total=health.missed_persistence.load(Ordering::Relaxed);
                if healthy && total>persisted_faults && storage.monitoring(Operation::RecordPersistenceFaults{count:total-persisted_faults,now:wall}).await.is_ok() {persisted_faults=total;}
            }
        }
    }
    // Persist results already completed, cancel unfinished probes, never await an
    // uncancellable native call while holding the database's ownership lock.
    while let Some(result) = tasks.try_join_next() {
        if let Ok((id, revision, epoch, due, completed, result)) = result {
            let _ = storage
                .monitoring(Operation::Complete {
                    id,
                    revision,
                    epoch,
                    due,
                    completed,
                    result,
                })
                .await;
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    health.degraded.store(true, Ordering::Release);
    Ok(())
}
