use std::path::Path;

use axum::Router;
use tower_http::services::{ServeDir, ServeFile};

pub fn web_router(root: &Path) -> Router {
    let index = root.join("index.html");
    Router::new().fallback_service(
        ServeDir::new(root)
            .append_index_html_on_directories(true)
            .fallback(ServeFile::new(index)),
    )
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::web_router;

    async fn asked(named: &str, uri: &str) -> (StatusCode, String) {
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
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8_lossy(&body).into_owned())
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
    async fn a_reset_link_reaches_the_app_rather_than_the_api() {
        let (status, body) = asked("reset", "/reset?token=ft_pr_abc").await;

        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("the app"));
    }
}
