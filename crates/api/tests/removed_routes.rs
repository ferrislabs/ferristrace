use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{SignInMethods, me_router, methods_router};
use ferristrace_application::testing::{Bearers, Inert};
use ferristrace_domain::NoPhotos;
use tower::ServiceExt;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Fakes, state};

const GONE: [(&str, &str); 15] = [
    ("POST", "/auth/sign-in"),
    ("POST", "/auth/password"),
    ("POST", "/auth/reset/request"),
    ("POST", "/auth/reset/confirm"),
    ("GET", "/auth/2fa"),
    ("POST", "/auth/2fa/app"),
    ("DELETE", "/auth/2fa/app"),
    ("POST", "/auth/2fa/app/confirm"),
    ("POST", "/auth/2fa/recovery"),
    ("POST", "/auth/2fa/verify"),
    ("GET", "/auth/oidc/start"),
    ("GET", "/auth/oidc/callback"),
    ("POST", "/auth/sign-out"),
    ("GET", "/auth/sessions"),
    (
        "DELETE",
        "/auth/sessions/6f1d1f0e-0000-4000-8000-000000000001",
    ),
];

fn identity_routes(federated: bool) -> axum::Router {
    let app = state(
        Fakes::nowhere(),
        Inert::default(),
        NoPhotos,
        federated.then_some(Bearers),
    );
    let methods = if federated {
        SignInMethods::oidc("https://id.example/realms/ferris", "ferristrace-web")
    } else {
        SignInMethods::off()
    };
    methods_router(methods).merge(me_router(app))
}

async fn status_of(router: &axum::Router, method: &str, uri: &str) -> StatusCode {
    let request = Request::builder()
        .method(Method::from_bytes(method.as_bytes()).expect("a method"))
        .uri(uri)
        .body(Body::empty())
        .expect("a request");
    router
        .clone()
        .oneshot(request)
        .await
        .expect("a response")
        .status()
}

#[tokio::test]
async fn no_removed_route_exists_with_a_provider_configured() {
    let router = identity_routes(true);

    for (method, uri) in GONE {
        assert_eq!(
            status_of(&router, method, uri).await,
            StatusCode::NOT_FOUND,
            "{method} {uri}"
        );
    }
}

#[tokio::test]
async fn no_removed_route_exists_without_a_provider() {
    let router = identity_routes(false);

    for (method, uri) in GONE {
        assert_eq!(
            status_of(&router, method, uri).await,
            StatusCode::NOT_FOUND,
            "{method} {uri}"
        );
    }
}

#[tokio::test]
async fn the_methods_are_still_mounted_when_a_provider_is_configured() {
    let router = identity_routes(true);

    assert_eq!(
        status_of(&router, "GET", "/auth/methods").await,
        StatusCode::OK
    );
}
