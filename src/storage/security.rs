//! Every security mutation is admitted and committed on the sole SQLite worker.
use super::*;
use crate::auth::{ABSOLUTE_MS, IDLE_MS};
use rusqlite::{OptionalExtension, Transaction, params};

pub struct Account {
    pub username: String,
    pub hash: String,
    pub generation: i64,
}
pub enum Operation {
    Account,
    Local {
        user: String,
        hash: String,
        reset: bool,
        now: i64,
    },
    Login {
        generation: i64,
        token: [u8; 32],
        csrf: [u8; 32],
        now: i64,
    },
    Session {
        token: [u8; 32],
        now: i64,
    },
    Logout {
        token: [u8; 32],
        now: i64,
    },
    Change {
        token: [u8; 32],
        generation: i64,
        hash: String,
        now: i64,
    },
}
pub enum Value {
    Account(Option<Account>),
    Session([u8; 32]),
    Done,
}
pub(super) struct Command {
    deadline: Instant,
    operation: Operation,
    reply: oneshot::Sender<Result<Value>>,
}
impl StorageClient {
    pub async fn security(&self, operation: Operation) -> Result<Value> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(Error::Unavailable);
        }
        let (reply, receive) = oneshot::channel();
        self.tx
            .try_send(Work::Security(Command {
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
}
pub(super) fn dispatch(db: &mut Connection, root: &Path, command: Command) {
    if command.reply.is_closed() {
        return;
    }
    let result = (|| {
        if Instant::now() >= command.deadline {
            return Err(Error::Deadline);
        }
        // Reads that need no activity update can proceed under write pressure.
        if !matches!(command.operation, Operation::Account)
            && !health(db, root, command.deadline)?.writable
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
fn account(db: &Connection) -> Result<Option<Account>> {
    db.query_row(
        "SELECT username,password_hash,credential_generation FROM admin WHERE id=1",
        [],
        |r| {
            Ok(Account {
                username: r.get(0)?,
                hash: r.get(1)?,
                generation: r.get(2)?,
            })
        },
    )
    .optional()
    .map_err(|_| Error::Storage)
}
fn active(tx: &Transaction<'_>, token: &[u8; 32], now: i64) -> Result<[u8; 32]> {
    let row: Option<(Vec<u8>,i64,i64,i64)>=tx.query_row("SELECT csrf_binding,created_at_ms,last_used_at_ms,expires_at_ms FROM sessions WHERE token_hash=?1",[token.as_slice()],|r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_| Error::Storage)?;
    let (csrf, created, last, expires) = row.ok_or(Error::Unauthorized)?;
    if now < created
        || now < last
        || now >= expires
        || now - last >= IDLE_MS
        || expires - created != ABSOLUTE_MS
    {
        return Err(Error::Unauthorized);
    }
    csrf.try_into().map_err(|_| Error::Storage)
}
fn audit(tx: &Transaction<'_>, now: i64, category: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM audit_events WHERE created_at_ms < ?1",
        [now - 30 * 24 * 60 * 60 * 1000],
    )
    .map_err(|_| Error::Storage)?;
    tx.execute("DELETE FROM audit_events WHERE id IN (SELECT id FROM audit_events ORDER BY id DESC LIMIT -1 OFFSET 4999)",[]).map_err(|_| Error::Storage)?;
    tx.execute("INSERT INTO audit_events(created_at_ms,category,entity_id,outcome) VALUES(?1,?2,1,'success')",params![now,category]).map_err(|_| Error::Storage)?;
    Ok(())
}
fn execute(db: &mut Connection, operation: Operation, deadline: Instant) -> Result<Value> {
    if matches!(operation, Operation::Account) {
        return Ok(Value::Account(account(db)?));
    }
    let tx = db.transaction().map_err(|_| Error::Storage)?;
    let result = match operation {
        Operation::Account => return Err(Error::Storage),
        Operation::Local {
            user,
            hash,
            reset,
            now,
        } => {
            let existing = account(&tx)?;
            if existing.is_some() != reset {
                return Err(Error::Conflict);
            }
            tx.execute("DELETE FROM sessions", [])
                .map_err(|_| Error::Storage)?;
            if reset {
                tx.execute("UPDATE admin SET username=?1,password_hash=?2,credential_generation=credential_generation+1,updated_at_ms=?3 WHERE id=1",params![user,hash,now]).map_err(|_| Error::Storage)?;
            } else {
                tx.execute("INSERT INTO admin(id,username,password_hash,credential_generation,created_at_ms,updated_at_ms) VALUES(1,?1,?2,1,?3,?3)",params![user,hash,now]).map_err(|_| Error::Storage)?;
            }
            audit(
                &tx,
                now,
                if reset {
                    "local_reset"
                } else {
                    "local_bootstrap"
                },
            )?;
            Value::Done
        }
        Operation::Login {
            generation,
            token,
            csrf,
            now,
        } => {
            if !account(&tx)?.is_some_and(|a| a.generation == generation) {
                return Err(Error::Unauthorized);
            }
            tx.execute("DELETE FROM sessions WHERE expires_at_ms<=?1 OR last_used_at_ms<=?2 OR created_at_ms>?1 OR last_used_at_ms>?1",params![now,now-IDLE_MS]).map_err(|_| Error::Storage)?;
            tx.execute("DELETE FROM sessions WHERE token_hash IN (SELECT token_hash FROM sessions ORDER BY created_at_ms DESC,rowid DESC LIMIT -1 OFFSET 4)",[]).map_err(|_| Error::Storage)?;
            tx.execute("INSERT INTO sessions(token_hash,admin_id,csrf_binding,created_at_ms,last_used_at_ms,expires_at_ms) VALUES(?1,1,?2,?3,?3,?4)",params![token.as_slice(),csrf.as_slice(),now,now+ABSOLUTE_MS]).map_err(|_| Error::Storage)?;
            audit(&tx, now, "login")?;
            Value::Done
        }
        Operation::Session { token, now } => {
            let csrf = active(&tx, &token, now)?;
            // Polls at 15 s cause at most one activity write per session/minute.
            tx.execute("UPDATE sessions SET last_used_at_ms=?1 WHERE token_hash=?2 AND last_used_at_ms<=?3",params![now,token.as_slice(),now-60_000]).map_err(|_| Error::Storage)?;
            Value::Session(csrf)
        }
        Operation::Logout { token, now } => {
            active(&tx, &token, now)?;
            tx.execute(
                "DELETE FROM sessions WHERE token_hash=?1",
                [token.as_slice()],
            )
            .map_err(|_| Error::Storage)?;
            audit(&tx, now, "logout")?;
            Value::Done
        }
        Operation::Change {
            token,
            generation,
            hash,
            now,
        } => {
            active(&tx, &token, now)?;
            if !account(&tx)?.is_some_and(|a| a.generation == generation) {
                return Err(Error::Unauthorized);
            }
            tx.execute("UPDATE admin SET password_hash=?1,credential_generation=credential_generation+1,updated_at_ms=?2 WHERE id=1",params![hash,now]).map_err(|_| Error::Storage)?;
            tx.execute("DELETE FROM sessions", [])
                .map_err(|_| Error::Storage)?;
            audit(&tx, now, "password_change")?;
            Value::Done
        }
    };
    if Instant::now() >= deadline {
        return Err(Error::Deadline);
    }
    tx.commit().map_err(|_| Error::Storage)?;
    Ok(result)
}
