use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use ferristrace_api::{
    SessionCookie, SignInMethods, me_router, methods_router, oidc_router, session_router,
};
use ferristrace_application::testing::Inert;
use ferristrace_domain::{
    AuthError, AuthProvider, Authorizer, NoPhotos, PendingFlow, ResolvedIdentity, SignInProof,
};
use tower::ServiceExt;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Fakes, state};

const GONE: [(&str, &str); 10] = [
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
];

struct Provider;

impl Authorizer for Provider {
    fn begin(&self) -> Result<(String, PendingFlow), AuthError> {
        Err(AuthError::ProviderUnreachable)
    }
}

impl AuthProvider for Provider {
    async fn sign_in(&self, _proof: SignInProof) -> Result<ResolvedIdentity, AuthError> {
        Err(AuthError::Rejected)
    }
}

fn identity_routes(federated: bool) -> axum::Router {
    let fakes = Fakes::nowhere();
    let app = if federated {
        state(
            fakes,
            Inert::default(),
            NoPhotos,
            Some(Provider),
            SessionCookie::default(),
        )
    } else {
        state(
            fakes,
            Inert::default(),
            NoPhotos,
            None::<Provider>,
            SessionCookie::default(),
        )
    };
    let router = methods_router(SignInMethods { oidc: federated })
        .merge(me_router(app.clone()))
        .merge(session_router(app.clone()));
    if federated {
        router.merge(oidc_router(app, "/"))
    } else {
        router
    }
}

async fn status_of(router: &axum::Router, method: &str, uri: &str) -> StatusCode {
    let request = Request::builder()
        .method(Method::from_bytes(method.as_bytes()).expect("a method"))
        .uri(uri)
        .header("x-requested-with", "ferristrace")
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
async fn no_password_or_second_factor_route_exists_with_a_provider_configured() {
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
async fn no_password_or_second_factor_route_exists_without_a_provider() {
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
async fn the_way_in_is_still_mounted_when_a_provider_is_configured() {
    let router = identity_routes(true);

    assert_ne!(
        status_of(&router, "GET", "/auth/oidc/start").await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        status_of(&router, "GET", "/auth/methods").await,
        StatusCode::OK
    );
}
