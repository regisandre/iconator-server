//! HTTP server exposing the `iconator` file and folder icon lookups.

use std::{env, path::Path, time::Duration};

use anyhow::Context;
use axum::{
    Json, Router,
    extract::Query,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::{
    net::TcpListener,
    signal::{
        self,
        unix::{SignalKind, signal},
    },
};
use tower_http::{limit::RequestBodyLimitLayer, timeout::TimeoutLayer, trace::TraceLayer};
use tracing_subscriber::EnvFilter;

const DEFAULT_HOST: &str = "0.0.0.0";
const DEFAULT_PORT: u16 = 8080;
const MAX_PATH_LEN: usize = 4096;
const MAX_BODY_BYTES: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
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
    InvalidPath(&'static str),
    NotFound,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
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

fn lookup(
    query: &IconQuery,
    get_icon: impl Fn(&Path) -> Option<u64>,
) -> Result<Response, ApiError> {
    let path = validate_path(query.path.as_deref())?;
    let icon_id = get_icon(path).ok_or(ApiError::NotFound)?;
    Ok((
        [(header::CACHE_CONTROL, ICON_CACHE_CONTROL)],
        Json(IconResponse { icon_id }),
    )
        .into_response())
}

async fn file_icon(Query(query): Query<IconQuery>) -> Result<Response, ApiError> {
    lookup(&query, |path| iconator::get_icon_for_file(path))
}

async fn folder_icon(Query(query): Query<IconQuery>) -> Result<Response, ApiError> {
    lookup(&query, |path| iconator::get_icon_for_folder(path))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

fn app() -> Router {
    Router::new()
        .route("/icons/file", get(file_icon))
        .route("/icons/folder", get(folder_icon))
        .route("/health", get(health))
        .layer((
            TraceLayer::new_for_http(),
            TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT),
            RequestBodyLimitLayer::new(MAX_BODY_BYTES),
        ))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(err) = signal::ctrl_c().await {
            tracing::error!(%err, "cannot listen for Ctrl+C");
            std::future::pending::<()>().await;
        }
    };
    let terminate = async {
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(err) => {
                tracing::error!(%err, "cannot listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };
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

    async fn get(uri: &str) -> (StatusCode, serde_json::Value) {
        let response = send(uri).await;
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&body).unwrap())
    }

    #[tokio::test]
    async fn known_file_returns_icon_id() {
        assert_eq!(
            get("/icons/file?path=./test.rs").await,
            (StatusCode::OK, json!({ "icon_id": 525 }))
        );
    }

    #[tokio::test]
    async fn icon_responses_are_cacheable() {
        let response = send("/icons/file?path=./test.rs").await;
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            ICON_CACHE_CONTROL
        );
    }

    #[tokio::test]
    async fn known_folder_returns_icon_id() {
        assert_eq!(
            get("/icons/folder?path=./src").await,
            (StatusCode::OK, json!({ "icon_id": 1054 }))
        );
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
    async fn invalid_paths_return_bad_request() {
        let too_long = format!("/icons/file?path={}", "a".repeat(MAX_PATH_LEN + 1));
        for uri in [
            "/icons/file",
            "/icons/file?path=",
            "/icons/file?path=.",
            "/icons/folder?path=..",
            "/icons/folder?path=/",
            "/icons/file?path=a%00.rs",
            too_long.as_str(),
        ] {
            let (status, body) = get(uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(body["error"], "invalid_path", "{uri}");
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
