//! Short monitoring transactions on the existing bounded SQLite worker.
use super::*;
use crate::monitoring::*;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Serialize, de::DeserializeOwned};

pub enum Operation {
    List,
    Recent,
    Operational,
    RecordPersistenceFaults {
        count: u64,
        now: i64,
    },
    Create {
        config: MonitorConfig,
        now: i64,
        jitter_ms: i64,
        key: Option<String>,
    },
    Edit {
        id: i64,
        revision: i64,
        config: MonitorConfig,
        now: i64,
        jitter_ms: i64,
    },
    Pause {
        id: i64,
        revision: i64,
        now: i64,
    },
    Resume {
        id: i64,
        revision: i64,
        now: i64,
        jitter_ms: i64,
    },
    Delete {
        id: i64,
        revision: i64,
        now: i64,
    },
    Complete {
        id: i64,
        revision: i64,
        epoch: i64,
        due: i64,
        completed: i64,
        result: ProbeResult,
    },
    /// Count known missed slots (at most 500), using normal cadence after a miss.
    CatchUp {
        now: i64,
    },
    /// Returns false while catch-up remains; caller cannot launch probes yet.
    Reanchor {
        now: i64,
        reason: &'static str,
    },
    Maintain {
        now: i64,
    },
    History {
        id: i64,
        start: i64,
        end: i64,
        now: i64,
    },
    Incidents {
        before: i64,
        limit: u32,
    },
}
/// A bounded heartbeat preview, without targets or sensitive error text.
#[derive(Serialize)]
pub struct RecentObservation {
    pub scheduled_ms: i64,
    pub observed_ms: i64,
    pub outcome: Outcome,
}
#[derive(Serialize)]
pub struct Hour {
    pub hour_ms: i64,
    pub counts: Counts,
    pub latency_sum_ms: u64,
    pub latency_count: u64,
    pub latency_max_ms: Option<u32>,
}
#[derive(Serialize)]
pub struct Incident {
    pub id: i64,
    pub monitor_id: i64,
    pub revision: i64,
    pub first_failure_at_ms: i64,
    pub confirmed_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub end_reason: Option<String>,
    pub observation_gap: bool,
}
pub enum Value {
    Monitors(Vec<Monitor>),
    Recent(std::collections::BTreeMap<i64, Vec<RecentObservation>>),
    Monitor(Box<Monitor>),
    Committed(bool),
    Hours(Vec<Hour>, bool),
    Incidents(Vec<Incident>),
    Done,
    Operational {
        dropped_alerts: u64,
        persistence_faults: u64,
    },
}
pub(super) struct Command {
    deadline: Instant,
    operation: Operation,
    reply: oneshot::Sender<Result<Value>>,
}
impl StorageClient {
    pub async fn monitoring(&self, operation: Operation) -> Result<Value> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Unavailable);
        }
        let (reply, receive) = oneshot::channel();
        self.tx
            .try_send(Work::Monitoring(Command {
                deadline: Instant::now() + QUERY_TIME,
                operation,
                reply,
            }))
            .map_err(|e| match e {
                TrySendError::Full(_) => Error::Busy,
                TrySendError::Disconnected(_) => Error::WorkerStopped,
            })?;
        tokio::time::timeout(REPLY_TIME, receive)
            .await
            .map_err(|_| Error::Deadline)?
            .map_err(|_| Error::WorkerStopped)?
    }
    pub async fn monitors(&self) -> Result<Vec<Monitor>> {
        match self.monitoring(Operation::List).await? {
            Value::Monitors(v) => Ok(v),
            _ => Err(Error::Storage),
        }
    }
}
pub(super) fn dispatch(db: &mut Connection, root: &Path, command: Command) {
    if command.reply.is_closed() {
        return;
    }
    let result = (|| {
        if Instant::now() >= command.deadline {
            return Err(Error::Deadline);
        }
        if !matches!(
            command.operation,
            Operation::List
                | Operation::Recent
                | Operation::Operational
                | Operation::History { .. }
                | Operation::Incidents { .. }
        ) && !health(db, root, command.deadline)?.writable
        {
            return Err(Error::DiskPressure);
        }
        db.progress_handler(100, Some(move || Instant::now() >= command.deadline))
            .map_err(|_| Error::Storage)?;
        execute(db, command.operation, command.deadline)
    })();
    let cleared = db
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| Error::Storage);
    let _ = command
        .reply
        .send(result.and_then(|value| cleared.map(|_| value)));
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::Storage)
}
fn decode<T: DeserializeOwned>(text: String) -> Result<T> {
    serde_json::from_str(&text).map_err(|_| Error::Storage)
}
fn word<T: Serialize>(value: &T) -> Result<String> {
    Ok(encode(value)?.trim_matches('"').to_owned())
}
fn enum_value<T: DeserializeOwned>(text: String) -> Result<T> {
    decode(format!("\"{text}\""))
}
fn sql<T>(value: rusqlite::Result<T>) -> Result<T> {
    value.map_err(|_| Error::Storage)
}

fn load(db: &Connection) -> Result<Vec<Monitor>> {
    let mut statement=sql(db.prepare("SELECT m.id,m.revision,m.name,m.kind,m.target,m.port,m.interval_seconds,m.deadline_seconds,m.failure_threshold,m.enabled,m.alerts,m.address_family,m.accepted_statuses,s.epoch_id,s.status,s.failure_streak,s.first_failure_ms,s.last_due_at_ms,s.last_completed_at_ms,s.active_incident_id,s.cadence,s.next_due_ms,s.last_result_json,s.system_error FROM monitors m JOIN monitor_state s ON s.monitor_id=m.id WHERE m.tombstoned_at_ms IS NULL ORDER BY m.id LIMIT 51"))?;
    let mut rows = sql(statement.query([]))?;
    let mut monitors = Vec::with_capacity(50);
    while let Some(row) = sql(rows.next())? {
        if monitors.len() == 50 {
            return Err(Error::Storage);
        }
        let config = MonitorConfig {
            name: sql(row.get(2))?,
            kind: enum_value(sql(row.get(3))?)?,
            target: sql(row.get(4))?,
            port: sql(row.get(5))?,
            interval_seconds: sql(row.get(6))?,
            deadline_seconds: sql(row.get(7))?,
            failure_threshold: sql(row.get(8))?,
            enabled: sql(row.get(9))?,
            alerts: sql(row.get(10))?,
            address_family: enum_value(sql(row.get(11))?)?,
            accepted_statuses: decode(
                sql(row.get::<_, Option<String>>(12))?.unwrap_or("[[200,299]]".into()),
            )?,
        };
        config.validate()?;
        let state = State {
            system_error: sql(row.get::<_, Option<String>>(23))?
                .map(enum_value)
                .transpose()?,
            status: enum_value(sql(row.get(14))?)?,
            failure_streak: sql(row.get(15))?,
            first_failure_ms: sql(row.get(16))?,
            last_due_ms: sql(row.get(17))?,
            last_completed_ms: sql(row.get(18))?,
            active_incident: sql(row.get(19))?,
            cadence: enum_value(sql(row.get(20))?)?,
            next_due_ms: sql(row.get(21))?,
            last_result: sql(row.get::<_, Option<String>>(22))?
                .map(decode)
                .transpose()?,
        };
        monitors.push(Monitor {
            id: sql(row.get(0))?,
            revision: sql(row.get(1))?,
            epoch: sql(row.get(13))?,
            config,
            state,
        });
    }
    Ok(monitors)
}
fn capacity(tx: &Connection, replacing: Option<i64>, config: &MonitorConfig) -> Result<()> {
    let monitors = load(tx)?;
    let total = monitors
        .iter()
        .filter(|m| Some(m.id) != replacing)
        .map(|m| m.config.load())
        .sum::<f64>()
        + config.load();
    if total > 3.0 + 1e-10 {
        Err(Error::Conflict)
    } else {
        Ok(())
    }
}
fn check_time(now: i64) -> Result<()> {
    // Leaves arithmetic room for durations, retention and clock comparisons.
    if !(0..=i64::MAX - 366 * DAY_MS).contains(&now) {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}
fn due(config: &MonitorConfig, now: i64, jitter: i64) -> Result<i64> {
    check_time(now)?;
    if !(0..config.interval_ms()).contains(&jitter) {
        return Err(Error::InvalidInput);
    }
    Ok(now + jitter)
}
fn epoch(
    tx: &Transaction<'_>,
    id: i64,
    revision: i64,
    config: &MonitorConfig,
    now: i64,
    due: i64,
    reason: Option<&str>,
) -> Result<i64> {
    sql(tx.execute("INSERT INTO schedule_epochs(monitor_id,revision,anchor_at_ms,interval_seconds,enabled_start_ms,enabled_end_ms,discontinuity) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![id,revision,due,config.interval_seconds,now,(!config.enabled).then_some(now),reason]))?;
    Ok(tx.last_insert_rowid())
}
fn save_state(tx: &Transaction<'_>, monitor: &Monitor) -> Result<()> {
    let s = &monitor.state;
    sql(tx.execute("INSERT INTO monitor_state(monitor_id,status,failure_streak,last_due_at_ms,last_completed_at_ms,last_outcome,last_error_category,active_incident_id,epoch_id,next_due_ms,cadence,first_failure_ms,last_result_json,system_error) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(monitor_id) DO UPDATE SET status=excluded.status,failure_streak=excluded.failure_streak,last_due_at_ms=excluded.last_due_at_ms,last_completed_at_ms=excluded.last_completed_at_ms,last_outcome=excluded.last_outcome,last_error_category=excluded.last_error_category,active_incident_id=excluded.active_incident_id,epoch_id=excluded.epoch_id,next_due_ms=excluded.next_due_ms,cadence=excluded.cadence,first_failure_ms=excluded.first_failure_ms,last_result_json=excluded.last_result_json,system_error=excluded.system_error",params![monitor.id,word(&s.status)?,s.failure_streak,s.last_due_ms,s.last_completed_ms,s.last_result.map(|r|word(&r.outcome)).transpose()?,s.last_result.and_then(|r|r.error).map(|e|word(&e)).transpose()?,s.active_incident,monitor.epoch,s.next_due_ms,word(&s.cadence)?,s.first_failure_ms,s.last_result.map(|r|encode(&r)).transpose()?,s.system_error.map(|e|word(&e)).transpose()?]))?;
    Ok(())
}
fn set_config(tx: &Transaction<'_>, id: i64, revision: i64, c: &MonitorConfig) -> Result<()> {
    sql(tx.execute("UPDATE monitors SET name=?2,kind=?3,target=?4,port=?5,accepted_statuses=?6,interval_seconds=?7,deadline_seconds=?8,failure_threshold=?9,enabled=?10,alerts=?11,address_family=?12,revision=?13 WHERE id=?1",params![id,c.name,word(&c.kind)?,c.target,c.port,encode(&c.accepted_statuses)?,c.interval_seconds,c.deadline_seconds,c.failure_threshold,c.enabled,c.alerts,word(&c.address_family)?,revision]))?;
    Ok(())
}
fn close_epoch(tx: &Transaction<'_>, m: &Monitor, now: i64) -> Result<()> {
    sql(tx.execute("UPDATE schedule_epochs SET enabled_end_ms=max(enabled_start_ms,?2) WHERE id=?1 AND enabled_end_ms IS NULL",params![m.epoch,now]))?;
    Ok(())
}
fn close_incident(tx: &Transaction<'_>, m: &mut Monitor, now: i64, reason: &str) -> Result<()> {
    if let Some(incident) = m.state.active_incident.take() {
        sql(tx.execute("UPDATE incidents SET ended_at_ms=max(confirmed_at_ms,?2),end_reason=?3,last_evidence_at_ms=max(last_evidence_at_ms,?2) WHERE id=?1 AND ended_at_ms IS NULL",params![incident,now,reason]))?;
        if reason != "recovered" {
            sql(tx.execute("UPDATE notification_outbox SET state='canceled',terminal_at_ms=?2 WHERE incident_id=?1 AND state IN ('pending','sending')",params![incident,now]))?;
        }
    }
    Ok(())
}
fn gap(tx: &Transaction<'_>, m: &mut Monitor) -> Result<()> {
    m.state.status = Status::Unknown;
    m.state.failure_streak = 0;
    m.state.first_failure_ms = None;
    m.state.cadence = Cadence::Normal;
    if let Some(id) = m.state.active_incident {
        sql(tx.execute("UPDATE incidents SET observation_gap=1 WHERE id=?1", [id]))?;
    }
    Ok(())
}
fn sample(
    tx: &Transaction<'_>,
    id: i64,
    due: i64,
    count: i64,
    result: Option<ProbeResult>,
) -> Result<()> {
    let success = result.is_some_and(|r| r.outcome == Outcome::Success);
    let failure = result.is_some_and(|r| r.outcome == Outcome::Failure);
    let valid = success || failure;
    let latency = if valid {
        result.map(|r| r.elapsed_ms).unwrap_or(0)
    } else {
        0
    };
    sql(tx.execute("INSERT INTO hourly_stats(monitor_id,hour_ms,expected,successful,failed,unknown,latency_sum_ms,latency_count,latency_max_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(monitor_id,hour_ms) DO UPDATE SET expected=expected+excluded.expected,successful=successful+excluded.successful,failed=failed+excluded.failed,unknown=unknown+excluded.unknown,latency_sum_ms=latency_sum_ms+excluded.latency_sum_ms,latency_count=latency_count+excluded.latency_count,latency_max_ms=CASE WHEN excluded.latency_max_ms IS NULL THEN latency_max_ms ELSE max(coalesce(latency_max_ms,0),excluded.latency_max_ms) END",params![id,due.div_euclid(HOUR_MS)*HOUR_MS,count,if success {count} else {0},if failure {count} else {0},if valid {0} else {count},latency,if valid {1} else {0},valid.then_some(latency)]))?;
    Ok(())
}
/// Compact sanitized snapshot commits with its unique incident transition.
fn event(tx: &Transaction<'_>, m: &Monitor, incident: i64, kind: &str, now: i64) -> Result<()> {
    let (enabled, revision): (bool, i64) = sql(tx.query_row(
        "SELECT telegram_enabled,destination_revision FROM settings WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if !enabled || !m.config.alerts {
        return Ok(());
    }
    let payload = serde_json::to_string(&crate::notifications::EventContent {
        name: crate::notifications::clean_name(&m.config.name),
        observed_at_ms: now,
        reason: if kind == "down" {
            m.state.last_result.and_then(|r| r.error)
        } else {
            None
        },
    })
    .map_err(|_| Error::Storage)?;
    let (count,bytes):(i64,i64)=sql(tx.query_row("SELECT count(*),coalesce(sum(length(CAST(payload AS BLOB))),0) FROM notification_outbox WHERE state IN ('pending','sending')",[],|r|Ok((r.get(0)?,r.get(1)?))))?;
    if count >= 1000 || bytes + payload.len() as i64 > 2 * 1024 * 1024 {
        sql(tx.execute("UPDATE health_state SET dropped_alerts=min(dropped_alerts+1,9223372036854775807),updated_at_ms=?1 WHERE id=1",[now]))?;
    } else {
        sql(tx.execute("INSERT INTO notification_outbox(event_identity,incident_id,kind,destination_revision,payload,state,next_attempt_at_ms,created_at_ms,expires_at_ms) VALUES(?1,?2,?3,?4,?5,'pending',?6,?6,?7)",params![format!("{incident}:{kind}"),incident,kind,revision,payload,now,now+6*HOUR_MS]))?;
    }
    Ok(())
}
fn missed(tx: &Transaction<'_>, m: &mut Monitor, until: i64, allowance: &mut i64) -> Result<bool> {
    if !m.config.enabled {
        return Ok(true);
    }
    if m.state.next_due_ms <= until {
        m.state.system_error = Some(ProbeError::Scheduler);
        gap(tx, m)?;
        // Older coverage is outside the retained window; no replay or allocation.
        let boundary = (until - 90 * DAY_MS).max(0);
        if m.state.next_due_ms < boundary {
            m.state.next_due_ms += ((boundary - m.state.next_due_ms + m.config.interval_ms() - 1)
                / m.config.interval_ms())
                * m.config.interval_ms();
        }
        while m.state.next_due_ms <= until && *allowance > 0 {
            let slot = m.state.next_due_ms;
            // Count groups within an hour arithmetically, without raw fake probes.
            let upper = until.min((slot.div_euclid(HOUR_MS) + 1) * HOUR_MS - 1);
            let count = ((upper - slot) / m.config.interval_ms() + 1).min(*allowance);
            sample(tx, m.id, slot, count, None)?;
            // Raw unknown slots retain exact recent-window coverage. They are
            // explicitly absent observations, never synthetic target results.
            for index in 0..count {
                let due = slot + index * m.config.interval_ms();
                if due >= until - 7 * DAY_MS {
                    sql(tx.execute("INSERT INTO probe_results(monitor_id,epoch_id,revision,scheduled_slot_ms,observed_at_ms,outcome,error_category) VALUES(?1,?2,?3,?4,?5,'unknown','scheduler')",params![m.id,m.epoch,m.revision,due,until]))?;
                }
            }
            m.state.next_due_ms += count * m.config.interval_ms();
            *allowance -= count;
        }
        sql(tx.execute(
            "UPDATE schedule_epochs SET aggregated_through_ms=?2 WHERE id=?1",
            params![m.epoch, m.state.next_due_ms],
        ))?;
        save_state(tx, m)?;
    }
    Ok(m.state.next_due_ms > until)
}
fn execute(db: &mut Connection, operation: Operation, deadline: Instant) -> Result<Value> {
    match operation {
        Operation::List => return Ok(Value::Monitors(load(db)?)),
        Operation::Recent => {
            let monitors = load(db)?;
            let mut recent = std::collections::BTreeMap::new();
            let mut statement = sql(db.prepare("SELECT scheduled_slot_ms,observed_at_ms,outcome FROM probe_results WHERE monitor_id=?1 ORDER BY scheduled_slot_ms DESC LIMIT 12"))?;
            for monitor in monitors {
                let values = sql(statement.query_map([monitor.id], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                }))?;
                let mut observations = Vec::with_capacity(12);
                for value in values {
                    let (scheduled_ms, observed_ms, outcome) = sql(value)?;
                    observations.push(RecentObservation {
                        scheduled_ms,
                        observed_ms,
                        outcome: enum_value(outcome)?,
                    });
                }
                observations.reverse();
                recent.insert(monitor.id, observations);
            }
            return Ok(Value::Recent(recent));
        }
        Operation::Operational => {
            let (dropped, faults): (i64, i64) = sql(db.query_row(
                "SELECT dropped_alerts,persistence_faults FROM health_state WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            ))?;
            return Ok(Value::Operational {
                dropped_alerts: dropped as u64,
                persistence_faults: faults as u64,
            });
        }
        Operation::History {
            id,
            start,
            end,
            now,
        } => return history(db, id, start, end, now),
        Operation::Incidents { before, limit } => return incidents(db, before, limit),
        _ => (),
    }
    let tx = sql(db.transaction())?;
    let value = match operation {
        Operation::RecordPersistenceFaults { count, now } => {
            check_time(now)?;
            let count = count.min(i64::MAX as u64) as i64;
            sql(tx.execute("UPDATE health_state SET persistence_faults=CASE WHEN persistence_faults>?2-?1 THEN ?2 ELSE persistence_faults+?1 END,updated_at_ms=?3 WHERE id=1",params![count,i64::MAX,now]))?;
            Value::Done
        }
        Operation::Create {
            config,
            now,
            jitter_ms,
            key,
        } => {
            config.validate()?;
            let next = due(&config, now, jitter_ms)?;
            let digest: [u8; 32] = {
                use sha2::Digest;
                sha2::Sha256::digest(encode(&config)?.as_bytes()).into()
            };
            if let Some(key) = key.as_ref() {
                if key.is_empty()
                    || key.len() > 128
                    || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                {
                    return Err(Error::InvalidInput);
                }
                let old:Option<(String,Vec<u8>,String)>=sql(tx.query_row("SELECT operation,request_digest,result_reference FROM request_keys WHERE admin_id=1 AND key=?1 AND created_at_ms>=?2",params![key,now-DAY_MS],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional())?;
                if let Some((operation, old, reference)) = old {
                    if old != digest || operation != "monitor_create" {
                        return Err(Error::Conflict);
                    }
                    let id = reference.parse::<i64>().map_err(|_| Error::Storage)?;
                    return Ok(Value::Monitor(Box::new(
                        load(&tx)?
                            .into_iter()
                            .find(|m| m.id == id)
                            .ok_or(Error::Conflict)?,
                    )));
                }
            }
            capacity(&tx, None, &config)?;
            let count: i64 = sql(tx.query_row("SELECT count(*) FROM monitors", [], |r| r.get(0)))?;
            if count >= 50 {
                return Err(Error::Conflict);
            }
            let previous: i64 = sql(tx.query_row(
                "SELECT last_id FROM monitor_identity_allocator WHERE id=1",
                [],
                |r| r.get(0),
            ))?;
            let id = previous.checked_sub(1).ok_or(Error::Conflict)?;
            sql(tx.execute(
                "UPDATE monitor_identity_allocator SET last_id=?1 WHERE id=1",
                [id],
            ))?;
            sql(tx.execute("INSERT INTO monitors(id,name,kind,target,port,accepted_statuses,interval_seconds,deadline_seconds,failure_threshold,enabled,alerts,address_family,revision) VALUES(?12,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1)",params![config.name,word(&config.kind)?,config.target,config.port,encode(&config.accepted_statuses)?,config.interval_seconds,config.deadline_seconds,config.failure_threshold,config.enabled,config.alerts,word(&config.address_family)?,id]))?;
            let epoch = epoch(&tx, id, 1, &config, now, next, None)?;
            let m = Monitor {
                id,
                revision: 1,
                epoch,
                state: State::new(config.enabled, next),
                config,
            };
            save_state(&tx, &m)?;
            if let Some(key) = key {
                sql(tx.execute("DELETE FROM request_keys WHERE created_at_ms<?1 OR rowid IN (SELECT rowid FROM request_keys ORDER BY created_at_ms DESC,rowid DESC LIMIT -1 OFFSET 99)",[now-DAY_MS]))?;
                sql(tx.execute("INSERT INTO request_keys(admin_id,key,operation,request_digest,result_reference,created_at_ms) VALUES(1,?1,'monitor_create',?2,?3,?4)",params![key,digest.as_slice(),id.to_string(),now]))?;
            }
            Value::Monitor(Box::new(m))
        }
        Operation::Edit {
            id,
            revision,
            config,
            now,
            jitter_ms,
        } => {
            config.validate()?;
            let next = due(&config, now, jitter_ms)?;
            let mut m = find(&tx, id, revision)?;
            capacity(&tx, Some(id), &config)?;
            let mut allowance = 500;
            if !missed(&tx, &mut m, now - 1, &mut allowance)? {
                return Err(Error::Busy);
            }
            close_epoch(&tx, &m, now)?;
            close_incident(&tx, &mut m, now, "configuration_changed")?;
            m.revision = m.revision.checked_add(1).ok_or(Error::Conflict)?;
            m.config = config;
            m.epoch = epoch(
                &tx,
                id,
                m.revision,
                &m.config,
                now,
                next,
                Some("configuration_changed"),
            )?;
            m.state = State::new(m.config.enabled, next);
            set_config(&tx, id, m.revision, &m.config)?;
            save_state(&tx, &m)?;
            Value::Monitor(Box::new(m))
        }
        Operation::Pause { id, revision, now }
        | Operation::Resume {
            id,
            revision,
            now,
            jitter_ms: _,
        }
        | Operation::Delete { id, revision, now } => {
            check_time(now)?;
            let mut m = find(&tx, id, revision)?;
            let (enabled, deleting, jitter) = match operation {
                Operation::Resume { jitter_ms, .. } => (true, false, jitter_ms),
                Operation::Delete { .. } => (false, true, 0),
                _ => (false, false, 0),
            };
            let mut allowance = 500;
            if !missed(&tx, &mut m, now - 1, &mut allowance)? {
                return Err(Error::Busy);
            }
            close_epoch(&tx, &m, now)?;
            close_incident(
                &tx,
                &mut m,
                now,
                if deleting { "deleted" } else { "paused" },
            )?;
            m.config.enabled = enabled;
            capacity(&tx, Some(id), &m.config)?;
            m.revision = m.revision.checked_add(1).ok_or(Error::Conflict)?;
            let next = due(&m.config, now, jitter)?;
            m.epoch = epoch(
                &tx,
                id,
                m.revision,
                &m.config,
                now,
                next,
                Some(if enabled { "resume" } else { "pause" }),
            )?;
            m.state = State::new(enabled, next);
            set_config(&tx, id, m.revision, &m.config)?;
            save_state(&tx, &m)?;
            if deleting {
                sql(tx.execute(
                    "UPDATE monitors SET tombstoned_at_ms=?2 WHERE id=?1",
                    params![id, now],
                ))?;
            }
            Value::Monitor(Box::new(m))
        }
        Operation::Complete {
            id,
            revision,
            epoch,
            due,
            completed,
            result,
        } => {
            check_time(completed)?;
            let Some(mut m) = load(&tx)?.into_iter().find(|m| {
                m.id == id && m.revision == revision && m.epoch == epoch && m.config.enabled
            }) else {
                return Ok(Value::Committed(false));
            };
            if m.state.next_due_ms != due
                || completed < due
                || completed > due + i64::from(m.config.deadline_seconds) * 1000 + 2000
            {
                return Ok(Value::Committed(false));
            }
            if result.elapsed_ms > 30_000
                || result
                    .status_code
                    .is_some_and(|c| !(100..=599).contains(&c))
                || result.destination_family == Some(Family::Auto)
                || result.error.is_some_and(|e| e.outcome() != result.outcome)
            {
                return Err(Error::InvalidInput);
            }
            let inserted=sql(tx.execute("INSERT INTO probe_results(monitor_id,epoch_id,revision,scheduled_slot_ms,observed_at_ms,outcome,elapsed_ms,status_code,error_category,destination_family) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10) ON CONFLICT(monitor_id,epoch_id,revision,scheduled_slot_ms) DO NOTHING",params![id,epoch,revision,due,completed,word(&result.outcome)?,result.elapsed_ms,result.status_code,result.error.map(|e|word(&e)).transpose()?,result.destination_family.map(|f|word(&f)).transpose()?]))?;
            if inserted == 0 {
                return Ok(Value::Committed(false));
            }
            sample(&tx, id, due, 1, Some(result))?;
            match m.state.apply(&m.config, due, completed, result) {
                Transition::Down => {
                    sql(tx.execute("INSERT INTO incidents(monitor_id,revision,first_failure_at_ms,confirmed_at_ms,last_evidence_at_ms) VALUES(?1,?2,?3,?4,?4)",params![id,revision,m.state.first_failure_ms,completed]))?;
                    let incident = tx.last_insert_rowid();
                    m.state.active_incident = Some(incident);
                    event(&tx, &m, incident, "down", completed)?;
                }
                Transition::Recovery => {
                    let incident = m.state.active_incident.expect("recovery requires incident");
                    event(&tx, &m, incident, "recovery", completed)?;
                    close_incident(&tx, &mut m, completed, "recovered")?;
                }
                Transition::None => (),
            }
            if result.outcome == Outcome::Unknown {
                gap(&tx, &mut m)?;
            }
            if let Some(incident) = m.state.active_incident {
                sql(tx.execute(
                    "UPDATE incidents SET last_evidence_at_ms=?2 WHERE id=?1",
                    params![incident, completed],
                ))?;
            }
            sql(tx.execute(
                "UPDATE schedule_epochs SET aggregated_through_ms=?2 WHERE id=?1",
                params![epoch, m.state.next_due_ms],
            ))?;
            save_state(&tx, &m)?;
            Value::Committed(true)
        }
        Operation::CatchUp { now } => {
            check_time(now)?;
            let mut allowance = 500;
            let mut done = true;
            for mut m in load(&tx)? {
                let until = now - i64::from(m.config.deadline_seconds) * 1000 - 2000;
                done = missed(&tx, &mut m, until, &mut allowance)? && done;
            }
            Value::Committed(done)
        }
        Operation::Reanchor { now, reason } => {
            check_time(now)?;
            if !matches!(reason, "restart" | "clock_jump") {
                return Err(Error::InvalidInput);
            }
            let mut allowance = 500;
            let mut ready = true;
            let mut monitors = load(&tx)?;
            for m in &mut monitors {
                if m.config.enabled {
                    gap(&tx, m)?;
                    m.state.system_error = Some(if reason == "clock_jump" {
                        ProbeError::ClockJump
                    } else {
                        ProbeError::Scheduler
                    });
                    ready = missed(&tx, m, now - 1, &mut allowance)? && ready;
                    save_state(&tx, m)?;
                }
            }
            if ready {
                for m in &mut monitors {
                    if m.config.enabled {
                        close_epoch(&tx, m, now)?;
                        let jitter = random_jitter(&m.config)?;
                        let next = due(&m.config, now, jitter)?;
                        m.epoch = epoch(&tx, m.id, m.revision, &m.config, now, next, Some(reason))?;
                        m.state.next_due_ms = next;
                        save_state(&tx, m)?;
                    }
                }
            }
            Value::Committed(ready)
        }
        Operation::Maintain { now } => {
            check_time(now)?;
            maintain(&tx, now)?;
            Value::Done
        }
        _ => return Err(Error::Storage),
    };
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    sql(tx.commit())?;
    Ok(value)
}
pub fn random_jitter(config: &MonitorConfig) -> Result<i64> {
    config.validate()?;
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes).map_err(|_| Error::Unavailable)?;
    Ok((u64::from_ne_bytes(bytes) % config.interval_ms() as u64) as i64)
}
fn find(tx: &Transaction<'_>, id: i64, revision: i64) -> Result<Monitor> {
    load(tx)?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or(Error::NotFound)
        .and_then(|m| {
            if m.revision == revision {
                Ok(m)
            } else {
                Err(Error::Conflict)
            }
        })
}
fn maintain(tx: &Transaction<'_>, now: i64) -> Result<()> {
    // Across all history tables together: at most 500 deleted rows per command.
    let mut remaining = 500usize;
    for (query, cutoff) in [
        (
            "DELETE FROM probe_results WHERE rowid IN (SELECT rowid FROM probe_results WHERE scheduled_slot_ms<?1 ORDER BY scheduled_slot_ms LIMIT ?2)",
            now - 7 * DAY_MS,
        ),
        (
            "DELETE FROM hourly_stats WHERE rowid IN (SELECT rowid FROM hourly_stats WHERE hour_ms<?1 ORDER BY hour_ms LIMIT ?2)",
            (now - 90 * DAY_MS).div_euclid(HOUR_MS) * HOUR_MS,
        ),
        (
            "DELETE FROM notification_outbox WHERE id IN (SELECT id FROM notification_outbox WHERE state IN ('delivered','failed','expired','canceled') AND coalesce(terminal_at_ms,created_at_ms)<?1 LIMIT ?2)",
            now - 7 * DAY_MS,
        ),
        (
            "DELETE FROM notification_outbox WHERE id IN (SELECT id FROM notification_outbox WHERE state IN ('delivered','failed','expired','canceled') ORDER BY created_at_ms DESC,id DESC LIMIT ?2 OFFSET 2000)",
            now,
        ),
        (
            "DELETE FROM incidents WHERE id IN (SELECT id FROM incidents WHERE ended_at_ms<?1 AND NOT EXISTS(SELECT 1 FROM notification_outbox o WHERE o.incident_id=incidents.id) AND NOT EXISTS(SELECT 1 FROM monitor_state s WHERE s.active_incident_id=incidents.id) ORDER BY ended_at_ms LIMIT ?2)",
            now - 180 * DAY_MS,
        ),
        (
            "DELETE FROM schedule_epochs WHERE id IN (SELECT e.id FROM schedule_epochs e WHERE enabled_end_ms<?1 AND NOT EXISTS(SELECT 1 FROM monitor_state s WHERE s.epoch_id=e.id) AND NOT EXISTS(SELECT 1 FROM probe_results p WHERE p.monitor_id=e.monitor_id AND p.epoch_id=e.id) ORDER BY enabled_end_ms LIMIT ?2)",
            now - 90 * DAY_MS,
        ),
    ] {
        if remaining == 0 {
            break;
        }
        remaining -= sql(tx.execute(query, params![cutoff, remaining as i64]))?;
    }
    // Tombstones delete in dependency order, bounded by the same allowance.
    let deleted: Option<i64> = sql(tx
        .query_row(
            "SELECT id FROM monitors WHERE tombstoned_at_ms IS NOT NULL ORDER BY id LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional())?;
    if let Some(id) = deleted {
        for table in [
            "notification_outbox",
            "probe_results",
            "hourly_stats",
            "monitor_state",
            "incidents",
            "schedule_epochs",
        ] {
            if remaining == 0 {
                break;
            }
            let query = if table == "notification_outbox" {
                "DELETE FROM notification_outbox WHERE id IN (SELECT o.id FROM notification_outbox o JOIN incidents i ON i.id=o.incident_id WHERE i.monitor_id=?1 LIMIT ?2)".to_owned()
            } else {
                format!(
                    "DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE monitor_id=?1 LIMIT ?2)"
                )
            };
            let count = sql(tx.execute(&query, params![id, remaining as i64]))?;
            remaining -= count;
            let query = if table == "notification_outbox" {
                "SELECT EXISTS(SELECT 1 FROM notification_outbox o JOIN incidents i ON i.id=o.incident_id WHERE i.monitor_id=?1)".to_owned()
            } else {
                format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE monitor_id=?1)")
            };
            if sql(tx.query_row(&query, [id], |r| r.get::<_, bool>(0)))? {
                return Ok(());
            }
        }
        if remaining > 0 {
            sql(tx.execute("DELETE FROM monitors WHERE id=?1", [id]))?;
        }
    }
    Ok(())
}
fn history(db: &Connection, id: i64, start: i64, end: i64, now: i64) -> Result<Value> {
    if !sql(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM monitors WHERE id=?1 AND tombstoned_at_ms IS NULL)",
        [id],
        |r| r.get::<_, bool>(0),
    ))? {
        return Err(Error::NotFound);
    }
    check_time(end)?;
    if start < 0
        || end <= start
        || end > now
        || end - start > 90 * DAY_MS
        || start < now - 90 * DAY_MS
    {
        return Err(Error::InvalidInput);
    }
    // Group aligned hours into <=300 buckets, summing counts rather than means.
    let width = (((end - start + HOUR_MS - 1) / HOUR_MS + 298) / 299).max(1) * HOUR_MS;
    let recent = start >= now - 7 * DAY_MS;
    let query = if recent {
        "SELECT (scheduled_slot_ms/?4)*?4,count(*),sum(outcome='success'),sum(outcome='failure'),sum(outcome='unknown'),sum(CASE WHEN outcome!='unknown' THEN coalesce(elapsed_ms,0) ELSE 0 END),sum(outcome!='unknown'),max(CASE WHEN outcome!='unknown' THEN elapsed_ms ELSE NULL END) FROM probe_results WHERE monitor_id=?1 AND scheduled_slot_ms>=?2 AND scheduled_slot_ms<?3 GROUP BY 1 ORDER BY 1 LIMIT 300"
    } else {
        "SELECT (hour_ms/?4)*?4,sum(expected),sum(successful),sum(failed),sum(unknown),sum(latency_sum_ms),sum(latency_count),max(latency_max_ms) FROM hourly_stats WHERE monitor_id=?1 AND hour_ms>=?2 AND hour_ms<?3 GROUP BY 1 ORDER BY 1 LIMIT 300"
    };
    let mut statement = sql(db.prepare(query))?;
    let hours = sql(statement.query_map(
        params![
            id,
            if recent {
                start
            } else {
                start.div_euclid(HOUR_MS) * HOUR_MS
            },
            end,
            width
        ],
        |r| {
            Ok(Hour {
                hour_ms: r.get(0)?,
                counts: Counts {
                    expected: r.get::<_, i64>(1)? as u64,
                    successful: r.get::<_, i64>(2)? as u64,
                    failed: r.get::<_, i64>(3)? as u64,
                    unknown: r.get::<_, i64>(4)? as u64,
                },
                latency_sum_ms: r.get::<_, i64>(5)? as u64,
                latency_count: r.get::<_, i64>(6)? as u64,
                latency_max_ms: r.get(7)?,
            })
        },
    ))?;
    let discontinuity:bool=sql(db.query_row("SELECT EXISTS(SELECT 1 FROM schedule_epochs WHERE monitor_id=?1 AND enabled_start_ms>=?2 AND enabled_start_ms<?3 AND discontinuity IN ('restart','clock_jump'))",params![id,start,end],|r|r.get(0)))?;
    Ok(Value::Hours(sql(hours.collect())?, discontinuity))
}
fn incidents(db: &Connection, before: i64, limit: u32) -> Result<Value> {
    if before < 1 || !(1..=100).contains(&limit) {
        return Err(Error::InvalidInput);
    }
    let mut statement=sql(db.prepare("SELECT i.id,i.monitor_id,i.revision,i.first_failure_at_ms,i.confirmed_at_ms,i.ended_at_ms,i.end_reason,i.observation_gap FROM incidents i JOIN monitors m ON m.id=i.monitor_id WHERE i.id<?1 AND m.tombstoned_at_ms IS NULL ORDER BY i.id DESC LIMIT ?2"))?;
    let rows = sql(statement.query_map(params![before, limit], |r| {
        Ok(Incident {
            id: r.get(0)?,
            monitor_id: r.get(1)?,
            revision: r.get(2)?,
            first_failure_at_ms: r.get(3)?,
            confirmed_at_ms: r.get(4)?,
            ended_at_ms: r.get(5)?,
            end_reason: r.get(6)?,
            observation_gap: r.get(7)?,
        })
    }))?;
    Ok(Value::Incidents(sql(rows.collect())?))
}
