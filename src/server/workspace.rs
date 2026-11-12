//! Local-only management endpoints for the embedded Codex browser reader.
use crate::service::legado_reading_time::MAX_ARCHIVE_BYTES;
use crate::{
    api::{
        commands::{legado_time, webdav},
        AppState,
    },
    error::error::ApiResponse,
};
use axum::{
    extract::{DefaultBodyLimit, Multipart, Request, State},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};

pub fn router(enabled: bool) -> Router<AppState> {
    if !enabled {
        return Router::new();
    }
    Router::new()
        .route("/reader3/testLegadoWebdav", post(test_webdav))
        .route("/reader3/syncLegadoBookProgress", post(sync_progress))
        .route("/reader3/uploadLegadoBookProgress", post(upload_progress))
        .route(
            "/reader3/mergeLegadoReadingTime",
            post(merge_time).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES + 1024)),
        )
        .route(
            "/reader3/inspectLegadoLibrary",
            post(inspect_library).layer(DefaultBodyLimit::max(MAX_ARCHIVE_BYTES + 1024)),
        )
        .layer(middleware::from_fn(local_request))
}

// Reject DNS rebinding and cross-site form uploads. Browser sends a non-simple
// header, so a third-party page cannot invoke these endpoints via a form.
async fn local_request(req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let valid_host = host.split_once(':').is_some_and(|(name, port)| {
        (name == "127.0.0.1" || name == "localhost") && port.parse::<u16>().is_ok()
    });
    let origin_ok = req
        .headers()
        .get("origin")
        .is_none_or(|origin| origin.to_str().ok() == Some(format!("http://{host}").as_str()));
    if !valid_host || !origin_ok || req.headers().get("x-reader-local").is_none_or(|v| v != "1") {
        return (
            axum::http::StatusCode::FORBIDDEN,
            Json(ApiResponse::<()>::err("此操作只允许本机阅读页面调用")),
        )
            .into_response();
    }
    next.run(req).await
}

#[derive(serde::Deserialize)]
struct ConfigRequest {
    config: webdav::LegadoWebdavConfig,
}
async fn test_webdav(State(state): State<AppState>, Json(req): Json<ConfigRequest>) -> Response {
    super::router::envelope(webdav::test_legado_webdav_core(&state, req.config).await)
}
async fn sync_progress(
    State(state): State<AppState>,
    Json(req): Json<webdav::LegadoProgressRequest>,
) -> Response {
    super::router::envelope(webdav::sync_legado_book_progress_core(&state, req).await)
}
async fn upload_progress(
    State(state): State<AppState>,
    Json(req): Json<webdav::LegadoProgressRequest>,
) -> Response {
    super::router::envelope(webdav::upload_legado_book_progress_core(&state, req).await)
}
async fn merge_time(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let (_, bytes) = match super::router::read_upload(&mut multipart).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };
    super::router::envelope(legado_time::merge_legado_reading_time_core(&state, bytes).await)
}

async fn inspect_library(State(state): State<AppState>, mut multipart: Multipart) -> Response {
    let (_, bytes) = match super::router::read_upload(&mut multipart).await {
        Ok(upload) => upload,
        Err(response) => return response,
    };
    super::router::envelope(legado_time::inspect_legado_library_core(&state, bytes).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn local_management_rejects_cross_origin_forms_and_dns_rebinding() {
        let app = Router::new()
            .route("/check", post(|| async { "ok" }))
            .layer(middleware::from_fn(local_request));
        for (host, origin, header, expected) in [
            (
                "127.0.0.1:1122",
                Some("http://127.0.0.1:1122"),
                true,
                StatusCode::OK,
            ),
            ("localhost:1122", None, true, StatusCode::OK),
            ("evil.example:1122", None, true, StatusCode::FORBIDDEN),
            (
                "127.0.0.1:1122",
                Some("https://evil.example"),
                true,
                StatusCode::FORBIDDEN,
            ),
            ("127.0.0.1:1122", None, false, StatusCode::FORBIDDEN),
        ] {
            let mut req = HttpRequest::builder()
                .method("POST")
                .uri("/check")
                .header("host", host);
            if let Some(origin) = origin {
                req = req.header("origin", origin);
            }
            if header {
                req = req.header("x-reader-local", "1");
            }
            assert_eq!(
                app.clone()
                    .oneshot(req.body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                expected
            );
        }
    }
}
