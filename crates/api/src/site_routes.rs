use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::routing::get;
use ferristrace_application::Stack;
use ferristrace_application::registry::SiteRepos;
use ferristrace_domain::{ServiceSite, TenantId};
use serde::Serialize;
use uuid::Uuid;

use crate::carried::reading;
use crate::explore_routes::LookRefused;
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct Site {
    project: Option<String>,
    environments: Vec<String>,
}

impl From<ServiceSite> for Site {
    fn from(site: ServiceSite) -> Self {
        Self {
            project: site.project.map(str::into_string),
            environments: site
                .environments
                .into_iter()
                .map(str::into_string)
                .collect(),
        }
    }
}

pub fn site_router<S: Stack>(state: AppState<S>) -> Router
where
    S::Repos: SiteRepos,
{
    Router::new()
        .route(
            "/api/orgs/{org}/services/{name}/placement",
            get(placement::<S>),
        )
        .with_state(state)
}

async fn placement<S: Stack>(
    State(state): State<AppState<S>>,
    Path((org, name)): Path<(Uuid, String)>,
    headers: HeaderMap,
) -> Result<Json<Site>, LookRefused>
where
    S::Repos: SiteRepos,
{
    let who = reading(&state, &headers).await?;
    let site = state
        .app
        .service_site(&who, TenantId::new(org), &name)
        .await?;
    Ok(Json(site.into()))
}
