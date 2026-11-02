use axum::Router;
use ferristrace_api::{
    AppState, SessionCookie, SignInMethods, audit_router, auth_router, catalogue_router,
    explore_router, export_router, issue_router, key_router, log_router, me_router, methods_router,
    oidc_router, org_router, password_router, reset_router, roster_router, second_factor_router,
    session_router, team_router, usage_router,
};

use crate::adapters::Production;
use crate::config::Config;

pub(crate) type State = AppState<Production>;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Held {
    pub(crate) explore: bool,
    pub(crate) issues: bool,
}

pub(crate) fn cookie_of(config: &Config) -> SessionCookie {
    SessionCookie {
        secure: config.cookie_secure,
        ..SessionCookie::default()
    }
}

pub(crate) fn control_routes(config: &Config, state: Option<&State>, held: Held) -> Router {
    identity_routes(config, state)
        .merge(explore_routes(state, held.explore))
        .merge(issue_routes(state, held.issues))
        .merge(catalogue_routes(state))
        .merge(key_routes(state))
        .merge(org_routes(state))
        .merge(audit_routes(state))
        .merge(export_routes(state))
        .merge(roster_routes(state))
        .merge(team_routes(state))
        .merge(usage_routes(state))
}

fn identity_routes(config: &Config, state: Option<&State>) -> Router {
    let offered = methods_router(SignInMethods {
        local_password: config.local_password && state.is_some(),
        oidc: config.oidc.is_some() && state.is_some(),
    });
    let Some(state) = state else {
        if config.local_password || config.oidc.is_some() {
            tracing::warn!("no control plane is configured, so nobody can sign in");
        }
        return offered;
    };
    offered
        .merge(me_router(state.clone()))
        .merge(session_router(state.clone()))
        .merge(password_routes(config, state))
        .merge(federated_routes(config, state))
        .merge(reset_routes(config, state))
}

fn password_routes(config: &Config, state: &State) -> Router {
    if !config.local_password {
        return Router::new();
    }
    tracing::info!("local passwords accepted");
    auth_router(state.clone()).merge(second_factor_router(state.clone()))
}

fn federated_routes(config: &Config, state: &State) -> Router {
    let Some(settings) = &config.oidc else {
        return Router::new();
    };
    tracing::info!(issuer = %settings.issuer, "federated sign in accepted");
    oidc_router(state.clone(), config.after_sign_in.clone())
}

fn reset_routes(config: &Config, state: &State) -> Router {
    if !config.local_password {
        if config.mail.is_some() {
            tracing::warn!("a mail relay is configured but local passwords are off");
        }
        return Router::new();
    }
    reset_router(state.clone()).merge(password_router(state.clone()))
}

fn explore_routes(state: Option<&State>, held: bool) -> Router {
    let (Some(state), true) = (state, held) else {
        tracing::warn!(
            "traces and logs cannot be read back: that needs both a control plane \
             to authorise the reader and a span store to read from"
        );
        return Router::new();
    };
    explore_router(state.clone()).merge(log_router(state.clone()))
}

fn issue_routes(state: Option<&State>, held: bool) -> Router {
    let (Some(state), true) = (state, held) else {
        tracing::warn!(
            "issues cannot be grouped: that needs both a control plane to authorise \
             the reader and a span store holding the exceptions to group"
        );
        return Router::new();
    };
    issue_router(state.clone())
}

fn catalogue_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| catalogue_router(state.clone()))
}

fn key_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| key_router(state.clone()))
}

fn org_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| org_router(state.clone()))
}

fn audit_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| audit_router(state.clone()))
}

fn export_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| export_router(state.clone()))
}

fn roster_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| roster_router(state.clone()))
}

fn team_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| team_router(state.clone()))
}

fn usage_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| usage_router(state.clone()))
}
