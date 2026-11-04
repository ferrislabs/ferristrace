use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::routing::get;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignInMethods {
    pub oidc: bool,
    pub issuer: Option<String>,
    pub client_id: Option<String>,
}

impl SignInMethods {
    pub fn off() -> Self {
        Self {
            oidc: false,
            issuer: None,
            client_id: None,
        }
    }

    pub fn oidc(issuer: &str, client_id: &str) -> Self {
        Self {
            oidc: true,
            issuer: Some(issuer.to_owned()),
            client_id: Some(client_id.to_owned()),
        }
    }
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
    async fn the_way_in_is_readable_before_anybody_has_signed_in() {
        let (status, body) = asked(SignInMethods::oidc(
            "https://auth.example.com/realms/ferris",
            "ferristrace-web",
        ))
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            body,
            r#"{"oidc":true,"issuer":"https://auth.example.com/realms/ferris","client_id":"ferristrace-web"}"#
        );
    }

    #[tokio::test]
    async fn a_deployment_with_no_way_in_says_so() {
        let (status, body) = asked(SignInMethods::off()).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"oidc":false,"issuer":null,"client_id":null}"#);
    }
}
