//! Embedded, nonsensitive local assets. All data is fetched through guarded APIs.
use axum::{Router, http::header, response::Html, routing::get};
pub(crate) fn router() -> Router {
    Router::new()
        .route("/", get(page))
        .route("/settings", get(page))
        .route("/incidents", get(page))
        .route("/monitors/{id}", get(page))
        .route(
            "/assets/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../assets/app.js"),
                )
            }),
        )
        .route(
            "/assets/theme.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../assets/theme.js"),
                )
            }),
        )
        .route(
            "/assets/app.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../assets/app.css"),
                )
            }),
        )
}
async fn page() -> Html<&'static str> {
    Html(include_str!("../assets/app.html"))
}
