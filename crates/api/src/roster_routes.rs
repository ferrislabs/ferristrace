use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch};
use ferristrace_domain::{
    AccountId, Email, InvalidCredential, Member, Memberships, OrgRole, Roll, Roster, RosterError,
    SessionStore, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::explore_routes::{NotCarried, carried};
use crate::whoami_routes::photo_url;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    Owner,
    Admin,
    Member,
}

impl From<Standing> for OrgRole {
    fn from(standing: Standing) -> Self {
        match standing {
            Standing::Owner => Self::Owner,
            Standing::Admin => Self::Admin,
            Standing::Member => Self::Member,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Members {
    members: Vec<Listed>,
}

#[derive(Debug, Serialize)]
struct Listed {
    account: Uuid,
    email: String,
    name: Option<String>,
    photo: Option<String>,
    role: &'static str,
    joined_at_ms: u64,
}

impl From<Member> for Listed {
    fn from(member: Member) -> Self {
        Self {
            account: member.account.as_uuid(),
            email: member.email.as_str().to_owned(),
            name: member.name.map(Into::into),
            photo: member
                .photo
                .as_ref()
                .map(|portrait| photo_url(member.account.as_uuid(), portrait)),
            role: member.role.as_str(),
            joined_at_ms: member.joined_at_unix_nanos / NANOS_PER_MILLI,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Welcomed {
    member: Listed,
    invite_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Inviting {
    email: String,
    role: Standing,
}

#[derive(Debug, Deserialize)]
pub struct Granting {
    role: Standing,
}

pub fn roster_router<S, G, M>(roster: Arc<Roster<S, G, M>>) -> Router
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    M: Roll + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/api/orgs/{org}/members",
            get(members::<S, G, M>).post(invite::<S, G, M>),
        )
        .route(
            "/api/orgs/{org}/members/{account}",
            patch(grant::<S, G, M>).delete(remove::<S, G, M>),
        )
        .with_state(roster)
}

async fn members<S, G, M>(
    State(roster): State<Arc<Roster<S, G, M>>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Members>, MemberRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    M: Roll + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let roll = roster.members(&token, TenantId::new(org)).await?;
    Ok(Json(Members {
        members: roll.into_iter().map(Listed::from).collect(),
    }))
}

async fn invite<S, G, M>(
    State(roster): State<Arc<Roster<S, G, M>>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Inviting>,
) -> Result<(StatusCode, Json<Welcomed>), MemberRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    M: Roll + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let email = Email::try_from(asked.email.as_str())?;
    let role = OrgRole::from(asked.role);
    let invitation = roster
        .invite(&token, TenantId::new(org), &email, role)
        .await?;
    tracing::info!(
        org = %org,
        role = role.as_str(),
        awaited = invitation.way_in.is_some(),
        "an address was admitted to an organisation"
    );
    Ok((
        StatusCode::CREATED,
        Json(Welcomed {
            member: invitation.member.into(),
            invite_token: invitation.way_in.map(|way_in| way_in.exposed().to_owned()),
        }),
    ))
}

async fn grant<S, G, M>(
    State(roster): State<Arc<Roster<S, G, M>>>,
    Path((org, account)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<Granting>,
) -> Result<Json<Listed>, MemberRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    M: Roll + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    let changed = roster
        .grant(
            &token,
            TenantId::new(org),
            AccountId::new(account),
            asked.role.into(),
        )
        .await?;
    tracing::info!(org = %org, account = %account, role = changed.role.as_str(), "a role was changed");
    Ok(Json(changed.into()))
}

async fn remove<S, G, M>(
    State(roster): State<Arc<Roster<S, G, M>>>,
    Path((org, account)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<StatusCode, MemberRefused>
where
    S: SessionStore + Send + Sync + 'static,
    G: Memberships + Send + Sync + 'static,
    M: Roll + Send + Sync + 'static,
{
    let token = carried(&headers)?;
    roster
        .remove(&token, TenantId::new(org), AccountId::new(account))
        .await?;
    tracing::info!(org = %org, account = %account, "a member was removed");
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, thiserror::Error)]
pub enum MemberRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error("that is not an address anything could send to")]
    Address,
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("no such member")]
    NoSuchMember,
    #[error("changing who belongs here is for an administrator to do")]
    NotAnAdministrator,
    #[error("the owner role is an owner to hand out and an owner to take back")]
    OwnersOnly,
    #[error("an organisation keeps at least one owner, so this one stays")]
    LastOwner,
    #[error("that address already belongs to this organisation")]
    AlreadyAMember,
    #[error("the members could not be read")]
    Unavailable,
}

impl From<InvalidCredential> for MemberRefused {
    fn from(_: InvalidCredential) -> Self {
        Self::Address
    }
}

impl From<RosterError> for MemberRefused {
    fn from(error: RosterError) -> Self {
        match error {
            RosterError::NoSession => Self::Reaching(NotCarried::NoSession),
            RosterError::NotYours => Self::NoSuchOrg,
            RosterError::NotYoursToChange => Self::NotAnAdministrator,
            RosterError::OwnersOnly => Self::OwnersOnly,
            RosterError::LastOwner => Self::LastOwner,
            RosterError::NoSuchMember => Self::NoSuchMember,
            RosterError::AlreadyAMember => Self::AlreadyAMember,
            RosterError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for MemberRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Address => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoSuchOrg | Self::NoSuchMember => StatusCode::NOT_FOUND,
            Self::NotAnAdministrator | Self::OwnersOnly => StatusCode::FORBIDDEN,
            Self::AlreadyAMember | Self::LastOwner => StatusCode::CONFLICT,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the members could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
