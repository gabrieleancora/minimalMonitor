//! All outbox mutations run on the single bounded SQLite worker.
use super::*;
use crate::notifications::{Batch, Delivery, EventContent, JobRef, clean_name, stamp};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;

const SIX_HOURS: i64 = 21_600_000;
const DAY: i64 = 86_400_000;
pub enum Operation {
    Startup {
        fingerprint: Option<[u8; 32]>,
        now: i64,
    },
    Settings,
    Configure {
        revision: i64,
        enabled: bool,
        chat_id: Option<i64>,
        token_present: bool,
        now: i64,
    },
    Test {
        token_present: bool,
        now: i64,
    },
    List {
        before: i64,
        limit: u32,
    },
    Claim {
        now: i64,
        origin: String,
    },
    /// Fence a lost claim reply on this worker. Only the admitted sender that
    /// knows it never entered transport may release this reservation.
    ReconcileUnsubmittedClaim {
        now: i64,
    },
    Finish {
        request_id: i64,
        jobs: Vec<JobRef>,
        outcome: Delivery,
        now: i64,
    },
    Maintain {
        now: i64,
    },
}
#[derive(Serialize)]
pub struct Settings {
    pub revision: i64,
    pub enabled: bool,
    #[serde(serialize_with = "chat_string")]
    pub chat_id: Option<i64>,
    pub destination_revision: i64,
    pub configuration_failed: bool,
    pub cooldown_until_ms: i64,
    pub last_error: Option<String>,
    pub last_success_ms: Option<i64>,
    pub pending: u32,
    pub oldest_pending_ms: Option<i64>,
    pub dropped_alerts: u64,
}
fn chat_string<S: serde::Serializer>(
    value: &Option<i64>,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    value.map(|v| v.to_string()).serialize(serializer)
}
#[derive(Serialize)]
pub struct Notice {
    pub id: i64,
    pub incident_id: Option<i64>,
    pub kind: String,
    pub state: String,
    pub attempts: u8,
    pub next_attempt_at_ms: i64,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
    pub terminal_at_ms: Option<i64>,
    pub message_id: Option<i64>,
    pub error_category: Option<String>,
}
pub enum Value {
    Done,
    Settings(Settings),
    Queued(i64),
    Notices(Vec<Notice>),
    Batch(Option<Batch>),
}
pub(super) struct Command {
    deadline: Instant,
    operation: Operation,
    reply: oneshot::Sender<Result<Value>>,
    #[cfg(test)]
    reply_fault: Option<ClaimReplyFault>,
}
/// Per-client, one-shot post-commit failure injection, absent from production.
#[cfg(test)]
pub(super) struct ClaimReplyFault {
    reconcile: bool,
    committed: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
impl StorageClient {
    pub async fn notifications(&self, operation: Operation) -> Result<Value> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Unavailable);
        }
        let (reply, receive) = oneshot::channel();
        #[cfg(test)]
        let reply_fault = {
            let mut fault = self.claim_reply_fault.lock().map_err(|_| Error::Storage)?;
            if fault.as_ref().is_some_and(|f| {
                if f.reconcile {
                    matches!(operation, Operation::ReconcileUnsubmittedClaim { .. })
                } else {
                    matches!(operation, Operation::Claim { .. })
                }
            }) {
                fault.take()
            } else {
                None
            }
        };
        self.tx
            .try_send(Work::Notifications(Command {
                deadline: Instant::now() + QUERY_TIME,
                operation,
                reply,
                #[cfg(test)]
                reply_fault,
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
            Operation::Settings | Operation::List { .. }
        ) && !health(db, root, command.deadline)?.writable
        {
            return Err(Error::DiskPressure);
        }
        db.progress_handler(100, Some(move || Instant::now() >= command.deadline))
            .map_err(|_| Error::Storage)?;
        execute(db, command.operation, command.deadline)
    })();
    let clear = db
        .progress_handler(0, None::<fn() -> bool>)
        .map_err(|_| Error::Storage);
    #[cfg(test)]
    if let Some(fault) = command.reply_fault {
        assert!(if fault.reconcile {
            matches!(&result, Ok(Value::Done))
        } else {
            matches!(&result, Ok(Value::Batch(Some(_))))
        });
        assert!(clear.is_ok());
        let _ = fault.committed.send(());
        let _ = fault.release.blocking_recv();
        // Deliberately discard the already-committed response after timeout.
        return;
    }
    let _ = command.reply.send(result.and_then(|v| clear.map(|_| v)));
}
fn sql<T>(v: rusqlite::Result<T>) -> Result<T> {
    v.map_err(|_| Error::Storage)
}
fn time(now: i64) -> Result<()> {
    if !(0..=i64::MAX - SIX_HOURS).contains(&now) {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}
fn settings(db: &Connection) -> Result<Settings> {
    sql(db.query_row("SELECT s.revision,s.telegram_enabled,s.telegram_chat_id,s.destination_revision,t.configuration_failed,t.cooldown_until_ms,t.last_error,t.last_success_ms,(SELECT count(*) FROM notification_outbox WHERE state IN ('pending','sending')),(SELECT min(created_at_ms) FROM notification_outbox WHERE state IN ('pending','sending')),h.dropped_alerts FROM settings s JOIN telegram_state t ON t.id=s.id JOIN health_state h ON h.id=s.id WHERE s.id=1",[],|r|Ok(Settings {
        revision:r.get(0)?,enabled:r.get(1)?,chat_id:r.get(2)?,destination_revision:r.get(3)?,configuration_failed:r.get(4)?,cooldown_until_ms:r.get(5)?,last_error:r.get(6)?,last_success_ms:r.get(7)?,pending:r.get(8)?,oldest_pending_ms:r.get(9)?,dropped_alerts:r.get::<_,i64>(10)? as u64
    })))
}
fn capacity(tx: &Transaction<'_>, bytes: usize) -> Result<bool> {
    let (count,used):(i64,i64)=sql(tx.query_row("SELECT count(*),coalesce(sum(length(CAST(payload AS BLOB))),0) FROM notification_outbox WHERE state IN ('pending','sending')",[],|r|Ok((r.get(0)?,r.get(1)?))))?;
    Ok(count < 1000 && used + bytes as i64 <= 2 * 1024 * 1024)
}
fn cancel(tx: &Transaction<'_>, now: i64) -> Result<()> {
    sql(tx.execute("UPDATE notification_outbox SET state='canceled',terminal_at_ms=?1 WHERE state IN ('pending','sending')",[now]))?;
    Ok(())
}
fn audit(tx: &Transaction<'_>, now: i64, outcome: &str) -> Result<()> {
    sql(tx.execute("INSERT INTO audit_events(created_at_ms,category,outcome) VALUES(?1,'telegram_configuration',?2)",params![now,outcome]))?;
    sql(tx.execute("DELETE FROM audit_events WHERE id IN (SELECT id FROM audit_events ORDER BY created_at_ms DESC,id DESC LIMIT 500 OFFSET 5000)",[]))?;
    Ok(())
}
fn maintain(tx: &Transaction<'_>, now: i64) -> Result<()> {
    // Pending is capped at 1000; each maintenance pass changes/deletes <=500.
    sql(tx.execute("UPDATE notification_outbox SET state=CASE WHEN expires_at_ms<=?1 THEN 'expired' ELSE 'failed' END,terminal_at_ms=?1 WHERE id IN (SELECT id FROM notification_outbox WHERE state='pending' AND (expires_at_ms<=?1 OR attempts>=10) LIMIT 500)",[now]))?;
    sql(tx.execute("DELETE FROM notification_outbox WHERE id IN (SELECT id FROM notification_outbox WHERE state IN ('delivered','failed','expired','canceled') AND coalesce(terminal_at_ms,created_at_ms)<?1 ORDER BY coalesce(terminal_at_ms,created_at_ms),id LIMIT 500)",[now-7*DAY]))?;
    let (drops,summarized):(i64,i64)=sql(tx.query_row("SELECT h.dropped_alerts,t.summarized_drops FROM health_state h JOIN telegram_state t ON t.id=h.id WHERE h.id=1",[],|r|Ok((r.get(0)?,r.get(1)?))))?;
    let current = settings(tx)?;
    if current.enabled && drops > summarized {
        let payload =
            serde_json::json!({"count":drops-summarized,"observed_at_ms":now}).to_string();
        if capacity(tx, payload.len())? {
            sql(tx.execute("INSERT INTO notification_outbox(event_identity,kind,destination_revision,payload,state,next_attempt_at_ms,created_at_ms,expires_at_ms) VALUES(?1,'suppression',?2,?3,'pending',?4,?4,?5)",params![format!("suppression:{drops}"),current.destination_revision,payload,now,now+SIX_HOURS]))?;
            sql(tx.execute(
                "UPDATE telegram_state SET summarized_drops=?1 WHERE id=1",
                [drops],
            ))?;
        }
    }
    Ok(())
}
fn execute(db: &mut Connection, operation: Operation, deadline: Instant) -> Result<Value> {
    match operation {
        Operation::Settings => return settings(db).map(Value::Settings),
        Operation::List { before, limit } => {
            if before <= 0 || !(1..=100).contains(&limit) {
                return Err(Error::InvalidInput);
            }
            let mut statement=sql(db.prepare("SELECT id,incident_id,kind,state,attempts,next_attempt_at_ms,created_at_ms,expires_at_ms,terminal_at_ms,message_id,error_category FROM notification_outbox WHERE id<?1 ORDER BY id DESC LIMIT ?2"))?;
            let notices = sql(statement.query_map(params![before, limit], |r| {
                Ok(Notice {
                    id: r.get(0)?,
                    incident_id: r.get(1)?,
                    kind: r.get(2)?,
                    state: r.get(3)?,
                    attempts: r.get(4)?,
                    next_attempt_at_ms: r.get(5)?,
                    created_at_ms: r.get(6)?,
                    expires_at_ms: r.get(7)?,
                    terminal_at_ms: r.get(8)?,
                    message_id: r.get(9)?,
                    error_category: r.get(10)?,
                })
            }))?;
            return sql(notices.collect::<rusqlite::Result<Vec<_>>>()).map(Value::Notices);
        }
        _ => (),
    }
    let tx = sql(db.transaction())?;
    let value = match operation {
        Operation::Startup { fingerprint, now } => {
            time(now)?;
            let old: Option<Vec<u8>> = sql(tx.query_row(
                "SELECT bot_fingerprint FROM telegram_state WHERE id=1",
                [],
                |r| r.get(0),
            ))?;
            if old.as_deref() != fingerprint.as_ref().map(|v| v.as_slice()) {
                cancel(&tx, now)?;
                sql(tx.execute("UPDATE settings SET revision=revision+1,destination_revision=destination_revision+1 WHERE id=1",[]))?;
                audit(&tx, now, "secret_changed_jobs_canceled")?;
                sql(tx.execute("UPDATE telegram_state SET bot_fingerprint=?1,configuration_failed=0,last_error=NULL WHERE id=1",[fingerprint.as_ref().map(|v|v.as_slice())]))?;
                // A new credential never clears a still-binding provider cooldown.
            }
            let changed=sql(tx.execute("UPDATE notification_outbox SET state=CASE WHEN expires_at_ms<=?1 THEN 'expired' WHEN attempts>=10 THEN 'failed' ELSE 'pending' END,next_attempt_at_ms=max(next_attempt_at_ms,?2),terminal_at_ms=CASE WHEN expires_at_ms<=?1 OR attempts>=10 THEN ?1 ELSE NULL END,error_category='interrupted' WHERE state='sending'",params![now,now+60_000]))?;
            if changed > 0 {
                sql(tx.execute(
                    "UPDATE telegram_state SET last_error='interrupted' WHERE id=1",
                    [],
                ))?;
            }
            sql(tx.execute(
                "UPDATE telegram_state SET acknowledged_sequence=request_sequence WHERE id=1",
                [],
            ))?;
            maintain(&tx, now)?;
            Value::Done
        }
        Operation::Configure {
            revision,
            enabled,
            chat_id,
            token_present,
            now,
        } => {
            time(now)?;
            if chat_id == Some(0) || (enabled && (!token_present || chat_id.is_none())) {
                return Err(Error::InvalidInput);
            }
            let current = settings(&tx)?;
            if revision != current.revision {
                return Err(Error::Conflict);
            }
            let changed = current.chat_id != chat_id || current.enabled != enabled;
            if changed {
                cancel(&tx, now)?;
            }
            sql(tx.execute("UPDATE settings SET revision=revision+1,telegram_enabled=?1,telegram_chat_id=?2,destination_revision=destination_revision+?3 WHERE id=1",params![enabled,chat_id,i64::from(changed)]))?;
            sql(tx.execute(
                "UPDATE telegram_state SET configuration_failed=0,last_error=NULL WHERE id=1",
                [],
            ))?;
            audit(
                &tx,
                now,
                if changed {
                    "jobs_canceled"
                } else {
                    "correction_acknowledged"
                },
            )?;
            Value::Settings(settings(&tx)?)
        }
        Operation::Test { token_present, now } => {
            time(now)?;
            let current = settings(&tx)?;
            if !current.enabled
                || !token_present
                || current.chat_id.is_none()
                || current.configuration_failed
            {
                return Err(Error::Conflict);
            }
            let after: i64 = sql(tx.query_row(
                "SELECT test_after_ms FROM telegram_state WHERE id=1",
                [],
                |r| r.get(0),
            ))?;
            if now < after {
                return Err(Error::RateLimited);
            }
            if !capacity(&tx, 2)? {
                return Err(Error::Busy);
            }
            sql(tx.execute("INSERT INTO notification_outbox(event_identity,kind,destination_revision,payload,state,next_attempt_at_ms,created_at_ms,expires_at_ms) VALUES(?1,'test',?2,'{}','pending',?3,?3,?4)",params![format!("test:{}:{now}",current.destination_revision),current.destination_revision,now,now+SIX_HOURS]))?;
            let id = tx.last_insert_rowid();
            sql(tx.execute(
                "UPDATE telegram_state SET test_after_ms=?1 WHERE id=1",
                [now + 60_000],
            ))?;
            Value::Queued(id)
        }
        Operation::Maintain { now } => {
            time(now)?;
            maintain(&tx, now)?;
            Value::Done
        }
        Operation::Claim { now, origin } => {
            time(now)?;
            maintain(&tx, now)?;
            Value::Batch(claim(&tx, now, &origin)?)
        }
        Operation::ReconcileUnsubmittedClaim { now } => {
            time(now)?;
            let outstanding: bool = sql(tx.query_row(
                "SELECT acknowledged_sequence<request_sequence FROM telegram_state WHERE id=1",
                [],
                |r| r.get(0),
            ))?;
            if outstanding {
                // No transport call was made. Refund only the reservation on
                // still-sending rows; terminal/canceled jobs cannot be revived.
                // Existing expiry, destination revisions and ordering stay put.
                sql(tx.execute("UPDATE notification_outbox SET state=CASE WHEN expires_at_ms<=?1 THEN 'expired' ELSE 'pending' END,attempts=max(attempts-1,0),terminal_at_ms=CASE WHEN expires_at_ms<=?1 THEN ?1 ELSE NULL END,error_category='interrupted' WHERE state='sending'",[now]))?;
                sql(tx.execute("UPDATE telegram_state SET acknowledged_sequence=request_sequence,last_error='interrupted' WHERE id=1",[]))?;
            }
            Value::Done
        }
        Operation::Finish {
            request_id,
            jobs,
            outcome,
            now,
        } => {
            time(now)?;
            finish(&tx, request_id, &jobs, outcome, now)?;
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

struct Event {
    id: i64,
    incident: Option<i64>,
    kind: String,
    payload: String,
    attempts: u8,
    created: i64,
    confirmed: Option<i64>,
    ended: Option<i64>,
    gap: bool,
    name: Option<String>,
}
const EVENT_SELECT: &str = "SELECT o.id,o.incident_id,o.kind,o.payload,o.attempts,o.created_at_ms,i.confirmed_at_ms,i.ended_at_ms,coalesce(i.observation_gap,0),m.name FROM notification_outbox o LEFT JOIN incidents i ON i.id=o.incident_id LEFT JOIN monitors m ON m.id=i.monitor_id";
fn row_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<Event> {
    Ok(Event {
        id: r.get(0)?,
        incident: r.get(1)?,
        kind: r.get(2)?,
        payload: r.get(3)?,
        attempts: r.get(4)?,
        created: r.get(5)?,
        confirmed: r.get(6)?,
        ended: r.get(7)?,
        gap: r.get(8)?,
        name: r.get(9)?,
    })
}
fn text(tx: &Transaction<'_>, e: &Event) -> Result<String> {
    match e.kind.as_str() {
        "test" => Ok(format!("Telegram test at {}", stamp(e.created))),
        "suppression" => {
            let payload: serde_json::Value =
                serde_json::from_str(&e.payload).map_err(|_| Error::Storage)?;
            let count = payload
                .get("count")
                .and_then(|v| v.as_u64())
                .ok_or(Error::Storage)?;
            Ok(format!(
                "Alert suppression summary at {}: {count} events were dropped while the outbox was full. Inspect dated incident history.",
                stamp(e.created)
            ))
        }
        "down" | "recovery" => {
            // Migration-era reference-only events fall back to the bounded name;
            // all new events snapshot name/reason without targets or raw errors.
            let content = serde_json::from_str::<EventContent>(&e.payload).ok();
            let name = clean_name(
                content
                    .as_ref()
                    .map(|c| c.name.as_str())
                    .or(e.name.as_deref())
                    .unwrap_or("Monitor"),
            );
            let observed = content
                .as_ref()
                .map(|c| c.observed_at_ms)
                .unwrap_or(e.created);
            let reason = content
                .as_ref()
                .and_then(|c| c.reason)
                .map(|v| serde_json::to_string(&v).unwrap_or_default())
                .unwrap_or_else(|| "\"reachability failure\"".to_owned());
            let incident = e.incident.ok_or(Error::Storage)?;
            let confirmed = e.confirmed.ok_or(Error::Storage)?;
            let down_delivered: bool = sql(tx.query_row(
                "SELECT telegram_down_delivered FROM incidents WHERE id=?1",
                [incident],
                |r| r.get(0),
            ))?;
            if let Some(ended) = e.ended {
                let duration = (ended - confirmed).max(0) / 1000;
                let label = if e.gap {
                    "observed duration; coverage gaps"
                } else {
                    "observed duration"
                };
                let prior = if e.kind == "recovery" && !down_delivered {
                    " Earlier outage alert was not delivered."
                } else {
                    ""
                };
                Ok(format!(
                    "{name} — RESOLVED incident #{incident}: DOWN {} ({}) → RECOVERED {}; {label} {duration}s.{prior}",
                    stamp(confirmed),
                    reason.trim_matches('"'),
                    stamp(ended)
                ))
            } else {
                Ok(format!(
                    "{name} — DOWN at {}; incident #{incident}; reason: {}",
                    stamp(observed),
                    reason.trim_matches('"')
                ))
            }
        }
        _ => Err(Error::Storage),
    }
}
fn claim(tx: &Transaction<'_>, now: i64, origin: &str) -> Result<Option<Batch>> {
    // Validate the dashboard link even for non-web callers. Fixed API origin is
    // unrelated to this link and can never be selected from stored settings.
    let link = url::Url::parse(origin).map_err(|_| Error::InvalidInput)?;
    if origin.len() > 2048
        || !matches!(link.scheme(), "http" | "https")
        || !link.username().is_empty()
        || link.password().is_some()
        || link.query().is_some()
        || link.fragment().is_some()
        || link.path() != "/"
    {
        return Err(Error::InvalidInput);
    }
    let current = settings(tx)?;
    let (paced, cooldown): (i64, i64) = sql(tx.query_row(
        "SELECT paced_until_ms,cooldown_until_ms FROM telegram_state WHERE id=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ))?;
    if !current.enabled
        || current.configuration_failed
        || now < paced.max(cooldown)
        || current.chat_id.is_none()
    {
        return Ok(None);
    }
    // No second batch while one admitted request/ack is outstanding.
    let sending:bool=sql(tx.query_row("SELECT acknowledged_sequence<request_sequence OR EXISTS(SELECT 1 FROM notification_outbox WHERE state='sending') FROM telegram_state WHERE id=1",[],|r|r.get(0)))?;
    if sending {
        return Ok(None);
    }
    let oldest:Option<i64>=sql(tx.query_row("SELECT min(o.created_at_ms) FROM notification_outbox o WHERE o.state='pending' AND o.next_attempt_at_ms<=?1 AND o.expires_at_ms>?1 AND o.attempts<10 AND o.destination_revision=?2 AND (o.kind!='recovery' OR NOT EXISTS(SELECT 1 FROM notification_outbox d WHERE d.incident_id=o.incident_id AND d.kind='down' AND d.state IN ('pending','sending')))",params![now,current.destination_revision],|r|r.get(0)))?;
    let Some(oldest) = oldest else {
        return Ok(None);
    };
    if now < oldest.saturating_add(5000) {
        return Ok(None);
    }
    let query = format!(
        "{EVENT_SELECT} WHERE o.state='pending' AND o.next_attempt_at_ms<=?1 AND o.expires_at_ms>?1 AND o.attempts<10 AND o.destination_revision=?2 AND o.created_at_ms<=?3 AND (o.kind!='recovery' OR NOT EXISTS(SELECT 1 FROM notification_outbox d WHERE d.incident_id=o.incident_id AND d.kind='down' AND d.state IN ('pending','sending'))) ORDER BY o.created_at_ms,o.id LIMIT 16"
    );
    let mut statement = sql(tx.prepare(&query))?;
    let events = sql(statement.query_map(
        params![
            now,
            current.destination_revision,
            oldest.saturating_add(5000)
        ],
        row_event,
    ))?;
    let events = sql(events.collect::<rusqlite::Result<Vec<_>>>())?;
    let mut message = "minimalMonitor alerts\n".to_owned();
    let footer = format!("\nDashboard: {origin}");
    let mut jobs = Vec::with_capacity(16);
    for event in events {
        if jobs.iter().any(|j: &JobRef| j.id == event.id) {
            continue;
        }
        let line = text(tx, &event)?;
        if message.encode_utf16().count()
            + line.encode_utf16().count()
            + footer.encode_utf16().count()
            + 1
            > 3000
        {
            break;
        }
        message.push_str(&line);
        message.push('\n');
        jobs.push(JobRef {
            id: event.id,
            attempt: event.attempts + 1,
        });
        // A resolved down incorporates its pending recovery, even outside the
        // original coalescing window. Both logical events share one receipt.
        if event.kind == "down" && event.ended.is_some() {
            let recovery:Option<(i64,u8)>=sql(tx.query_row("SELECT id,attempts FROM notification_outbox WHERE incident_id=?1 AND kind='recovery' AND state='pending' AND attempts<10 AND expires_at_ms>?2 AND destination_revision=?3",params![event.incident,now,current.destination_revision],|r|Ok((r.get(0)?,r.get(1)?))).optional())?;
            if let Some((id, attempt)) = recovery {
                jobs.push(JobRef {
                    id,
                    attempt: attempt + 1,
                });
            }
        }
        if jobs.len() >= 16 {
            break;
        }
    }
    if jobs.is_empty() {
        return Err(Error::Storage);
    }
    message.push_str(&footer);
    for job in &jobs {
        sql(tx.execute("UPDATE notification_outbox SET state='sending',attempts=?2 WHERE id=?1 AND state='pending'",params![job.id,job.attempt]))?;
    }
    // Also survives interrupted requests; completion extends pacing further.
    sql(tx.execute("UPDATE telegram_state SET paced_until_ms=?1,request_sequence=request_sequence+1,request_destination_revision=?2 WHERE id=1",params![now+3000,current.destination_revision]))?;
    let request_id = sql(tx.query_row(
        "SELECT request_sequence FROM telegram_state WHERE id=1",
        [],
        |r| r.get(0),
    ))?;
    Ok(Some(Batch {
        request_id,
        jobs,
        chat: current.chat_id.ok_or(Error::Storage)?,
        text: message,
    }))
}
fn finish(
    tx: &Transaction<'_>,
    request_id: i64,
    jobs: &[JobRef],
    outcome: Delivery,
    now: i64,
) -> Result<()> {
    if jobs.is_empty() || jobs.len() > 17 || jobs.iter().any(|j| j.attempt == 0 || j.attempt > 10) {
        return Err(Error::InvalidInput);
    }
    let (sequence,acknowledged,destination,current):(i64,i64,i64,i64)=sql(tx.query_row("SELECT t.request_sequence,t.acknowledged_sequence,t.request_destination_revision,s.destination_revision FROM telegram_state t JOIN settings s ON s.id=t.id WHERE t.id=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))))?;
    if request_id != sequence || request_id <= acknowledged {
        return Ok(());
    }
    let (error, message, wait, permanent) = match outcome {
        Delivery::Delivered(id) if id > 0 => (None, Some(id), 0, false),
        Delivery::Delivered(_) => return Err(Error::InvalidInput),
        Delivery::Transient => (Some("transient"), None, 60_000, false),
        Delivery::RateLimited {
            retry_after_seconds,
        } => (
            Some("rate_limited"),
            None,
            retry_after_seconds
                .saturating_mul(1000)
                .max(60_000)
                .min(i64::MAX as u64) as i64,
            false,
        ),
        Delivery::Configuration => (Some("configuration"), None, 0, true),
    };
    // Idempotent acknowledgement: replay cannot extend cooldowns/retries or
    // override a canceled/delivered/newer attempt. A canceled in-flight request
    // still carries its real provider cooldown, so match its attempt as well.
    for job in jobs {
        let row: Option<(String, i64)> = sql(tx
            .query_row(
                "SELECT state,expires_at_ms FROM notification_outbox WHERE id=?1 AND attempts=?2",
                params![job.id, job.attempt],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional())?;
        let Some((state, expires)) = row else {
            continue;
        };
        if state != "sending" && state != "canceled" {
            continue;
        }
        if state == "canceled" {
            continue;
        }
        if message.is_some() {
            sql(tx.execute("UPDATE incidents SET telegram_down_delivered=1 WHERE id=(SELECT incident_id FROM notification_outbox WHERE id=?1 AND kind='down')",[job.id]))?;
        }
        let terminal = message.is_some() || permanent || expires <= now || job.attempt >= 10;
        let state = if message.is_some() {
            "delivered"
        } else if expires <= now {
            "expired"
        } else if permanent || job.attempt >= 10 {
            "failed"
        } else {
            "pending"
        };
        sql(tx.execute("UPDATE notification_outbox SET state=?3,next_attempt_at_ms=?4,terminal_at_ms=?5,message_id=?6,error_category=?7 WHERE id=?1 AND attempts=?2 AND state='sending'",params![job.id,job.attempt,state,now.saturating_add(wait),terminal.then_some(now),message,error]))?;
    }
    {
        let cooldown = if matches!(outcome, Delivery::RateLimited { .. }) {
            now.saturating_add(wait)
        } else {
            0
        };
        sql(tx.execute("UPDATE telegram_state SET acknowledged_sequence=?7,cooldown_until_ms=max(cooldown_until_ms,?1),paced_until_ms=max(paced_until_ms,?2),configuration_failed=max(configuration_failed,?3),last_error=?4,last_success_ms=CASE WHEN ?5 IS NOT NULL THEN ?6 ELSE last_success_ms END WHERE id=1",params![cooldown,now+3000,permanent && destination==current,error,message,now,request_id]))?;
    }
    Ok(())
}

#[cfg(test)]
mod claim_reply_tests {
    use super::*;
    use crate::{
        bounds::WorkBounds,
        notifications::{SendFuture, Sender, SenderHealth, Transport},
        ownership::StateOwner,
        probes::ProbePermit,
    };
    use std::sync::Mutex;

    struct Recording(Arc<Mutex<Vec<i64>>>);
    impl Transport for Recording {
        fn send<'a>(&'a self, chat: i64, _text: &'a str, _permit: ProbePermit) -> SendFuture<'a> {
            Box::pin(async move {
                self.0.lock().unwrap().push(chat);
                Delivery::Delivered(42)
            })
        }
    }
    struct Fixture {
        _dir: tempfile::TempDir,
        storage: Storage,
        db: Connection,
        bounds: WorkBounds,
        health: SenderHealth,
        sends: Arc<Mutex<Vec<i64>>>,
        first: i64,
    }
    impl Fixture {
        async fn new() -> (Self, Sender<Recording>) {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("state");
            let storage = Storage::start(StateOwner::acquire(&root).unwrap())
                .await
                .unwrap();
            storage
                .client()
                .notifications(Operation::Configure {
                    revision: 1,
                    enabled: true,
                    chat_id: Some(-100),
                    token_present: true,
                    now: 0,
                })
                .await
                .unwrap();
            let first = queued(&storage.client(), 0).await;
            let db = Connection::open(root.join("monitor.db")).unwrap();
            let bounds = WorkBounds::default();
            let health = SenderHealth {
                token_present: true,
                ..Default::default()
            };
            let sends = Arc::new(Mutex::new(Vec::new()));
            let sender = Sender::new(
                storage.client(),
                Recording(sends.clone()),
                bounds.clone(),
                health.clone(),
                &url::Url::parse("https://dashboard.invalid/").unwrap(),
            )
            .with_clock(Arc::new(|| Ok(0)));
            (
                Self {
                    _dir: dir,
                    storage,
                    db,
                    bounds,
                    health,
                    sends,
                    first,
                },
                sender,
            )
        }
        fn row(&self, id: i64) -> (String, i64) {
            self.db
                .query_row(
                    "SELECT state,attempts FROM notification_outbox WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }
        fn request(&self) -> (i64, i64) {
            self.db
                .query_row(
                    "SELECT request_sequence,acknowledged_sequence FROM telegram_state WHERE id=1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .unwrap()
        }
        fn degraded(&self) -> bool {
            self.health.persistence_degraded.load(Ordering::Relaxed)
        }
    }
    async fn queued(client: &StorageClient, now: i64) -> i64 {
        match client
            .notifications(Operation::Test {
                token_present: true,
                now,
            })
            .await
            .unwrap()
        {
            Value::Queued(id) => id,
            _ => panic!("queued"),
        }
    }
    async fn withhold_committed_reply(
        client: &StorageClient,
        mut sender: Sender<Recording>,
        now: i64,
        reconcile: bool,
    ) -> (Sender<Recording>, oneshot::Sender<()>) {
        let (committed, observed) = oneshot::channel();
        let (release, hold) = oneshot::channel();
        *client.claim_reply_fault.lock().unwrap() = Some(ClaimReplyFault {
            reconcile,
            committed,
            release: hold,
        });
        let tick = tokio::spawn(async move {
            let result = sender.tick(now).await;
            (sender, result)
        });
        // Commit is real SQLite work. Only after observing it, advance the
        // actual client's Tokio reply timer while its worker withholds reply.
        observed.await.unwrap();
        tokio::time::pause();
        tokio::time::advance(REPLY_TIME + Duration::from_millis(1)).await;
        let (sender, result) = tick.await.unwrap();
        assert!(matches!(result, Err(Error::Deadline)));
        (sender, release)
    }
    #[tokio::test]
    async fn committed_claim_lost_reply_recovers_same_sender_and_keeps_health_degraded_until_fenced()
     {
        let (f, sender) = Fixture::new().await;
        let (mut sender, release) =
            withhold_committed_reply(&f.storage.client(), sender, 5000, false).await;
        assert_eq!(f.row(f.first), ("sending".into(), 1));
        assert_eq!(f.request(), (1, 0));
        assert!(f.degraded());
        assert!(f.sends.lock().unwrap().is_empty());
        assert!(matches!(f.bounds.notification(), Err(Error::Busy)));

        // A reconciliation queued behind the withheld reply also times out.
        // An empty claim must not hide this unresolved interval.
        let retry = tokio::spawn(async move {
            let result = sender.tick(6000).await;
            (sender, result)
        });
        tokio::task::yield_now().await;
        tokio::time::advance(REPLY_TIME + Duration::from_millis(1)).await;
        let (mut sender, result) = retry.await.unwrap();
        assert!(matches!(result, Err(Error::Deadline)));
        assert!(f.degraded());
        assert_eq!(f.request(), (1, 0));
        release.send(()).unwrap();
        tokio::time::resume();
        sender.tick(9000).await.unwrap();
        assert!(!f.degraded());
        assert_eq!(f.row(f.first), ("delivered".into(), 1));
        assert_eq!(f.request(), (2, 2));
        assert_eq!(*f.sends.lock().unwrap(), vec![-100]);
        assert!(f.bounds.notification().is_ok());
        let next = queued(&f.storage.client(), 60000).await;
        sender.tick(65000).await.unwrap();
        assert_eq!(f.row(next), ("delivered".into(), 1));
        assert_eq!(f.request(), (3, 3));
        assert_eq!(*f.sends.lock().unwrap(), vec![-100, -100]);
        let sending: i64 =
            f.db.query_row(
                "SELECT count(*) FROM notification_outbox WHERE state='sending'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(sending, 0);
        f.storage.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn lost_claim_refunds_only_unsent_reservation_and_respects_expiry_and_tenth_attempt() {
        for expired in [false, true] {
            let (f, sender) = Fixture::new().await;
            f.db.execute(
                "UPDATE notification_outbox SET attempts=9 WHERE id=?1",
                [f.first],
            )
            .unwrap();
            let (mut sender, release) =
                withhold_committed_reply(&f.storage.client(), sender, 5000, false).await;
            assert_eq!(f.row(f.first), ("sending".into(), 10));
            release.send(()).unwrap();
            tokio::time::resume();
            sender
                .tick(if expired { SIX_HOURS } else { 9000 })
                .await
                .unwrap();
            if expired {
                assert_eq!(f.row(f.first), ("expired".into(), 9));
                assert_eq!(f.request(), (1, 1));
                assert!(f.sends.lock().unwrap().is_empty());
                let next = queued(&f.storage.client(), SIX_HOURS).await;
                sender.tick(SIX_HOURS + 5000).await.unwrap();
                assert_eq!(f.row(next), ("delivered".into(), 1));
            } else {
                assert_eq!(f.row(f.first), ("delivered".into(), 10));
                sender.tick(100000).await.unwrap();
                assert_eq!(f.request(), (2, 2));
            }
            assert!(!f.degraded());
            assert_eq!(*f.sends.lock().unwrap(), vec![-100]);
            f.storage.shutdown().await.unwrap();
        }
    }
    #[tokio::test]
    async fn lost_reconciliation_reply_is_idempotent_and_does_not_refund_an_attempt_twice() {
        let (f, sender) = Fixture::new().await;
        f.db.execute(
            "UPDATE notification_outbox SET attempts=9 WHERE id=?1",
            [f.first],
        )
        .unwrap();
        let (sender, release) =
            withhold_committed_reply(&f.storage.client(), sender, 5000, false).await;
        release.send(()).unwrap();
        tokio::time::resume();
        let (mut sender, release) =
            withhold_committed_reply(&f.storage.client(), sender, 9000, true).await;
        // The fence committed, but its reply has not confirmed resolution to
        // the sender. Keep health/admission conservative through this interval.
        assert_eq!(f.row(f.first), ("pending".into(), 9));
        assert_eq!(f.request(), (1, 1));
        assert!(f.degraded());
        assert!(matches!(f.bounds.notification(), Err(Error::Busy)));
        assert!(f.sends.lock().unwrap().is_empty());
        release.send(()).unwrap();
        tokio::time::resume();
        sender.tick(9000).await.unwrap();
        assert_eq!(f.row(f.first), ("delivered".into(), 10));
        assert_eq!(f.request(), (2, 2));
        assert!(!f.degraded());
        assert_eq!(*f.sends.lock().unwrap(), vec![-100]);
        f.storage.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn lost_claim_cancellation_does_not_redirect_jobs_or_clear_provider_cooldown() {
        let (f, sender) = Fixture::new().await;
        let b = match f
            .storage
            .client()
            .notifications(Operation::Claim {
                now: 5000,
                origin: "https://dashboard.invalid/".into(),
            })
            .await
            .unwrap()
        {
            Value::Batch(Some(b)) => b,
            _ => panic!("claim"),
        };
        f.storage
            .client()
            .notifications(Operation::Finish {
                request_id: b.request_id,
                jobs: b.jobs,
                outcome: Delivery::RateLimited {
                    retry_after_seconds: 120,
                },
                now: 6000,
            })
            .await
            .unwrap();
        let (mut sender, release) =
            withhold_committed_reply(&f.storage.client(), sender, 126000, false).await;
        release.send(()).unwrap();
        tokio::time::resume();
        f.storage
            .client()
            .notifications(Operation::Configure {
                revision: 2,
                enabled: true,
                chat_id: Some(-200),
                token_present: true,
                now: 125999,
            })
            .await
            .unwrap();
        let next = queued(&f.storage.client(), 125999).await;
        sender.tick(125999).await.unwrap();
        assert!(!f.degraded());
        assert_eq!(f.request(), (2, 2));
        assert_eq!(f.row(f.first).0, "canceled");
        let cooldown: i64 =
            f.db.query_row(
                "SELECT cooldown_until_ms FROM telegram_state WHERE id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cooldown, 126000);
        assert!(f.sends.lock().unwrap().is_empty());
        sender.tick(130999).await.unwrap();
        assert_eq!(f.row(next), ("delivered".into(), 1));
        assert_eq!(f.row(f.first).0, "canceled");
        assert_eq!(*f.sends.lock().unwrap(), vec![-200]);
        f.storage.shutdown().await.unwrap();
    }
}
