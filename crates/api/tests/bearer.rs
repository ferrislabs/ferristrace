use axum::body::Body;
use axum::http::header::AUTHORIZATION;
use axum::http::{Request, StatusCode};
use ferristrace_api::me_router;
use ferristrace_application::testing::Inert;
use ferristrace_domain::{NoPhotos, SessionToken, UserRefusal, UserTokens};
use tower::ServiceExt;

#[allow(dead_code)]
#[path = "support/identity.rs"]
mod identity;

use identity::{Fakes, state};

const THREE_PARTS: &str = "header.payload.signature";

struct Verdict(Result<(), UserRefusal>);

impl UserTokens for Verdict {
    async fn verify(&self, token: &str) -> Result<SessionToken, UserRefusal> {
        self.0.map(|()| SessionToken::for_subject(token))
    }
}

async fn standing(users: Option<Verdict>, authorization: Option<&str>) -> StatusCode {
    let app = state(Fakes::nowhere(), Inert::default(), NoPhotos, users);
    let mut request = Request::builder().uri("/auth/me");
    if let Some(header) = authorization {
        request = request.header(AUTHORIZATION, header);
    }
    me_router(app)
        .oneshot(request.body(Body::empty()).expect("a request"))
        .await
        .expect("a response")
        .status()
}

#[tokio::test]
async fn a_call_with_no_bearer_is_told_to_sign_in() {
    assert_eq!(
        standing(Some(Verdict(Ok(()))), None).await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn a_bearer_that_is_not_a_three_part_token_never_reaches_the_verifier() {
    for header in ["Bearer ft_live_0123456789", "Basic abc", "Bearer"] {
        assert_eq!(
            standing(Some(Verdict(Ok(()))), Some(header)).await,
            StatusCode::UNAUTHORIZED,
            "{header}"
        );
    }
}

#[tokio::test]
async fn every_refusal_but_an_unreachable_issuer_answers_401() {
    for refusal in [
        UserRefusal::Off,
        UserRefusal::NotAPerson,
        UserRefusal::Invalid,
    ] {
        assert_eq!(
            standing(
                Some(Verdict(Err(refusal))),
                Some(&format!("Bearer {THREE_PARTS}"))
            )
            .await,
            StatusCode::UNAUTHORIZED,
            "{refusal:?}"
        );
    }
}

#[tokio::test]
async fn an_unreachable_issuer_answers_503() {
    assert_eq!(
        standing(
            Some(Verdict(Err(UserRefusal::Unavailable))),
            Some(&format!("Bearer {THREE_PARTS}"))
        )
        .await,
        StatusCode::SERVICE_UNAVAILABLE
    );
}

#[tokio::test]
async fn a_server_with_no_oidc_block_refuses_every_bearer() {
    assert_eq!(
        standing(None::<Verdict>, Some(&format!("Bearer {THREE_PARTS}"))).await,
        StatusCode::UNAUTHORIZED
    );
}
