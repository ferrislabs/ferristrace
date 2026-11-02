use std::sync::Arc;

use axum::http::request::Parts;
use rmcp::{
    ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::Extension, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::Deserialize;

use crate::app::{Organisation, bearer};
use crate::fault::Fault;
use crate::ops::{Backend, Call, LogSearch, MetricQuery, TraceSearch};

pub const TOOL_NAMES: [&str; 15] = [
    "list_services",
    "service_health",
    "search_traces",
    "get_trace",
    "search_logs",
    "query_metric",
    "cluster_status",
    "list_alerts",
    "alert_history",
    "list_incidents",
    "get_incident",
    "who_is_on_call",
    "comment_incident",
    "resolve_incident",
    "acknowledge_page",
];

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct WindowArgs {
    #[schemars(description = "How far back to look: 15m, 1h, 24h or 7d. Default 1h.")]
    pub window: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ServiceHealthArgs {
    #[schemars(description = "Name of the service, as shown by list_services.")]
    pub service: String,
    #[schemars(description = "How far back to look: 15m, 1h, 24h or 7d. Default 1h.")]
    pub window: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchTracesArgs {
    #[schemars(description = "Only traces whose root span belongs to this service.")]
    pub service: Option<String>,
    #[schemars(description = "When true, only traces that contain at least one error span.")]
    pub errors_only: Option<bool>,
    #[schemars(description = "Only traces at least this long, in milliseconds.")]
    pub min_duration_ms: Option<u64>,
    #[schemars(description = "How far back to look: 15m, 1h, 24h or 7d. Default 1h.")]
    pub window: Option<String>,
    #[schemars(description = "Maximum traces to return, 1 to 200. Default 20.")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct TraceArgs {
    #[schemars(
        description = "The trace identifier: 32 hexadecimal characters, as shown by search_traces."
    )]
    pub trace_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct SearchLogsArgs {
    #[schemars(description = "Only log lines emitted by this service.")]
    pub service: Option<String>,
    #[schemars(
        description = "Only lines of exactly this level: trace, debug, info, warn, error or fatal. Omit for every level."
    )]
    pub severity: Option<String>,
    #[schemars(description = "Free text that the message of a line must contain.")]
    pub text: Option<String>,
    #[schemars(description = "Only lines from this Kubernetes namespace.")]
    pub namespace: Option<String>,
    #[schemars(description = "Only lines from this Kubernetes pod.")]
    pub pod: Option<String>,
    #[schemars(description = "How far back to look: 15m, 1h, 24h or 7d. Default 1h.")]
    pub window: Option<String>,
    #[schemars(description = "Maximum lines to return, newest first, 1 to 200. Default 50.")]
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct QueryMetricArgs {
    #[schemars(description = "Metric name, for example http.server.request.duration.")]
    pub name: String,
    #[schemars(
        description = "How to combine values: sum, avg, min, max, rate, p50, p95 or p99. Omit for the metric's default."
    )]
    pub aggregation: Option<String>,
    #[schemars(
        description = "Label keys to split the series by, comma separated, for example service.name,http.route."
    )]
    pub group_by: Option<String>,
    #[schemars(
        description = "Keep only the series of this service (matched on the service.name label)."
    )]
    pub service: Option<String>,
    #[schemars(description = "How far back to look: 15m, 1h, 24h or 7d. Default 1h.")]
    pub window: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct ClusterArgs {
    #[schemars(description = "Only this Kubernetes cluster.")]
    pub cluster: Option<String>,
    #[schemars(description = "Only this Kubernetes namespace.")]
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct AlertsArgs {
    #[schemars(
        description = "Only rules in this state: ok, pending, firing or no_data. Omit for every rule."
    )]
    pub state: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct AlertHistoryArgs {
    #[schemars(description = "UUID of the alert rule, as shown by list_alerts.")]
    pub alert_id: String,
    #[schemars(description = "How far back to look: 15m, 1h, 24h or 7d. Default 1h.")]
    pub window: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct IncidentsArgs {
    #[schemars(description = "open, resolved or all. Default open.")]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct IncidentArgs {
    #[schemars(description = "UUID of the incident, as shown by list_incidents.")]
    pub incident_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct OnCallArgs {
    #[schemars(
        description = "Schedule id (UUID) or name. Omit to list every schedule that has paged someone."
    )]
    pub schedule: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct CommentArgs {
    #[schemars(description = "UUID of the incident, as shown by list_incidents.")]
    pub incident_id: String,
    #[schemars(description = "The comment, plain text, at most 10000 characters.")]
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[schemars(crate = "rmcp::schemars")]
pub struct PageArgs {
    #[schemars(description = "UUID of the page, as shown by get_incident or the on-call pages.")]
    pub page_id: String,
}

#[derive(Debug, Clone)]
pub struct FerristraceTools {
    backend: Arc<Backend>,
    tool_router: ToolRouter<Self>,
}

fn finish(outcome: Result<String, Fault>) -> CallToolResult {
    match outcome {
        Ok(text) => CallToolResult::success(vec![ContentBlock::text(text)]),
        Err(fault) => CallToolResult::error(vec![ContentBlock::text(fault.to_string())]),
    }
}

fn call(parts: &Parts) -> Result<Call<'_>, Fault> {
    let token = bearer(&parts.headers).ok_or(Fault::NoToken)?;
    let org = parts
        .extensions
        .get::<Organisation>()
        .ok_or(Fault::NoOrganisation)?;
    Ok(Call { token, org: org.0 })
}

impl FerristraceTools {
    pub fn new(backend: Arc<Backend>) -> Self {
        Self {
            backend,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl FerristraceTools {
    #[tool(
        description = "List the services that reported traces in a window, with request count, errors, error rate, throughput and p50/p95/p99 latency. Start here to learn service names.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_services(
        &self,
        Parameters(args): Parameters<WindowArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .list_services(call, args.window.as_deref())
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Health of one service over a window: requests, errors, error rate, latency percentiles, and the operations with the most errors.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn service_health(
        &self,
        Parameters(args): Parameters<ServiceHealthArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .service_health(call, &args.service, args.window.as_deref())
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Search traces, newest first. Returns one row per trace with its id, root span, duration, span count and error count. Use get_trace on an id to see its spans.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn search_traces(
        &self,
        Parameters(args): Parameters<SearchTracesArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .search_traces(
                        call,
                        &TraceSearch {
                            service: args.service.as_deref(),
                            errors_only: args.errors_only.unwrap_or(false),
                            min_duration_ms: args.min_duration_ms,
                            window: args.window.as_deref(),
                            limit: args.limit,
                        },
                    )
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Get one trace as a list of spans ordered by start time, with service, operation, status, timing, parent and the error message of failed spans.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_trace(
        &self,
        Parameters(args): Parameters<TraceArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => self.backend.get_trace(call, &args.trace_id).await,
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Search log lines, newest first, by service, level, text, Kubernetes namespace or pod. Each row has the time, level, service, message and trace id.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn search_logs(
        &self,
        Parameters(args): Parameters<SearchLogsArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .search_logs(
                        call,
                        &LogSearch {
                            service: args.service.as_deref(),
                            severity: args.severity.as_deref(),
                            text: args.text.as_deref(),
                            namespace: args.namespace.as_deref(),
                            pod: args.pod.as_deref(),
                            window: args.window.as_deref(),
                            limit: args.limit,
                        },
                    )
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Query one metric over a window and get a summary per series: number of points and the last, min, max and average values.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn query_metric(
        &self,
        Parameters(args): Parameters<QueryMetricArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .query_metric(
                        call,
                        &MetricQuery {
                            name: &args.name,
                            aggregation: args.aggregation.as_deref(),
                            group_by: args.group_by.as_deref(),
                            service: args.service.as_deref(),
                            window: args.window.as_deref(),
                        },
                    )
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Kubernetes status: clusters, namespaces, node readiness, the unhealthy pods and the workloads running fewer replicas than desired.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn cluster_status(
        &self,
        Parameters(args): Parameters<ClusterArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .cluster_status(call, args.cluster.as_deref(), args.namespace.as_deref())
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "List the alert rules with their current state, severity, value and when the state began.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_alerts(
        &self,
        Parameters(args): Parameters<AlertsArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => self.backend.list_alerts(call, args.state.as_deref()).await,
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "State transitions of one alert rule over a window, newest first, with the value and labels at each change.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn alert_history(
        &self,
        Parameters(args): Parameters<AlertHistoryArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .alert_history(call, &args.alert_id, args.window.as_deref())
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "List incidents with severity, status, title, affected services, opening time and owner. Defaults to the open ones.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_incidents(
        &self,
        Parameters(args): Parameters<IncidentsArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .list_incidents(call, args.status.as_deref())
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Get one incident: its facts, attachments and the full timeline of comments, status changes, pages and alerts, with who did each and on whose behalf.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_incident(
        &self,
        Parameters(args): Parameters<IncidentArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => self.backend.get_incident(call, &args.incident_id).await,
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Who is on call right now, per on-call schedule, and until when. Give a schedule id or name, or nothing to see every schedule that has paged someone.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn who_is_on_call(
        &self,
        Parameters(args): Parameters<OnCallArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .who_is_on_call(call, args.schedule.as_deref())
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Add a private comment to an incident's timeline. It is recorded under the caller's name.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn comment_incident(
        &self,
        Parameters(args): Parameters<CommentArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => {
                self.backend
                    .comment_incident(call, &args.incident_id, &args.text)
                    .await
            }
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Mark an incident as resolved. This closes it for everyone; only do it when the user has said the incident is over.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            open_world_hint = false
        )
    )]
    async fn resolve_incident(
        &self,
        Parameters(args): Parameters<IncidentArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => self.backend.resolve_incident(call, &args.incident_id).await,
            Err(fault) => Err(fault),
        })
    }

    #[tool(
        description = "Acknowledge an on-call page so that it stops escalating to the next person.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            open_world_hint = false
        )
    )]
    async fn acknowledge_page(
        &self,
        Parameters(args): Parameters<PageArgs>,
        Extension(parts): Extension<Parts>,
    ) -> CallToolResult {
        finish(match call(&parts) {
            Ok(call) => self.backend.acknowledge_page(call, &args.page_id).await,
            Err(fault) => Err(fault),
        })
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FerristraceTools {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "FerrisTrace tools for an on-call engineer. Start with list_services, list_alerts or list_incidents to learn the names and ids the other tools take. Every answer is capped at 200 rows and 64 KiB and says when it was cut.",
        )
    }
}
