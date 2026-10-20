use crate::{
    auth::now_ms,
    bounds::RESPONSE_BYTES,
    error::{Error, Result},
    notifications::SenderHealth,
    storage::{
        StorageClient,
        notifications::{Operation, Value},
    },
    web_security::error,
};
use axum::{
    Json, Router,
    extract::{FromRequest, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::atomic::Ordering;
#[derive(Clone)]
struct Backend {
    storage: StorageClient,
    health: SenderHealth,
}
pub(crate) fn router(storage: StorageClient, health: SenderHealth) -> Router {
    Router::new()
        .route("/api/v1/settings/telegram", get(settings).patch(configure))
        .route("/api/v1/settings/telegram/test", post(test))
        .route("/api/v1/notifications", get(list))
        .with_state(Backend { storage, health })
}
fn response(status: StatusCode, value: serde_json::Value) -> Response {
    match serde_json::to_vec(&value) {
        Ok(bytes) if bytes.len() <= RESPONSE_BYTES => (
            status,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            bytes,
        )
            .into_response(),
        _ => error(Error::Storage),
    }
}
async fn settings(State(app): State<Backend>) -> Response {
    match app.storage.notifications(Operation::Settings).await {
        Ok(Value::Settings(settings)) => response(
            StatusCode::OK,
            serde_json::json!({"settings":settings,"token_present":app.health.token_present,"persistence_degraded":app.health.persistence_degraded.load(Ordering::Relaxed)}),
        ),
        Err(e) => error(e),
        _ => error(Error::Storage),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configure {
    revision: i64,
    enabled: bool,
    chat_id: Option<ChatId>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ChatId {
    Integer(i64),
    Decimal(String),
}
impl ChatId {
    fn signed(self) -> Result<i64> {
        match self {
            Self::Integer(id) => Ok(id),
            Self::Decimal(text) => {
                let id: i64 = text.parse().map_err(|_| Error::InvalidInput)?;
                if id.to_string() != text {
                    return Err(Error::InvalidInput);
                }
                Ok(id)
            }
        }
    }
}
async fn configure(State(app): State<Backend>, request: Request) -> Response {
    let result = async {
        let Json(input) = Json::<Configure>::from_request(request, &())
            .await
            .map_err(|_| Error::InvalidInput)?;
        app.storage
            .notifications(Operation::Configure {
                revision: input.revision,
                enabled: input.enabled,
                chat_id: input.chat_id.map(ChatId::signed).transpose()?,
                token_present: app.health.token_present,
                now: now_ms()?,
            })
            .await
    }
    .await;
    match result {
        Ok(Value::Settings(settings)) => response(
            StatusCode::OK,
            serde_json::json!({"settings":settings,"token_present":app.health.token_present}),
        ),
        Err(e) => error(e),
        _ => error(Error::Storage),
    }
}
async fn test(State(app): State<Backend>, request: Request) -> Response {
    // Empty body only; no token, alternate destination or endpoint is accepted.
    let result = async {
        let body = axum::body::to_bytes(request.into_body(), 1)
            .await
            .map_err(|_| Error::InvalidInput)?;
        if !body.is_empty() {
            return Err(Error::InvalidInput);
        }
        app.storage
            .notifications(Operation::Test {
                token_present: app.health.token_present,
                now: now_ms()?,
            })
            .await
    }
    .await;
    match result {
        Ok(Value::Queued(id)) => response(
            StatusCode::ACCEPTED,
            serde_json::json!({"id":id,"state":"pending"}),
        ),
        Err(Error::RateLimited) => {
            let mut response = error(Error::RateLimited);
            response.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from_static("60"),
            );
            response
        }
        Err(e) => error(e),
        _ => error(Error::Storage),
    }
}
fn pagination(request: &Request) -> Result<(i64, u32)> {
    let mut before = None;
    let mut limit = None;
    if let Some(query) = request.uri().query() {
        if query.len() > 100 {
            return Err(Error::InvalidInput);
        }
        for part in query.split('&') {
            let (key, value) = part.split_once('=').ok_or(Error::InvalidInput)?;
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::InvalidInput);
            }
            match key {
                "before" if before.is_none() => {
                    before = Some(value.parse().map_err(|_| Error::InvalidInput)?)
                }
                "limit" if limit.is_none() => {
                    limit = Some(value.parse().map_err(|_| Error::InvalidInput)?)
                }
                _ => return Err(Error::InvalidInput),
            }
        }
    }
    Ok((before.unwrap_or(i64::MAX), limit.unwrap_or(25)))
}
async fn list(State(app): State<Backend>, request: Request) -> Response {
    let page = pagination(&request);
    drop(request);
    let result = async {
        let (before, limit) = page?;
        app.storage
            .notifications(Operation::List { before, limit })
            .await
    }
    .await;
    match result {
        Ok(Value::Notices(notices)) => {
            response(StatusCode::OK, serde_json::json!({"notifications":notices}))
        }
        Err(e) => error(e),
        _ => error(Error::Storage),
    }
}
