use axum::Router;
use ferristrace_api::{
    AppState, SignInMethods, agent_router, alert_router, audit_router, catalogue_router,
    cluster_router, explore_router, export_router, incident_router, issue_router, key_router,
    llm_router, log_router, mark_routes, mcp_router, me_router, methods_router, metric_router,
    oncall_router, org_router, postmortem_router, release_router, roster_router, site_router,
    status_router, team_router, usage_router, view_router, workspace_router,
};

use crate::adapters::Production;
use crate::config::Config;

pub(crate) type State = AppState<Production>;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Held {
    pub(crate) explore: bool,
    pub(crate) issues: bool,
}

pub(crate) fn control_routes(config: &Config, state: Option<&State>, held: Held) -> Router {
    mark_routes(
        identity_routes(config, state)
            .merge(explore_routes(state, held.explore))
            .merge(issue_routes(state, held.issues))
            .merge(catalogue_routes(state))
            .merge(key_routes(state))
            .merge(org_routes(state))
            .merge(audit_routes(state))
            .merge(alert_routes(state))
            .merge(incident_routes(state))
            .merge(oncall_routes(state))
            .merge(postmortem_routes(state))
            .merge(status_routes(state))
            .merge(export_routes(state))
            .merge(roster_routes(state))
            .merge(team_routes(state))
            .merge(view_routes(state))
            .merge(usage_routes(state))
            .merge(agent_routes(state))
            .merge(mcp_routes(state)),
    )
}

fn identity_routes(config: &Config, state: Option<&State>) -> Router {
    let offered = methods_router(sign_in_methods(config, state.is_some()));
    let Some(state) = state else {
        if config.oidc.is_some() {
            tracing::warn!("no control plane is configured, so nobody can sign in");
        }
        return offered;
    };
    offered
        .merge(me_router(state.clone()))
        .merge(workspace_router(state.clone()))
}

fn sign_in_methods(config: &Config, control_plane: bool) -> SignInMethods {
    match &config.oidc {
        Some(oidc) if control_plane => SignInMethods::oidc(&oidc.issuer, &oidc.client_id),
        _ => SignInMethods::off(),
    }
}

fn explore_routes(state: Option<&State>, held: bool) -> Router {
    let (Some(state), true) = (state, held) else {
        tracing::warn!(
            "traces, logs and metrics cannot be read back: that needs both a control plane \
             to authorise the reader and a span store to read from"
        );
        return Router::new();
    };
    explore_router(state.clone())
        .merge(log_router(state.clone()))
        .merge(metric_router(state.clone()))
        .merge(cluster_router(state.clone()))
        .merge(site_router(state.clone()))
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
        .merge(release_router(state.clone()))
        .merge(llm_router(state.clone()))
}

fn catalogue_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| catalogue_router(state.clone()))
}

fn key_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| key_router(state.clone()))
}

fn agent_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| agent_router(state.clone()))
}

fn mcp_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| mcp_router(state.clone()))
}

fn org_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| org_router(state.clone()))
}

fn audit_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| audit_router(state.clone()))
}

fn alert_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| alert_router(state.clone()))
}

fn incident_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| incident_router(state.clone()))
}

fn oncall_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| oncall_router(state.clone()))
}

fn postmortem_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| postmortem_router(state.clone()))
}

fn status_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| status_router(state.clone()))
}

fn export_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| export_router(state.clone()))
}

fn roster_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| roster_router(state.clone()))
}

fn view_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| view_router(state.clone()))
}

fn team_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| team_router(state.clone()))
}

fn usage_routes(state: Option<&State>) -> Router {
    state.map_or_else(Router::new, |state| usage_router(state.clone()))
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;

    use super::{Held, control_routes, sign_in_methods};
    use crate::config::{Config, OidcConfig};

    fn with_provider() -> Config {
        Config {
            oidc: Some(OidcConfig {
                issuer: "https://id.example/realms/ferristrace".to_owned(),
                client_id: "ferristrace-web".to_owned(),
            }),
            ..Config::default()
        }
    }

    async fn answer(config: &Config, method: Method, uri: &str) -> (StatusCode, String) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("a request");
        let response = control_routes(
            config,
            None,
            Held {
                explore: false,
                issues: false,
            },
        )
        .oneshot(request)
        .await
        .expect("a response");
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("a body");
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    #[tokio::test]
    async fn without_a_provider_the_server_says_nobody_can_sign_in() {
        let (status, body) = answer(&Config::default(), Method::GET, "/auth/methods").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"oidc":false,"issuer":null,"client_id":null}"#);
    }

    #[tokio::test]
    async fn a_provider_without_a_control_plane_offers_no_way_in_either() {
        let (status, body) = answer(&with_provider(), Method::GET, "/auth/methods").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"oidc":false,"issuer":null,"client_id":null}"#);
    }

    #[test]
    fn a_provider_with_a_control_plane_names_its_issuer_and_the_web_client() {
        let methods = sign_in_methods(&with_provider(), true);

        assert!(methods.oidc);
        assert_eq!(
            methods.issuer.as_deref(),
            Some("https://id.example/realms/ferristrace")
        );
        assert_eq!(methods.client_id.as_deref(), Some("ferristrace-web"));
    }

    #[tokio::test]
    async fn the_removed_routes_answer_not_found_whatever_the_configuration() {
        for config in [Config::default(), with_provider()] {
            for (method, uri) in [
                (Method::POST, "/auth/sign-in"),
                (Method::POST, "/auth/password"),
                (Method::POST, "/auth/reset/request"),
                (Method::POST, "/auth/reset/confirm"),
                (Method::GET, "/auth/2fa"),
                (Method::POST, "/auth/2fa/verify"),
                (Method::GET, "/auth/oidc/start"),
                (Method::GET, "/auth/oidc/callback"),
                (Method::POST, "/auth/sign-out"),
                (Method::GET, "/auth/sessions"),
                (
                    Method::DELETE,
                    "/auth/sessions/6f1d1f0e-0000-4000-8000-000000000001",
                ),
            ] {
                let (status, _) = answer(&config, method.clone(), uri).await;

                assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}");
            }
        }
    }
}
