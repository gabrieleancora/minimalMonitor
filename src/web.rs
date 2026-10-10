use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::{TokioIo, TokioTimer};
use serde::Serialize;
use std::{
    convert::Infallible,
    future::Future,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::Semaphore, task::JoinSet};
use tower::ServiceExt;

use crate::{
    bounds::{BODY_BYTES, HEADER_BYTES, WEB_CONNECTIONS},
    error::{Error, Result},
    storage::StorageClient,
};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);
pub const REQUEST_TIME: Duration = Duration::from_secs(5);

#[derive(Clone, Copy)]
pub(crate) struct Peer(pub(crate) SocketAddr);

#[derive(Serialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}
#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
    request_id: u64,
}
#[derive(Serialize)]
struct HealthBody {
    status: &'static str,
}

pub(crate) fn failure(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody {
                code,
                message: "Request could not be completed.",
                request_id: REQUEST_ID.fetch_add(1, Ordering::Relaxed),
            },
        }),
    )
        .into_response()
}

pub(crate) fn health_router(storage: StorageClient) -> Router {
    Router::new()
        .route(
            "/health/live",
            get(|| async { Json(HealthBody { status: "live" }) }),
        )
        .route("/health/ready", get(ready))
        .with_state(storage)
}
#[cfg(test)]
fn router(storage: StorageClient) -> Router {
    health_router(storage)
        .fallback(|| async { failure(StatusCode::NOT_FOUND, "not_found") })
        .layer(middleware::from_fn(boundary))
}

async fn ready(State(storage): State<StorageClient>) -> Response {
    match storage.health().await {
        Ok(health) if health.writable => Json(HealthBody { status: "ready" }).into_response(),
        Ok(_) => failure(StatusCode::SERVICE_UNAVAILABLE, Error::DiskPressure.code()),
        Err(err) => failure(StatusCode::SERVICE_UNAVAILABLE, err.code()),
    }
}

pub(crate) async fn boundary(request: Request, next: Next) -> Response {
    // Uses the accepted socket identity. Never reads X-Forwarded-For or Host as
    // an authorization signal. Nginx must exclude both health paths publicly.
    let local = request
        .extensions()
        .get::<Peer>()
        .is_some_and(|peer| peer.0.ip().is_loopback());
    let length = request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok());
    let mut response = if !local {
        failure(StatusCode::FORBIDDEN, "local_only")
    } else if request.uri().to_string().len() > 2048 || request.uri().authority().is_some() {
        failure(StatusCode::BAD_REQUEST, "invalid_request")
    } else if length.is_some_and(|length| length > BODY_BYTES as u64) {
        failure(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large")
    } else if request.uri().path().starts_with("/health/")
        && (length.is_some_and(|length| length != 0)
            || request.headers().contains_key(header::TRANSFER_ENCODING)
            || request.uri().query().is_some())
    {
        failure(StatusCode::BAD_REQUEST, "invalid_request")
    } else {
        match tokio::time::timeout(REQUEST_TIME, async {
            let (parts, body) = request.into_parts();
            match axum::body::to_bytes(body, BODY_BYTES).await {
                Ok(bytes) => {
                    next.run(Request::from_parts(parts, axum::body::Body::from(bytes)))
                        .await
                }
                Err(_) => failure(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large"),
            }
        })
        .await
        {
            Ok(response) => response,
            Err(_) => failure(StatusCode::SERVICE_UNAVAILABLE, Error::Deadline.code()),
        }
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'"),
    );
    response
}

/// HTTP/1 only, one request per connection. The admission permit covers header
/// parsing, slow clients, handler work and response writes; no unbounded accept
/// tasks or application connection queue. Later private routes reuse this server.
pub async fn serve(
    listener: TcpListener,
    auth: crate::auth::Auth,
    origin: &url::Url,
    mode: crate::config::Mode,
    trusted_proxy: Option<std::net::IpAddr>,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    let routes = crate::web_security::router(auth, origin, mode, trusted_proxy)
        .layer(DefaultBodyLimit::max(BODY_BYTES))
        .layer(middleware::from_fn(boundary));
    serve_routes(listener, routes, shutdown).await
}
async fn serve_routes(
    listener: TcpListener,
    routes: Router,
    shutdown: impl Future<Output = ()>,
) -> Result<()> {
    let slots = Arc::new(Semaphore::new(WEB_CONNECTIONS));
    let mut tasks = JoinSet::new();
    tokio::pin!(shutdown);
    let result = loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break Ok(()),
            completed = tasks.join_next(), if !tasks.is_empty() => {
                if completed.is_some_and(|result| result.is_err()) { break Err(Error::WorkerStopped); }
            }
            accepted = listener.accept() => {
                let (stream, peer) = match accepted { Ok(accepted) => accepted, Err(_) => break Err(Error::Io) };
                let Ok(permit) = slots.clone().try_acquire_owned() else { drop(stream); continue; };
                let routes = routes.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let service = service_fn(move |mut request: hyper::Request<hyper::body::Incoming>| {
                        request.extensions_mut().insert(Peer(peer));
                        let routes = routes.clone();
                        async move { Ok::<_, Infallible>(routes.oneshot(request).await.unwrap_or_else(|never| match never {})) }
                    });
                    let mut builder = http1::Builder::new();
                    builder.keep_alive(false).max_headers(100).max_header_size(HEADER_BYTES).max_buf_size(HEADER_BYTES)
                        .timer(TokioTimer::new()).header_read_timeout(REQUEST_TIME);
                    // max_buf_size bounds the request header parsing buffer; the
                    // deadline also bounds stalled response writes.
                    let _ = tokio::time::timeout(REQUEST_TIME, builder.serve_connection(TokioIo::new(stream), service)).await;
                });
            }
        }
    };
    drop(listener);
    // Every connection already has a five-second deadline. Abort on unexpected
    // delay, then await destruction before closing the DB worker.
    if tokio::time::timeout(REQUEST_TIME, async {
        while tasks.join_next().await.is_some() {}
    })
    .await
    .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ownership::StateOwner, storage::Storage};
    use axum::body::{Body, to_bytes};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn health_requires_socket_identity_and_never_exposes_private_routes() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::start(StateOwner::acquire(&temp.path().join("state")).unwrap())
            .await
            .unwrap();
        let app = router(storage.client());
        for (uri, peer, status) in [
            ("/health/live", None, StatusCode::FORBIDDEN),
            ("/health/ready", Some("192.0.2.1:42"), StatusCode::FORBIDDEN),
            ("/health/live", Some("127.0.0.1:42"), StatusCode::OK),
            ("/health/ready", Some("[::1]:42"), StatusCode::OK),
            (
                "/api/v1/summary",
                Some("127.0.0.1:42"),
                StatusCode::NOT_FOUND,
            ),
            ("/settings", Some("127.0.0.1:42"), StatusCode::NOT_FOUND),
            (
                "/health/live?token=sensitive",
                Some("127.0.0.1:42"),
                StatusCode::BAD_REQUEST,
            ),
        ] {
            let mut request = Request::builder()
                .uri(uri)
                .header("X-Forwarded-For", "127.0.0.1")
                .body(Body::empty())
                .unwrap();
            if let Some(peer) = peer {
                request.extensions_mut().insert(Peer(peer.parse().unwrap()));
            }
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
            let body = to_bytes(response.into_body(), 256).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains("sensitive"));
        }
        let client = storage.client();
        storage.shutdown().await.unwrap();
        let mut request = Request::builder()
            .uri("/health/ready")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(Peer("127.0.0.1:42".parse().unwrap()));
        assert_eq!(
            router(client).oneshot(request).await.unwrap().status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
    }

    #[tokio::test]
    async fn parser_rejects_oversized_headers_and_server_closes_slow_clients() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::start(StateOwner::acquire(&temp.path().join("state")).unwrap())
            .await
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (stop, shutdown) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(serve_routes(listener, router(storage.client()), async {
            let _ = shutdown.await;
        }));
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let request = format!(
            "GET /health/live HTTP/1.1\r\nHost: localhost\r\nX-Large: {}\r\n\r\n",
            "a".repeat(HEADER_BYTES)
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut reply = Vec::new();
        let _ = stream.read_to_end(&mut reply).await;
        assert!(!String::from_utf8_lossy(&reply).contains("200 OK"));
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        stream
            .write_all(b"GET /health/live HTTP/1.1\r\n")
            .await
            .unwrap();
        let mut reply = Vec::new();
        tokio::time::timeout(
            REQUEST_TIME + Duration::from_secs(2),
            stream.read_to_end(&mut reply),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(!String::from_utf8_lossy(&reply).contains("200 OK"));
        stop.send(()).unwrap();
        server.await.unwrap().unwrap();
        storage.shutdown().await.unwrap();
    }
}
