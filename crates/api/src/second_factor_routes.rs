use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{AppendHeaders, IntoResponse, Response};
use axum::routing::{get, post};
use ferristrace_application::Stack;
use ferristrace_application::registry::IdentityRepos;
use ferristrace_domain::{
    Answer, Confirmed, FactorError, FactorKind, NotACode, RecoveryCode, TotpCode, TotpSecret,
};
use serde::{Deserialize, Serialize};

use crate::carried::{NotCarried, carried};
use crate::session_cookie;
use crate::state::AppState;

#[derive(Debug, Serialize)]
struct StandingSaid {
    live: Vec<&'static str>,
    recovery_left: u32,
}

#[derive(Debug, Serialize)]
struct EnrolmentSaid {
    kind: &'static str,
    key: String,
    uri: String,
}

#[derive(Debug, Serialize)]
struct MintedSaid {
    recovery: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Offered {
    code: String,
}

#[derive(Debug, Deserialize)]
pub struct Answered {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    recovery: Option<String>,
}

pub fn second_factor_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: IdentityRepos,
{
    Router::new()
        .route("/auth/2fa", get(standing::<S>))
        .route("/auth/2fa/app", post(enrol::<S>).delete(forget::<S>))
        .route("/auth/2fa/app/confirm", post(confirm::<S>))
        .route("/auth/2fa/recovery", post(regenerate::<S>))
        .route("/auth/2fa/verify", post(verify::<S>))
        .with_state(state)
}

async fn standing<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SecondFactorRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    let standing = state.app.factor_standing(&token).await?;
    Ok(Json(StandingSaid {
        live: standing.live.iter().map(|kind| kind.as_str()).collect(),
        recovery_left: standing.recovery_left,
    })
    .into_response())
}

async fn enrol<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SecondFactorRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    let enrolment = state
        .app
        .enrol_factor(&token, &TotpSecret::random())
        .await?;
    Ok(Json(EnrolmentSaid {
        kind: enrolment.kind.as_str(),
        key: enrolment.key,
        uri: enrolment.uri,
    })
    .into_response())
}

async fn confirm<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(offered): Json<Offered>,
) -> Result<Response, SecondFactorRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    let code = TotpCode::try_from(offered.code.as_str())?;
    match state
        .app
        .confirm_factor(&token, code, RecoveryCode::ten())
        .await?
    {
        Confirmed::First { recovery } => {
            tracing::info!("a second factor was confirmed and recovery codes were minted");
            Ok(Json(MintedSaid {
                recovery: exposed(&recovery),
            })
            .into_response())
        }
        Confirmed::Another => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

async fn forget<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SecondFactorRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    state
        .app
        .forget_factor(&token, FactorKind::Authenticator)
        .await?;
    tracing::info!("a second factor was forgotten by its owner");
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn regenerate<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
) -> Result<Response, SecondFactorRefused>
where
    S::Repos: IdentityRepos,
{
    let token = carried(&headers)?;
    let minted = state
        .app
        .regenerate_recovery_codes(&token, RecoveryCode::ten())
        .await?;
    Ok(Json(MintedSaid {
        recovery: exposed(&minted),
    })
    .into_response())
}

async fn verify<S: Stack>(
    State(state): State<AppState<S>>,
    headers: HeaderMap,
    Json(answered): Json<Answered>,
) -> Result<Response, SecondFactorRefused>
where
    S::Repos: IdentityRepos,
{
    let challenge =
        session_cookie::challenged(&headers).ok_or(SecondFactorRefused::NoChallengeCarried)?;
    let answer = answer_of(&answered)?;
    let session = state
        .app
        .verify_second_step(
            &challenge,
            &answer,
            session_cookie::agent(&headers).as_ref(),
        )
        .await?;
    let (Some(opened), Some(cleared)) = (
        session_cookie::opened(&session, state.cookie),
        session_cookie::answered(state.cookie),
    ) else {
        return Err(SecondFactorRefused::Refused(FactorError::StoreUnreachable));
    };
    tracing::info!("a second step opened a session");
    Ok((
        AppendHeaders([(SET_COOKIE, opened), (SET_COOKIE, cleared)]),
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

fn answer_of(answered: &Answered) -> Result<Answer, SecondFactorRefused> {
    if let Some(code) = &answered.code {
        return Ok(Answer::Code(TotpCode::try_from(code.as_str())?));
    }
    let Some(typed) = &answered.recovery else {
        return Err(SecondFactorRefused::Shape);
    };
    let recovery = RecoveryCode::from_plaintext(typed);
    if !recovery.looks_like_one() {
        return Err(SecondFactorRefused::Shape);
    }
    Ok(Answer::Recovery(recovery))
}

fn exposed(codes: &[RecoveryCode]) -> Vec<String> {
    codes.iter().map(|code| code.exposed().to_owned()).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum SecondFactorRefused {
    #[error(transparent)]
    Reaching(NotCarried),
    #[error("the request carries no live challenge")]
    NoChallengeCarried,
    #[error("an answer carries a code or a recovery code")]
    Shape,
    #[error(transparent)]
    Refused(FactorError),
    #[error(transparent)]
    NotACode(NotACode),
}

impl From<NotCarried> for SecondFactorRefused {
    fn from(error: NotCarried) -> Self {
        Self::Reaching(error)
    }
}

impl From<FactorError> for SecondFactorRefused {
    fn from(error: FactorError) -> Self {
        Self::Refused(error)
    }
}

impl From<NotACode> for SecondFactorRefused {
    fn from(error: NotACode) -> Self {
        Self::NotACode(error)
    }
}

impl IntoResponse for SecondFactorRefused {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::Reaching(carried) => return (*carried).into_response(),
            Self::NoChallengeCarried => StatusCode::UNAUTHORIZED,
            Self::Shape | Self::NotACode(_) => StatusCode::BAD_REQUEST,
            Self::Refused(error) => match error {
                FactorError::NoSession | FactorError::NoChallenge | FactorError::Refused => {
                    StatusCode::UNAUTHORIZED
                }
                FactorError::Exhausted => StatusCode::TOO_MANY_REQUESTS,
                FactorError::NoFactor | FactorError::AlreadyKept => StatusCode::CONFLICT,
                FactorError::NotOffered => StatusCode::NOT_IMPLEMENTED,
                FactorError::StoreUnreachable => StatusCode::SERVICE_UNAVAILABLE,
            },
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "a second factor could not be served");
        }
        (status, self.to_string()).into_response()
    }
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::{Answer, FactorKind, RecoveryCode};

    use super::{
        Answered, EnrolmentSaid, MintedSaid, SecondFactorRefused, StandingSaid, answer_of,
    };

    fn typed(code: Option<&str>, recovery: Option<&str>) -> Answered {
        Answered {
            code: code.map(str::to_owned),
            recovery: recovery.map(str::to_owned),
        }
    }

    #[test]
    fn a_standing_names_the_live_kinds_and_what_is_left() {
        let said = StandingSaid {
            live: vec![FactorKind::Authenticator.as_str()],
            recovery_left: 7,
        };

        assert_eq!(
            serde_json::to_string(&said).expect("json"),
            r#"{"live":["app"],"recovery_left":7}"#
        );
    }

    #[test]
    fn an_enrolment_carries_the_key_and_the_uri() {
        let said = EnrolmentSaid {
            kind: FactorKind::Authenticator.as_str(),
            key: "ABCD".to_owned(),
            uri: "otpauth://totp/x".to_owned(),
        };

        assert_eq!(
            serde_json::to_string(&said).expect("json"),
            r#"{"kind":"app","key":"ABCD","uri":"otpauth://totp/x"}"#
        );
    }

    #[test]
    fn minted_codes_are_sent_as_their_owner_will_read_them() {
        let codes = RecoveryCode::ten();
        let said = MintedSaid {
            recovery: super::exposed(&codes),
        };

        assert_eq!(said.recovery.len(), 10);
        assert_eq!(said.recovery[0], codes[0].exposed());
    }

    #[test]
    fn an_answer_is_a_code_or_a_recovery_code_and_nothing_else() {
        assert!(matches!(
            answer_of(&typed(Some("123456"), None)),
            Ok(Answer::Code(_))
        ));
        assert!(matches!(
            answer_of(&typed(None, Some("abcde-fghjk"))),
            Ok(Answer::Recovery(_))
        ));
        assert!(matches!(
            answer_of(&typed(None, None)),
            Err(SecondFactorRefused::Shape)
        ));
        assert!(matches!(
            answer_of(&typed(None, Some("nope"))),
            Err(SecondFactorRefused::Shape)
        ));
        assert!(matches!(
            answer_of(&typed(Some("12345"), None)),
            Err(SecondFactorRefused::NotACode(_))
        ));
    }
}
