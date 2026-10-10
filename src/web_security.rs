use crate::{
    auth::{Auth, hex, now_ms, unhex},
    config::Mode,
    error::Error,
    web::{Peer, failure},
};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, sync::Arc};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

struct App {
    auth: Auth,
    origin: String,
    host: String,
    secure: bool,
    trusted_proxy: Option<IpAddr>,
}
#[derive(Clone, Copy)]
struct ClientIp(IpAddr);
#[derive(Clone)]
struct Identity {
    token: String,
    csrf: [u8; 32],
}
pub fn router(auth: Auth, origin: &url::Url, mode: Mode, trusted_proxy: Option<IpAddr>) -> Router {
    let host = match origin.port() {
        Some(port) => format!("{}:{port}", origin.host_str().unwrap_or_default()),
        None => origin.host_str().unwrap_or_default().to_owned(),
    };
    let app = Arc::new(App {
        auth: auth.clone(),
        origin: origin.origin().ascii_serialization(),
        host,
        secure: mode == Mode::Production || origin.scheme() == "https",
        trusted_proxy,
    });
    Router::new()
        .route("/login", get(login_page))
        .route(
            "/assets/login.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../assets/login.js"),
                )
            }),
        )
        .route(
            "/auth/login",
            post(login).layer(axum::extract::DefaultBodyLimit::max(1024)),
        )
        .route("/auth/logout", post(logout))
        .route("/api/v1/account/session", get(session))
        .route(
            "/api/v1/account/password",
            post(change).layer(axum::extract::DefaultBodyLimit::max(1024)),
        )
        .fallback(|| async { failure(StatusCode::NOT_IMPLEMENTED, "feature_pending") })
        .method_not_allowed_fallback(|| async {
            failure(StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed")
        })
        .with_state(app.clone())
        .merge(crate::web::health_router(auth.storage))
        .layer(middleware::from_fn_with_state(app, guard))
}
fn names(secure: bool) -> (&'static str, &'static str) {
    if secure {
        ("__Host-mm-session", "__Host-mm-preauth")
    } else {
        ("mm-dev-session", "mm-dev-preauth")
    }
}
fn cookie(request: &Request, name: &str) -> Option<String> {
    let mut found = None;
    for value in request.headers().get_all(header::COOKIE) {
        for pair in value.to_str().ok()?.split(';') {
            let (key, value) = pair.trim().split_once('=')?;
            if key == name {
                if found.is_some() {
                    return None;
                }
                found = Some(value.to_owned());
            }
        }
    }
    found
}
fn value<'a>(request: &'a Request, name: &str) -> Option<&'a str> {
    if request.headers().get_all(name).iter().count() != 1 {
        return None;
    }
    request.headers().get(name)?.to_str().ok()
}
fn client_ip(request: &Request, peer: IpAddr, trusted: Option<IpAddr>) -> Result<IpAddr, Error> {
    let ip = if trusted == Some(peer) {
        value(request, "x-real-ip")
            .ok_or(Error::Forbidden)?
            .parse::<IpAddr>()
            .map_err(|_| Error::Forbidden)?
    } else {
        peer
    };
    Ok(crate::destination::normalize(ip))
}
fn set_cookie(response: &mut Response, name: &str, token: &str, secure: bool, age: u32) {
    let text = format!(
        "{name}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={age}{}",
        if secure { "; Secure" } else { "" }
    );
    if let Ok(header) = HeaderValue::from_str(&text) {
        response.headers_mut().append(header::SET_COOKIE, header);
    }
}
fn error(err: Error) -> Response {
    let status = match err {
        Error::Unauthorized => StatusCode::UNAUTHORIZED,
        Error::Forbidden => StatusCode::FORBIDDEN,
        Error::InvalidInput => StatusCode::UNPROCESSABLE_ENTITY,
        Error::Conflict => StatusCode::CONFLICT,
        Error::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    let mut response = failure(
        status,
        if err == Error::Unauthorized {
            "invalid_credentials"
        } else {
            err.code()
        },
    );
    if matches!(
        status,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
    ) {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("10"));
    }
    response
}
async fn guard(State(app): State<Arc<App>>, mut request: Request, next: Next) -> Response {
    let path = request.uri().path().to_owned();
    let health = matches!(path.as_str(), "/health/live" | "/health/ready");
    let public = matches!(path.as_str(), "/login" | "/auth/login" | "/assets/login.js");
    let mutation = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let decision = async {
        let peer = request
            .extensions()
            .get::<Peer>()
            .ok_or(Error::Forbidden)?
            .0;
        if !peer.ip().is_loopback() {
            return Err(Error::Forbidden);
        }
        if !health && value(&request, "host") != Some(app.host.as_str()) {
            return Err(Error::Forbidden);
        }
        if !health {
            let ip = client_ip(&request, peer.ip(), app.trusted_proxy)?;
            request.extensions_mut().insert(ClientIp(ip));
        }
        if request.uri().query().is_some() {
            return Err(Error::InvalidInput);
        }
        if mutation && !health && value(&request, "origin") != Some(app.origin.as_str()) {
            return Err(Error::Forbidden);
        }
        if mutation
            && !health
            && value(&request, "content-type")
                .is_none_or(|v| v.split(';').next() != Some("application/json"))
        {
            return Err(Error::InvalidInput);
        }
        if public {
            if path == "/auth/login" && request.method() == Method::POST {
                app.auth.check_preauth(
                    &cookie(&request, names(app.secure).1).ok_or(Error::Forbidden)?,
                    value(&request, "x-csrf-token").ok_or(Error::Forbidden)?,
                    now_ms()?,
                )?;
            }
        } else if !health {
            let token = cookie(&request, names(app.secure).0).ok_or(Error::Unauthorized)?;
            let csrf = app.auth.session(&token).await?;
            if mutation {
                let supplied =
                    unhex::<32>(value(&request, "x-csrf-token").ok_or(Error::Forbidden)?)?;
                if !bool::from(csrf.ct_eq(&supplied)) {
                    return Err(Error::Forbidden);
                }
                app.auth.limits.mutation()?;
            }
            request.extensions_mut().insert(Identity { token, csrf });
        }
        Ok::<_, Error>(())
    }
    .await;
    match decision {
        Ok(()) => next.run(request).await,
        Err(Error::Unauthorized)
            if !request.uri().path().starts_with("/api/") && request.method() == Method::GET =>
        {
            Redirect::to("/login").into_response()
        }
        Err(err) => error(err),
    }
}
async fn login_page(State(app): State<Arc<App>>) -> Response {
    let token = match now_ms().and_then(|now| app.auth.preauth(now)) {
        Ok(token) => token,
        Err(err) => return error(err),
    };
    let mut response=Html(format!("<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><title>minimalMonitor login</title><h1>Sign in</h1><form id=\"login\"><input name=\"username\" autocomplete=\"username\" aria-label=\"Username\" maxlength=\"64\" required><input name=\"password\" type=\"password\" autocomplete=\"current-password\" aria-label=\"Password\" maxlength=\"128\" required><input name=\"csrf\" type=\"hidden\" value=\"{token}\"><button>Sign in</button></form><p id=\"result\" role=\"status\"></p><script src=\"/assets/login.js\" defer></script></html>")).into_response();
    set_cookie(&mut response, names(app.secure).1, &token, app.secure, 300);
    response
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: Zeroizing<String>,
}
#[derive(Serialize)]
struct SessionBody {
    csrf_token: String,
}
async fn login(State(app): State<Arc<App>>, request: Request) -> Response {
    let peer = request
        .extensions()
        .get::<ClientIp>()
        .map(|p| p.0)
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
    let Json(credentials) = match Json::<Credentials>::from_request(request, &()).await {
        Ok(json) => json,
        Err(rejection) => return json_error(rejection),
    };
    match app
        .auth
        .login(credentials.username, credentials.password, peer)
        .await
    {
        Ok(session) => {
            let mut response = Json(SessionBody {
                csrf_token: session.csrf,
            })
            .into_response();
            set_cookie(
                &mut response,
                names(app.secure).0,
                &session.token,
                app.secure,
                86400,
            );
            set_cookie(&mut response, names(app.secure).1, "", app.secure, 0);
            response
        }
        Err(err) => error(err),
    }
}
use axum::extract::FromRequest;
fn json_error(rejection: axum::extract::rejection::JsonRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        failure(StatusCode::PAYLOAD_TOO_LARGE, "body_too_large")
    } else {
        error(Error::InvalidInput)
    }
}
async fn session(request: Request) -> Response {
    match request.extensions().get::<Identity>() {
        Some(identity) => Json(SessionBody {
            csrf_token: hex(&identity.csrf),
        })
        .into_response(),
        None => error(Error::Unauthorized),
    }
}
async fn logout(State(app): State<Arc<App>>, request: Request) -> Response {
    let Some(identity) = request.extensions().get::<Identity>() else {
        return error(Error::Unauthorized);
    };
    match app.auth.logout(&identity.token).await {
        Ok(()) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            set_cookie(&mut response, names(app.secure).0, "", app.secure, 0);
            response
        }
        Err(err) => error(err),
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    current_password: Zeroizing<String>,
    new_password: Zeroizing<String>,
}
async fn change(State(app): State<Arc<App>>, request: Request) -> Response {
    let Some(identity) = request.extensions().get::<Identity>().cloned() else {
        return error(Error::Unauthorized);
    };
    let Json(change) = match Json::<Change>::from_request(request, &()).await {
        Ok(json) => json,
        Err(rejection) => return json_error(rejection),
    };
    match app
        .auth
        .change(
            &identity.token,
            change.current_password,
            change.new_password,
        )
        .await
    {
        Ok(()) => {
            let mut response = StatusCode::NO_CONTENT.into_response();
            set_cookie(&mut response, names(app.secure).0, "", app.secure, 0);
            response
        }
        Err(err) => error(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bounds::{BODY_BYTES, WorkBounds},
        ownership::StateOwner,
        storage::Storage,
    };
    use axum::body::{Body, to_bytes};
    use tower::ServiceExt;
    #[test]
    fn identity_headers_require_explicit_matching_peer_and_one_numeric_value() {
        let peer = "127.0.0.1".parse().unwrap();
        let remote: IpAddr = "192.0.2.1".parse().unwrap();
        let mut r = request("/login", "GET", None, None, "");
        r.headers_mut()
            .insert("x-real-ip", HeaderValue::from_static("192.0.2.1"));
        r.headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_static("8.8.8.8"));
        assert_eq!(client_ip(&r, peer, None).unwrap(), peer);
        assert_eq!(client_ip(&r, peer, Some(peer)).unwrap(), remote);
        assert_eq!(
            client_ip(&r, "::1".parse().unwrap(), Some(peer)).unwrap(),
            "::1".parse::<IpAddr>().unwrap()
        );
        r.headers_mut()
            .append("x-real-ip", HeaderValue::from_static("8.8.8.8"));
        assert_eq!(client_ip(&r, peer, Some(peer)), Err(Error::Forbidden));
        r.headers_mut().remove("x-real-ip");
        assert_eq!(client_ip(&r, peer, Some(peer)), Err(Error::Forbidden));
        r.headers_mut()
            .insert("x-real-ip", HeaderValue::from_static("192.0.2.1, 8.8.8.8"));
        assert_eq!(client_ip(&r, peer, Some(peer)), Err(Error::Forbidden));
        r.headers_mut()
            .insert("x-real-ip", HeaderValue::from_static("::ffff:192.0.2.1"));
        assert_eq!(client_ip(&r, peer, Some(peer)).unwrap(), remote);
    }
    async fn fixture() -> (tempfile::TempDir, Storage, Auth, Router) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::start(StateOwner::acquire(&dir.path().join("state")).unwrap())
            .await
            .unwrap();
        let auth = Auth::new(storage.client(), WorkBounds::default())
            .await
            .unwrap();
        let app = router(
            auth.clone(),
            &url::Url::parse("https://monitor.invalid").unwrap(),
            Mode::Production,
            None,
        )
        .layer(middleware::from_fn(crate::web::boundary));
        (dir, storage, auth, app)
    }
    fn request(
        path: &str,
        method: &str,
        cookies: Option<&str>,
        csrf: Option<&str>,
        body: &str,
    ) -> Request {
        let mut builder = Request::builder()
            .uri(path)
            .method(method)
            .header("host", "monitor.invalid")
            .header("origin", "https://monitor.invalid")
            .header("content-type", "application/json");
        if let Some(cookie) = cookies {
            builder = builder.header("cookie", cookie);
        }
        if let Some(csrf) = csrf {
            builder = builder.header("x-csrf-token", csrf);
        }
        let mut r = builder.body(Body::from(body.to_owned())).unwrap();
        r.extensions_mut()
            .insert(Peer("127.0.0.1:42".parse().unwrap()));
        r
    }
    async fn body(response: Response) -> String {
        String::from_utf8(
            to_bytes(response.into_body(), 65536)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap()
    }
    #[tokio::test]
    async fn all_private_paths_and_methods_require_session_and_browser_redirect() {
        let (_dir, storage, _auth, app) = fixture().await;
        for (path, method) in [
            ("/api/v1/summary", "GET"),
            ("/api/v1/monitors", "POST"),
            ("/api/v1/monitors/1", "PATCH"),
            ("/api/v1/monitors/1", "DELETE"),
            ("/api/v1/monitors/1/pause", "POST"),
            ("/api/v1/monitors/1/resume", "POST"),
            ("/api/v1/monitors/1/history", "GET"),
            ("/api/v1/incidents", "GET"),
            ("/api/v1/settings/telegram", "GET"),
            ("/api/v1/settings/telegram", "PATCH"),
            ("/api/v1/settings/telegram/test", "POST"),
            ("/api/v1/notifications", "GET"),
            ("/api/v1/account/session", "GET"),
            ("/api/v1/account/password", "POST"),
            ("/auth/logout", "POST"),
            ("/api/v1/unrecognized", "GET"),
        ] {
            let mut req = request(path, method, None, None, "{}");
            req.headers_mut()
                .insert("x-forwarded-for", HeaderValue::from_static("10.1.1.1"));
            let response = app.clone().oneshot(req).await.unwrap();
            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {path}"
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        }
        for path in ["/", "/settings", "/monitors/1"] {
            let response = app
                .clone()
                .oneshot(request(path, "GET", None, None, ""))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()[header::LOCATION], "/login");
        }
        storage.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn canonical_origin_proxy_spoof_csrf_bounds_and_safe_errors() {
        let (_dir, storage, auth, app) = fixture().await;
        let token = auth.preauth(now_ms().unwrap()).unwrap();
        let cookie = format!("__Host-mm-preauth={token}");
        for (name, value) in [
            ("host", "evil.invalid"),
            ("origin", "https://evil.invalid"),
            ("x-csrf-token", "invalid"),
        ] {
            let mut req = request("/auth/login", "POST", Some(&cookie), Some(&token), "{}");
            req.headers_mut()
                .insert(name, HeaderValue::from_str(value).unwrap());
            req.headers_mut().insert(
                "x-forwarded-host",
                HeaderValue::from_static("monitor.invalid"),
            );
            req.headers_mut()
                .insert("x-forwarded-proto", HeaderValue::from_static("https"));
            assert_eq!(
                app.clone().oneshot(req).await.unwrap().status(),
                StatusCode::FORBIDDEN
            );
        }
        assert_eq!(
            app.clone()
                .oneshot(request("/auth/login", "POST", Some(&cookie), None, "{}"))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let huge = "z".repeat(BODY_BYTES + 1);
        let response = app
            .clone()
            .oneshot(request(
                "/auth/login",
                "POST",
                Some(&cookie),
                Some(&token),
                &huge,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let encoded = format!(
            "{{\"username\":\"fixture\",\"password\":\"{}\"}}",
            " ".repeat(1024)
        );
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "/auth/login",
                    "POST",
                    Some(&cookie),
                    Some(&token),
                    &encoded
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
        let absolute = request("https://evil.invalid/login", "GET", None, None, "");
        assert_eq!(
            app.clone().oneshot(absolute).await.unwrap().status(),
            StatusCode::BAD_REQUEST
        );
        let response=app.clone().oneshot(request("/auth/login","POST",Some(&cookie),Some(&token),"{\"password\":\"sensitive-placeholder\",\"username\":\"<script>\",\"unknown\":true}")).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let text = body(response).await;
        assert!(!text.contains("sensitive-placeholder"));
        assert!(!text.contains("<script>"));
        let mut req = request("/login", "GET", None, None, "");
        req.extensions_mut()
            .insert(Peer("192.0.2.1:42".parse().unwrap()));
        req.headers_mut()
            .insert("x-forwarded-for", HeaderValue::from_static("127.0.0.1"));
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
        for attempt in 0..3 {
            let response = app
                .clone()
                .oneshot(request(
                    "/auth/login",
                    "POST",
                    Some(&cookie),
                    Some(&token),
                    "{\"username\":\"missing\",\"password\":\"controlled test passphrase\"}",
                ))
                .await
                .unwrap();
            if attempt < 2 {
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            } else {
                assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
                assert_eq!(response.headers()[header::RETRY_AFTER], "10");
            }
        }
        storage.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn production_cookie_login_fixation_csrf_logout_and_password_change() {
        let (_dir, storage, auth, app) = fixture().await;
        auth.local_account(
            "fixture".into(),
            Zeroizing::new("controlled test passphrase".into()),
            false,
        )
        .await
        .unwrap();
        let response = app
            .clone()
            .oneshot(request("/login", "GET", None, None, ""))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let pre = response.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .to_owned();
        for flag in [
            "__Host-mm-preauth=",
            "Secure",
            "HttpOnly",
            "SameSite=Lax",
            "Path=/",
        ] {
            assert!(pre.contains(flag));
        }
        assert!(!pre.contains("Domain="));
        let pair = pre.split(';').next().unwrap();
        let token = pair.split_once('=').unwrap().1;
        let response = app
            .clone()
            .oneshot(request(
                "/auth/login",
                "POST",
                Some(&format!("{pair}; __Host-mm-session={}", "00".repeat(32))),
                Some(token),
                "{\"username\":\"fixture\",\"password\":\"controlled test passphrase\"}",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let session_cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .next()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        assert!(!session_cookie.contains(&"00".repeat(32)));
        let session_cookie = session_cookie.split(';').next().unwrap().to_owned();
        let response_body = body(response).await;
        let csrf = response_body.split('"').nth(3).unwrap();
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "/auth/logout",
                    "POST",
                    Some(&session_cookie),
                    None,
                    ""
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        let response = app
            .clone()
            .oneshot(request(
                "/api/v1/account/session",
                "GET",
                Some(&session_cookie),
                None,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(body(response).await.contains(csrf));
        let response=app.clone().oneshot(request("/api/v1/account/password","POST",Some(&session_cookie),Some(csrf),"{\"current_password\":\"controlled test passphrase\",\"new_password\":\"replacement test passphrase\"}")).await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            response.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "/api/v1/account/session",
                    "GET",
                    Some(&session_cookie),
                    None,
                    ""
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        storage.shutdown().await.unwrap();
    }
    #[tokio::test]
    async fn development_cookie_is_explicitly_separate_from_production() {
        let (_dir, storage, auth, _app) = fixture().await;
        let app = router(
            auth,
            &url::Url::parse("http://localhost:8080").unwrap(),
            Mode::Development,
            None,
        )
        .layer(middleware::from_fn(crate::web::boundary));
        let mut req = request("/login", "GET", None, None, "");
        req.headers_mut()
            .insert("host", HeaderValue::from_static("localhost:8080"));
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let c = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(c.starts_with("mm-dev-preauth="));
        assert!(!c.contains("Secure"));
        storage.shutdown().await.unwrap();
    }
}
