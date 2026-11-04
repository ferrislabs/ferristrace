use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, put};
use ferristrace_application::Stack;
use ferristrace_application::registry::TeamRepos;
use ferristrace_domain::{
    AccountId, BadTeamName, ProjectId, Team, TeamError, TeamId, TeamName, TenantId,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::carried::{NotCarried, carried};
use crate::state::AppState;

const NANOS_PER_MILLI: u64 = 1_000_000;

#[derive(Debug, Serialize)]
pub struct Arranged {
    teams: Vec<Listed>,
}

#[derive(Debug, Serialize)]
struct Listed {
    id: Uuid,
    name: String,
    created_at_ms: u64,
    members: Vec<Uuid>,
    projects: Vec<Held>,
}

#[derive(Debug, Serialize)]
struct Held {
    id: Uuid,
    name: String,
}

impl From<Team> for Listed {
    fn from(team: Team) -> Self {
        Self {
            id: team.id.as_uuid(),
            name: team.name.into_string(),
            created_at_ms: team.created_at_unix_nanos / NANOS_PER_MILLI,
            members: team.members.into_iter().map(AccountId::as_uuid).collect(),
            projects: team
                .projects
                .into_iter()
                .map(|project| Held {
                    id: project.id.as_uuid(),
                    name: project.name.into_string(),
                })
                .collect(),
        }
    }
}

fn arranged(teams: Vec<Team>) -> Json<Arranged> {
    Json(Arranged {
        teams: teams.into_iter().map(Listed::from).collect(),
    })
}

#[derive(Debug, Deserialize)]
pub struct Naming {
    name: String,
}

#[derive(Debug, Deserialize)]
pub struct Seating {
    members: Vec<Uuid>,
}

#[derive(Debug, Deserialize)]
pub struct Owning {
    projects: Vec<Uuid>,
}

pub fn team_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: TeamRepos,
{
    Router::new()
        .route("/api/orgs/{org}/teams", get(teams::<S>).post(create::<S>))
        .route(
            "/api/orgs/{org}/teams/{team}",
            patch(rename::<S>).delete(disband::<S>),
        )
        .route("/api/orgs/{org}/teams/{team}/members", put(seat::<S>))
        .route("/api/orgs/{org}/teams/{team}/projects", put(own::<S>))
        .with_state(state)
}

async fn teams<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Json<Arranged>, TeamRefused>
where
    S::Repos: TeamRepos,
{
    let token = carried(&state, &headers).await?;
    Ok(arranged(
        state.app.list_teams(&token, TenantId::new(org)).await?,
    ))
}

async fn create<S: Stack>(
    State(state): State<AppState<S>>,
    Path(org): Path<Uuid>,
    headers: HeaderMap,
    Json(asked): Json<Naming>,
) -> Result<(StatusCode, Json<Arranged>), TeamRefused>
where
    S::Repos: TeamRepos,
{
    let token = carried(&state, &headers).await?;
    let name = TeamName::try_from(asked.name.as_str())?;
    let teams = state
        .app
        .create_team(&token, TenantId::new(org), &name)
        .await?;
    tracing::info!(org = %org, "a team was created");
    Ok((StatusCode::CREATED, arranged(teams)))
}

async fn rename<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, team)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<Naming>,
) -> Result<Json<Arranged>, TeamRefused>
where
    S::Repos: TeamRepos,
{
    let token = carried(&state, &headers).await?;
    let name = TeamName::try_from(asked.name.as_str())?;
    let teams = state
        .app
        .rename_team(&token, TenantId::new(org), TeamId::new(team), &name)
        .await?;
    tracing::info!(org = %org, team = %team, "a team was renamed");
    Ok(arranged(teams))
}

async fn disband<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, team)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
) -> Result<Json<Arranged>, TeamRefused>
where
    S::Repos: TeamRepos,
{
    let token = carried(&state, &headers).await?;
    let teams = state
        .app
        .disband_team(&token, TenantId::new(org), TeamId::new(team))
        .await?;
    tracing::info!(org = %org, team = %team, "a team was disbanded");
    Ok(arranged(teams))
}

async fn seat<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, team)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<Seating>,
) -> Result<Json<Arranged>, TeamRefused>
where
    S::Repos: TeamRepos,
{
    let token = carried(&state, &headers).await?;
    let members: Vec<AccountId> = asked.members.into_iter().map(AccountId::new).collect();
    let teams = state
        .app
        .seat_team(&token, TenantId::new(org), TeamId::new(team), &members)
        .await?;
    tracing::info!(org = %org, team = %team, members = members.len(), "a team was seated");
    Ok(arranged(teams))
}

async fn own<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, team)): Path<(Uuid, Uuid)>,
    headers: HeaderMap,
    Json(asked): Json<Owning>,
) -> Result<Json<Arranged>, TeamRefused>
where
    S::Repos: TeamRepos,
{
    let token = carried(&state, &headers).await?;
    let projects: Vec<ProjectId> = asked.projects.into_iter().map(ProjectId::new).collect();
    let teams = state
        .app
        .own_projects(&token, TenantId::new(org), TeamId::new(team), &projects)
        .await?;
    tracing::info!(org = %org, team = %team, projects = projects.len(), "a team was handed its projects");
    Ok(arranged(teams))
}

#[derive(Debug, thiserror::Error)]
pub enum TeamRefused {
    #[error(transparent)]
    Reaching(#[from] NotCarried),
    #[error(transparent)]
    Name(#[from] BadTeamName),
    #[error("no such organisation")]
    NoSuchOrg,
    #[error("no such team")]
    NoSuchTeam,
    #[error("teams are an administrator's to arrange")]
    NotAnAdministrator,
    #[error("this organisation already holds a team under that name")]
    NameTaken,
    #[error("the teams could not be read")]
    Unavailable,
}

impl From<TeamError> for TeamRefused {
    fn from(error: TeamError) -> Self {
        match error {
            TeamError::NoSession => Self::Reaching(NotCarried::NoSession),
            TeamError::NotYours => Self::NoSuchOrg,
            TeamError::NotYoursToChange => Self::NotAnAdministrator,
            TeamError::NoSuchTeam => Self::NoSuchTeam,
            TeamError::NameTaken => Self::NameTaken,
            TeamError::StoreUnreachable => Self::Unavailable,
        }
    }
}

impl IntoResponse for TeamRefused {
    fn into_response(self) -> Response {
        let status = match self {
            Self::Reaching(refused) => return refused.into_response(),
            Self::Name(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::NoSuchOrg | Self::NoSuchTeam => StatusCode::NOT_FOUND,
            Self::NotAnAdministrator => StatusCode::FORBIDDEN,
            Self::NameTaken => StatusCode::CONFLICT,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        if status.is_server_error() {
            tracing::error!(error = %self, "the teams could not be served");
        }
        (status, self.to_string()).into_response()
    }
}
