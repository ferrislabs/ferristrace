use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SignInMethods {
    pub oidc: bool,
}

pub fn methods_router(methods: SignInMethods) -> Router {
    Router::new()
        .route("/auth/methods", get(offered))
        .with_state(methods)
}

async fn offered(State(methods): State<SignInMethods>) -> Json<SignInMethods> {
    Json(methods)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    use super::{SignInMethods, methods_router};

    async fn asked(methods: SignInMethods) -> (StatusCode, String) {
        let request = Request::builder()
            .uri("/auth/methods")
            .body(Body::empty())
            .expect("a request");
        let response = methods_router(methods)
            .oneshot(request)
            .await
            .expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8(body.to_vec()).expect("text"))
    }

    #[tokio::test]
    async fn the_ways_in_are_readable_before_anybody_has_signed_in() {
        let (status, body) = asked(SignInMethods { oidc: true }).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"oidc":true}"#);
    }

    #[tokio::test]
    async fn a_deployment_offering_no_way_says_so() {
        let (status, body) = asked(SignInMethods { oidc: false }).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"oidc":false}"#);
    }

    #[tokio::test]
    async fn a_federated_way_in_is_said_so() {
        let (_, body) = asked(SignInMethods { oidc: true }).await;

        assert_eq!(body, r#"{"oidc":true}"#);
    }
}
