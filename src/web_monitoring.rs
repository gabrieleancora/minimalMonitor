//! Protected backend operations; full HTML/theme/chart integration remains WP-05.
use crate::{
    auth::now_ms,
    bounds::RESPONSE_BYTES,
    error::{Error, Result},
    monitoring::{DAY_MS, MonitorConfig},
    scheduler::SchedulerHealth,
    storage::{
        StorageClient,
        monitoring::{Operation, Value, random_jitter},
    },
    web_security::error,
};
use axum::{
    Json, Router,
    extract::{FromRequest, Path, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use serde::{Deserialize, Serialize};
#[derive(Clone)]
struct Backend {
    storage: StorageClient,
    health: SchedulerHealth,
}
type Id = std::result::Result<Path<i64>, axum::extract::rejection::PathRejection>;
fn identifier(value: Id) -> Result<i64> {
    value.map(|Path(id)| id).map_err(|_| Error::InvalidInput)
}
pub(crate) fn router(storage: StorageClient, health: SchedulerHealth) -> Router {
    Router::new()
        .route("/api/v1/summary", get(summary))
        .route("/api/v1/monitors", post(create))
        .route("/api/v1/monitors/{id}", patch(edit).delete(delete))
        .route("/api/v1/monitors/{id}/pause", post(pause))
        .route("/api/v1/monitors/{id}/resume", post(resume))
        .route("/api/v1/monitors/{id}/history", get(history))
        .route("/api/v1/incidents", get(incidents))
        .with_state(Backend { storage, health })
}
pub(crate) fn allows_query(path: &str) -> bool {
    path == "/api/v1/incidents"
        || (path.starts_with("/api/v1/monitors/") && path.ends_with("/history"))
}
fn response<T: Serialize>(status: StatusCode, value: T) -> Response {
    match serde_json::to_vec(&value) {
        Ok(bytes) if bytes.len() <= RESPONSE_BYTES => (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response(),
        _ => error(Error::Unavailable),
    }
}
async fn json<T: serde::de::DeserializeOwned>(request: Request) -> Result<T> {
    Json::<T>::from_request(request, &())
        .await
        .map(|Json(v)| v)
        .map_err(|_| Error::InvalidInput)
}
async fn summary(
    State(app): State<Backend>,
    sender: Option<axum::Extension<crate::notifications::SenderHealth>>,
) -> Response {
    let now = match now_ms() {
        Ok(v) => v,
        Err(e) => return error(e),
    };
    match app.storage.monitors().await {
        Ok(mut monitors) => {
            let (dropped_alerts, persistence_faults) =
                match app.storage.monitoring(Operation::Operational).await {
                    Ok(Value::Operational {
                        dropped_alerts,
                        persistence_faults,
                    }) => (dropped_alerts, persistence_faults),
                    _ => return error(Error::Storage),
                };
            for m in &mut monitors {
                m.state.status = m.state.displayed_status(&m.config, now);
            }
            let recent = match app.storage.monitoring(Operation::Recent).await {
                Ok(Value::Recent(recent)) => recent,
                _ => return error(Error::Storage),
            };
            let telegram = match app
                .storage
                .notifications(crate::storage::notifications::Operation::Settings)
                .await
            {
                Ok(crate::storage::notifications::Value::Settings(s)) => serde_json::json!({
                    "delivery_delayed":s.oldest_pending_ms.is_some_and(|old|now.saturating_sub(old)>15000),
                    "settings":s,"token_present":sender.as_ref().is_some_and(|h|h.token_present),
                    "persistence_degraded":sender.as_ref().is_some_and(|h|h.persistence_degraded.load(std::sync::atomic::Ordering::Relaxed)),
                }),
                _ => return error(Error::Storage),
            };
            response(
                StatusCode::OK,
                serde_json::json!({"monitors":monitors,"recent":recent,"server_time_ms":now,"scheduler_degraded":!app.health.ready(now),"dropped_alerts":dropped_alerts,"persistence_faults":persistence_faults,"persistence_faults_this_process":app.health.missed_persistence.load(std::sync::atomic::Ordering::Relaxed),"telegram":telegram}),
            )
        }
        Err(e) => error(e),
    }
}
async fn create(State(app): State<Backend>, request: Request) -> Response {
    // Missing idempotency key is accepted for clients that never auto-retry.
    let key = if request.headers().get_all("idempotency-key").iter().count() > 1 {
        return error(Error::InvalidInput);
    } else {
        match request.headers().get("idempotency-key") {
            Some(value) => match value.to_str() {
                Ok(value) => Some(value.to_owned()),
                Err(_) => return error(Error::InvalidInput),
            },
            None => None,
        }
    };
    let result = async {
        let config: MonitorConfig = json(request).await?;
        let jitter_ms = random_jitter(&config)?;
        app.storage
            .monitoring(Operation::Create {
                config,
                now: now_ms()?,
                jitter_ms,
                key,
            })
            .await
    }
    .await;
    mutation(result, StatusCode::CREATED)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Edit {
    revision: i64,
    config: MonitorConfig,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delete {
    revision: i64,
    confirm_history_deletion: bool,
}
async fn edit(State(app): State<Backend>, id: Id, request: Request) -> Response {
    let result = async {
        let id = identifier(id)?;
        let edit: Edit = json(request).await?;
        let jitter_ms = random_jitter(&edit.config)?;
        app.storage
            .monitoring(Operation::Edit {
                id,
                revision: edit.revision,
                config: edit.config,
                now: now_ms()?,
                jitter_ms,
            })
            .await
    }
    .await;
    mutation(result, StatusCode::OK)
}
async fn toggle(app: Backend, id: i64, request: Request, enabled: bool) -> Response {
    let result = async {
        let revision: Revision = json(request).await?;
        let operation = if enabled {
            let config = app
                .storage
                .monitors()
                .await?
                .into_iter()
                .find(|m| m.id == id)
                .ok_or(Error::Conflict)?
                .config;
            Operation::Resume {
                id,
                revision: revision.revision,
                now: now_ms()?,
                jitter_ms: random_jitter(&config)?,
            }
        } else {
            Operation::Pause {
                id,
                revision: revision.revision,
                now: now_ms()?,
            }
        };
        app.storage.monitoring(operation).await
    }
    .await;
    mutation(result, StatusCode::OK)
}
async fn pause(State(app): State<Backend>, id: Id, request: Request) -> Response {
    match identifier(id) {
        Ok(id) => toggle(app, id, request, false).await,
        Err(e) => error(e),
    }
}
async fn resume(State(app): State<Backend>, id: Id, request: Request) -> Response {
    match identifier(id) {
        Ok(id) => toggle(app, id, request, true).await,
        Err(e) => error(e),
    }
}
async fn delete(State(app): State<Backend>, id: Id, request: Request) -> Response {
    let result = async {
        let id = identifier(id)?;
        let delete: Delete = json(request).await?;
        if !delete.confirm_history_deletion {
            return Err(Error::InvalidInput);
        }
        app.storage
            .monitoring(Operation::Delete {
                id,
                revision: delete.revision,
                now: now_ms()?,
            })
            .await
    }
    .await;
    mutation(result, StatusCode::ACCEPTED)
}
fn mutation(result: Result<Value>, status: StatusCode) -> Response {
    match result {
        Ok(Value::Monitor(m)) => response(status, m),
        Ok(_) => status.into_response(),
        Err(e) => error(e),
    }
}
fn query(request: &Request, allowed: &[&str]) -> Result<std::collections::HashMap<String, i64>> {
    let mut values = std::collections::HashMap::with_capacity(allowed.len());
    for (key, value) in url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
    {
        if !allowed.contains(&key.as_ref()) || values.contains_key(key.as_ref()) {
            return Err(Error::InvalidInput);
        }
        values.insert(
            key.into_owned(),
            value.parse().map_err(|_| Error::InvalidInput)?,
        );
    }
    Ok(values)
}
// Resolve relative ranges with the same timestamp used for storage validation.
// Network/summary age cannot push a supported window outside retained history.
fn history_window(query: &std::collections::HashMap<String, i64>, now: i64) -> Result<(i64, i64)> {
    if let Some(&range) = query.get("range_ms") {
        if query.len() != 1 || !(1..=90 * DAY_MS).contains(&range) {
            return Err(Error::InvalidInput);
        }
        return Ok((now.checked_sub(range).ok_or(Error::InvalidInput)?, now));
    }
    Ok((
        *query.get("start_ms").unwrap_or(&(now - DAY_MS)),
        *query.get("end_ms").unwrap_or(&now),
    ))
}
async fn history(State(app): State<Backend>, id: Id, request: Request) -> Response {
    let result = async move {
        let id = identifier(id)?;
        let query = query(&request, &["start_ms", "end_ms", "range_ms"])?;
        let now = now_ms()?;
        let (start, end) = history_window(&query, now)?;
        if end > now {
            return Err(Error::InvalidInput);
        }
        let value = app
            .storage
            .monitoring(Operation::History {
                id,
                start,
                end,
                now,
            })
            .await?;
        Ok((value, start, end, start < now - 7 * DAY_MS))
    }
    .await;
    match result {
        Ok((Value::Hours(hours, discontinuity), start, end, partial)) => response(
            StatusCode::OK,
            serde_json::json!({"start_ms":start,"end_ms":end,"granularity":"hourly","partial_boundary_buckets":partial,"coverage_discontinuity":discontinuity,"points":hours}),
        ),
        Ok(_) => error(Error::Storage),
        Err(e) => error(e),
    }
}
async fn incidents(State(app): State<Backend>, request: Request) -> Response {
    let result = async move {
        let query = query(&request, &["before", "limit"])?;
        let limit =
            u32::try_from(*query.get("limit").unwrap_or(&25)).map_err(|_| Error::InvalidInput)?;
        app.storage
            .monitoring(Operation::Incidents {
                before: *query.get("before").unwrap_or(&i64::MAX),
                limit,
            })
            .await
    }
    .await;
    match result {
        Ok(Value::Incidents(incidents)) => response(
            StatusCode::OK,
            serde_json::json!({"next_before":incidents.last().map(|i|i.id),"incidents":incidents}),
        ),
        Ok(_) => error(Error::Storage),
        Err(e) => error(e),
    }
}
