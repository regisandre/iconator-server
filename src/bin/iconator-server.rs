//! HTTP server exposing the `iconator` file and folder icon lookups.

use std::{env, path::Path};

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{Query, rejection::QueryRejection},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{net::TcpListener, signal};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 8080;
const MAX_PATH_LEN: usize = 4096;
/// Lookups are compiled into the binary, so a given path always maps to the same icon.
const ICON_CACHE_CONTROL: &str = "public, max-age=86400";

#[derive(Deserialize)]
struct IconQuery {
    path: Option<String>,
}

#[derive(Serialize)]
struct IconResponse {
    icon_id: u64,
}

enum ApiError {
    /// The query string could not be parsed (e.g. `path` given twice).
    InvalidQuery(String),
    InvalidPath(&'static str),
    NotFound,
}

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
        Self::InvalidQuery(rejection.body_text())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::InvalidQuery(message) => (
                StatusCode::BAD_REQUEST,
                json!({ "error": "invalid_query", "message": message }),
            ),
            Self::InvalidPath(message) => (
                StatusCode::BAD_REQUEST,
                json!({ "error": "invalid_path", "message": message }),
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, json!({ "error": "not_found" })),
        };
        (status, Json(body)).into_response()
    }
}

/// Checks the raw query value. The path is never resolved or read from disk:
/// the library only looks at its last component and extension.
fn validate_path(path: Option<&str>) -> Result<&Path, ApiError> {
    let path = path
        .filter(|path| !path.is_empty())
        .ok_or(ApiError::InvalidPath("`path` query parameter is required"))?;
    if path.len() > MAX_PATH_LEN {
        return Err(ApiError::InvalidPath("`path` is too long"));
    }
    if path.contains('\0') {
        return Err(ApiError::InvalidPath("`path` must not contain NUL bytes"));
    }
    let path = Path::new(path);
    if path.file_name().is_none() {
        return Err(ApiError::InvalidPath("`path` has no file or folder name"));
    }
    Ok(path)
}

/// Shared by both icon routes. Taking the extractor's `Result` lets a malformed query string
/// get the same JSON error shape as every other failure instead of axum's plain-text rejection.
fn lookup(
    query: Result<Query<IconQuery>, QueryRejection>,
    get_icon: fn(&Path) -> Option<u64>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(query) = query?;
    let path = validate_path(query.path.as_deref())?;
    let icon_id = get_icon(path).ok_or(ApiError::NotFound)?;
    Ok((
        [(header::CACHE_CONTROL, ICON_CACHE_CONTROL)],
        Json(IconResponse { icon_id }),
    ))
}

async fn file_icon(
    query: Result<Query<IconQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    lookup(query, |path| iconator::get_icon_for_file(path))
}

async fn folder_icon(
    query: Result<Query<IconQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    lookup(query, |path| iconator::get_icon_for_folder(path))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

/// GET-only routes that never read a body and answer from memory, so there is no body limit or
/// handler timeout here; connection limits and timeouts belong to the reverse proxy.
fn app() -> Router {
    Router::new()
        .route("/icons/file", get(file_icon))
        .route("/icons/folder", get(folder_icon))
        .route("/health", get(health))
        .layer(TraceLayer::new_for_http())
}

/// Resolves on Ctrl+C, or on SIGTERM (what Docker and Kubernetes send) on Unix.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = signal::ctrl_c().await {
            tracing::error!(%err, "cannot listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(err) => {
                tracing::error!(%err, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let host = env::var("HOST").unwrap_or_else(|_| DEFAULT_HOST.to_owned());
    let port = match env::var("PORT") {
        Ok(port) => port
            .parse()
            .with_context(|| format!("invalid PORT: {port:?}"))?,
        Err(_) => DEFAULT_PORT,
    };

    let listener = TcpListener::bind((host.as_str(), port))
        .await
        .with_context(|| format!("cannot bind to {host}:{port}"))?;
    tracing::info!(addr = %listener.local_addr()?, "listening");

    axum::serve(listener, app())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn send(uri: &str) -> Response {
        let request = Request::get(uri).body(Body::empty()).unwrap();
        app().oneshot(request).await.unwrap()
    }

    async fn read(response: Response) -> (StatusCode, serde_json::Value) {
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    async fn get(uri: &str) -> (StatusCode, serde_json::Value) {
        read(send(uri).await).await
    }

    #[tokio::test]
    async fn known_names_return_a_cacheable_icon_id() {
        for (uri, icon_id) in [
            ("/icons/file?path=./test.rs", 525),
            ("/icons/file?path=Cargo.toml", 85),
            ("/icons/folder?path=./src", 1054),
        ] {
            let response = send(uri).await;
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                ICON_CACHE_CONTROL,
                "{uri}"
            );
            assert_eq!(
                read(response).await,
                (StatusCode::OK, json!({ "icon_id": icon_id })),
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn unknown_names_return_not_found() {
        for uri in [
            "/icons/file?path=foo.unknownext",
            "/icons/file?path=foo",
            "/icons/file?path=../../etc/passwd",
            "/icons/folder?path=not-a-known-folder",
        ] {
            assert_eq!(
                get(uri).await,
                (StatusCode::NOT_FOUND, json!({ "error": "not_found" })),
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn bad_requests_return_a_json_error() {
        let too_long = format!("/icons/file?path={}", "a".repeat(MAX_PATH_LEN + 1));
        for (uri, error) in [
            ("/icons/file", "invalid_path"),
            ("/icons/file?path=", "invalid_path"),
            ("/icons/file?path=.", "invalid_path"),
            ("/icons/folder?path=..", "invalid_path"),
            ("/icons/folder?path=/", "invalid_path"),
            ("/icons/file?path=a%00.rs", "invalid_path"),
            (too_long.as_str(), "invalid_path"),
            ("/icons/file?path=a.rs&path=b.rs", "invalid_query"),
        ] {
            let (status, body) = get(uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(body["error"], error, "{uri}");
        }
    }

    #[tokio::test]
    async fn health_returns_ok() {
        assert_eq!(
            get("/health").await,
            (StatusCode::OK, json!({ "status": "ok" }))
        );
    }
}
