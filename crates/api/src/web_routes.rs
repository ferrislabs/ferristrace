use std::path::Path;

use axum::Router;
use axum::extract::Request;
use axum::http::header::CACHE_CONTROL;
use axum::http::{HeaderValue, StatusCode};
use axum::middleware::{Next, from_fn};
use axum::response::{IntoResponse, Response};
use tower_http::services::{ServeDir, ServeFile};

const HASHED: &str = "public, max-age=31536000, immutable";
const REVALIDATED: &str = "no-cache";
const SERVER_PREFIXES: [&str; 2] = ["/auth/", "/api/"];

pub fn web_router(root: &Path) -> Router {
    let index = root.join("index.html");
    Router::new()
        .fallback_service(
            ServeDir::new(root)
                .append_index_html_on_directories(true)
                .fallback(ServeFile::new(index)),
        )
        .layer(from_fn(cached))
}

async fn cached(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    if SERVER_PREFIXES
        .iter()
        .any(|prefix| path.starts_with(prefix))
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let hashed = path.starts_with("/assets/");
    let mut response = next.run(request).await;
    if response.status().is_success() {
        let policy = if hashed { HASHED } else { REVALIDATED };
        response
            .headers_mut()
            .insert(CACHE_CONTROL, HeaderValue::from_static(policy));
    }
    response
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::web_router;

    async fn asked(named: &str, uri: &str) -> (StatusCode, String) {
        let (status, _, body) = answered(named, uri).await;
        (status, body)
    }

    async fn answered(named: &str, uri: &str) -> (StatusCode, Option<String>, String) {
        let root = std::env::temp_dir().join(format!("ferristrace-web-{named}"));
        std::fs::create_dir_all(root.join("assets")).expect("a directory");
        std::fs::write(root.join("index.html"), "<!doctype html>the app").expect("the page");
        std::fs::write(root.join("assets").join("app.js"), "console.log(1)").expect("a bundle");

        let request = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("a request");
        let response = web_router(&root)
            .oneshot(request)
            .await
            .expect("a response");
        let status = response.status();
        let policy = response
            .headers()
            .get(axum::http::header::CACHE_CONTROL)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, policy, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn the_page_is_revalidated_so_a_deploy_reaches_every_browser() {
        for uri in ["/", "/traces", "/index.html"] {
            let (_, policy, _) = answered("revalidated", uri).await;

            assert_eq!(policy.as_deref(), Some("no-cache"), "{uri}");
        }
    }

    #[tokio::test]
    async fn a_hashed_asset_is_kept_for_a_year() {
        let (_, policy, _) = answered("hashed", "/assets/app.js").await;

        assert_eq!(
            policy.as_deref(),
            Some("public, max-age=31536000, immutable")
        );
    }

    #[tokio::test]
    async fn the_root_serves_the_page() {
        let (status, body) = asked("root", "/").await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("the app"), "{body}");
    }

    #[tokio::test]
    async fn a_built_asset_is_served_as_itself() {
        let (status, body) = asked("asset", "/assets/app.js").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "console.log(1)");
    }

    #[tokio::test]
    async fn a_path_only_the_browser_knows_still_serves_the_page() {
        for deep in [
            "/traces",
            "/traces/abc123",
            "/settings/organization/members",
        ] {
            let (status, body) = asked("deep", deep).await;

            assert_eq!(status, StatusCode::OK, "{deep} should reach the app");
            assert!(body.contains("the app"), "{deep} served {body}");
        }
    }

    #[tokio::test]
    async fn a_server_path_nobody_answers_is_not_found_rather_than_the_page() {
        for gone in [
            "/auth/sign-in",
            "/auth/reset/confirm",
            "/auth/2fa/verify",
            "/api/nothing",
        ] {
            let (status, body) = asked("server", gone).await;

            assert_eq!(status, StatusCode::NOT_FOUND, "{gone}");
            assert!(!body.contains("the app"), "{gone} served {body}");
        }
    }
}
