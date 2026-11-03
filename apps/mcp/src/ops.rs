use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use uuid::Uuid;

use crate::api::ApiClient;
use crate::fault::Fault;
use crate::input;
use crate::present::{self, MetricAsk, Rostered};
use crate::window::Window;

const DEFAULT_TRACES: u32 = 20;
const DEFAULT_LOGS: u32 = 50;

#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    pub token: &'a str,
    pub org: Uuid,
}

#[derive(Debug, Default)]
pub struct TraceSearch<'a> {
    pub service: Option<&'a str>,
    pub errors_only: bool,
    pub min_duration_ms: Option<u64>,
    pub window: Option<&'a str>,
    pub limit: Option<u32>,
}

#[derive(Debug, Default)]
pub struct LogSearch<'a> {
    pub service: Option<&'a str>,
    pub severity: Option<&'a str>,
    pub text: Option<&'a str>,
    pub namespace: Option<&'a str>,
    pub pod: Option<&'a str>,
    pub window: Option<&'a str>,
    pub limit: Option<u32>,
}

#[derive(Debug, Default)]
pub struct MetricQuery<'a> {
    pub name: &'a str,
    pub aggregation: Option<&'a str>,
    pub group_by: Option<&'a str>,
    pub service: Option<&'a str>,
    pub window: Option<&'a str>,
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

fn span(window: Window) -> Vec<(&'static str, String)> {
    let (from, to) = window.bounds(now_ms());
    vec![("from", from.to_string()), ("to", to.to_string())]
}

fn push(query: &mut Vec<(&'static str, String)>, key: &'static str, value: Option<&str>) {
    if let Some(value) = input::given(value) {
        query.push((key, value.to_owned()));
    }
}

#[derive(Debug, Clone)]
pub struct Backend {
    api: ApiClient,
}

impl Backend {
    pub fn new(api: ApiClient) -> Self {
        Self { api }
    }

    async fn get(
        &self,
        call: Call<'_>,
        tail: &[&str],
        query: &[(&str, String)],
    ) -> Result<Value, Fault> {
        let org = call.org.to_string();
        let mut segments = vec!["api", "orgs", org.as_str()];
        segments.extend_from_slice(tail);
        self.api.get(call.token, &segments, query).await
    }

    pub async fn list_services(
        &self,
        call: Call<'_>,
        window: Option<&str>,
    ) -> Result<String, Fault> {
        let window = Window::parse(window)?;
        let body = self.get(call, &["services"], &span(window)).await?;
        Ok(present::services(&body, window).render())
    }

    pub async fn service_health(
        &self,
        call: Call<'_>,
        service: &str,
        window: Option<&str>,
    ) -> Result<String, Fault> {
        let service = input::required("service", service)?;
        let window = Window::parse(window)?;
        let query = span(window);
        let listing = self.get(call, &["services"], &query).await?;
        let operations = self
            .get(call, &["services", service, "operations"], &query)
            .await?;
        Ok(present::service_health(&listing, service, &operations, window)?.render())
    }

    pub async fn search_traces(
        &self,
        call: Call<'_>,
        search: &TraceSearch<'_>,
    ) -> Result<String, Fault> {
        let window = Window::parse(search.window)?;
        let mut query = span(window);
        push(&mut query, "service", search.service);
        if search.errors_only {
            query.push(("errors", "true".to_owned()));
        }
        if let Some(ms) = search.min_duration_ms {
            query.push(("at_least_us", ms.saturating_mul(1_000).to_string()));
        }
        query.push((
            "limit",
            input::limit(search.limit, DEFAULT_TRACES).to_string(),
        ));
        let body = self.get(call, &["traces"], &query).await?;
        Ok(present::traces(&body, window).render())
    }

    pub async fn get_trace(&self, call: Call<'_>, trace_id: &str) -> Result<String, Fault> {
        let id = input::trace_id(trace_id)?;
        let body = self.get(call, &["traces", &id], &[]).await?;
        Ok(present::trace(&body).render())
    }

    pub async fn search_logs(
        &self,
        call: Call<'_>,
        search: &LogSearch<'_>,
    ) -> Result<String, Fault> {
        let window = Window::parse(search.window)?;
        let severity = input::choice("severity", search.severity, &input::LOG_SEVERITIES)?;
        let mut query = span(window);
        push(&mut query, "service", search.service);
        push(&mut query, "severity", severity);
        push(&mut query, "q", search.text);
        push(&mut query, "namespace", search.namespace);
        push(&mut query, "pod", search.pod);
        query.push((
            "limit",
            input::limit(search.limit, DEFAULT_LOGS).to_string(),
        ));
        let body = self.get(call, &["logs"], &query).await?;
        Ok(present::logs(&body, window).render())
    }

    pub async fn query_metric(
        &self,
        call: Call<'_>,
        metric: &MetricQuery<'_>,
    ) -> Result<String, Fault> {
        let name = input::required("name", metric.name)?;
        let window = Window::parse(metric.window)?;
        let aggregation = input::choice("aggregation", metric.aggregation, &input::AGGREGATIONS)?;
        let service = input::given(metric.service);
        let mut query = span(window);
        push(&mut query, "agg", aggregation);
        let mut groups = metric
            .group_by
            .map(|typed| {
                typed
                    .split(',')
                    .map(str::trim)
                    .filter(|key| !key.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if service.is_some() && !groups.iter().any(|key| key == "service.name") {
            groups.push("service.name".to_owned());
        }
        query.extend(groups.into_iter().map(|key| ("group_by", key)));
        let body = self.get(call, &["metrics", name, "series"], &query).await?;
        let ask = MetricAsk {
            name,
            aggregation,
            service,
            window,
        };
        Ok(present::metric(&body, &ask).render())
    }

    pub async fn cluster_status(
        &self,
        call: Call<'_>,
        cluster: Option<&str>,
        namespace: Option<&str>,
    ) -> Result<String, Fault> {
        let mut query = Vec::new();
        push(&mut query, "cluster", cluster);
        push(&mut query, "namespace", namespace);
        let body = self.get(call, &["cluster"], &query).await?;
        Ok(present::cluster(&body).render())
    }

    pub async fn list_alerts(&self, call: Call<'_>, state: Option<&str>) -> Result<String, Fault> {
        let state = input::choice("state", state, &input::ALERT_STATES)?;
        let body = self.get(call, &["alerts"], &[]).await?;
        Ok(present::alerts(&body, state).render())
    }

    pub async fn alert_history(
        &self,
        call: Call<'_>,
        alert_id: &str,
        window: Option<&str>,
    ) -> Result<String, Fault> {
        let id = input::uuid("alert_id", alert_id)?.to_string();
        let window = Window::parse(window)?;
        let body = self
            .get(call, &["alerts", &id, "history"], &span(window))
            .await?;
        Ok(present::alert_history(&body, window).render())
    }

    pub async fn list_incidents(
        &self,
        call: Call<'_>,
        status: Option<&str>,
    ) -> Result<String, Fault> {
        let status = input::choice("status", status, &input::INCIDENT_STATUSES)?.unwrap_or("open");
        let body = self
            .get(call, &["incidents"], &[("status", status.to_owned())])
            .await?;
        Ok(present::incidents(&body, status).render())
    }

    pub async fn get_incident(&self, call: Call<'_>, incident_id: &str) -> Result<String, Fault> {
        let id = input::uuid("incident_id", incident_id)?.to_string();
        let body = self.get(call, &["incidents", &id], &[]).await?;
        Ok(present::incident(&body).render())
    }

    pub async fn who_is_on_call(
        &self,
        call: Call<'_>,
        schedule: Option<&str>,
    ) -> Result<String, Fault> {
        let body = self.get(call, &["oncall", "now"], &[]).await?;
        let wanted = input::given(schedule);
        let rosters = body["schedules"]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|held| {
                wanted.is_none_or(|wanted| {
                    held["id"]
                        .as_str()
                        .is_some_and(|id| id.eq_ignore_ascii_case(wanted))
                        || held["name"]
                            .as_str()
                            .is_some_and(|name| name.eq_ignore_ascii_case(wanted))
                })
            })
            .map(|held| Rostered {
                schedule: held["name"].as_str().unwrap_or_default().to_owned(),
                person: held["on_call"].clone(),
                until_ms: held["until_ms"].as_u64(),
            })
            .collect::<Vec<_>>();
        if let (Some(wanted), true) = (wanted, rosters.is_empty()) {
            return Err(Fault::argument(format!(
                "no schedule has the name or id {wanted:?}; call who_is_on_call without a schedule to list them"
            )));
        }
        Ok(present::on_call(&rosters).render())
    }

    pub async fn comment_incident(
        &self,
        call: Call<'_>,
        incident_id: &str,
        text: &str,
    ) -> Result<String, Fault> {
        let id = input::uuid("incident_id", incident_id)?;
        let body = input::comment(text)?;
        let entry = self
            .write(
                call,
                Write::Post(Some(&json!({"body": body, "public": false}))),
                &["incidents", &id.to_string(), "comments"],
            )
            .await?;
        Ok(present::commented(&id.to_string(), &entry).render())
    }

    pub async fn resolve_incident(
        &self,
        call: Call<'_>,
        incident_id: &str,
    ) -> Result<String, Fault> {
        let id = input::uuid("incident_id", incident_id)?;
        let incident = self
            .write(
                call,
                Write::Patch(&json!({"status": "resolved"})),
                &["incidents", &id.to_string()],
            )
            .await?;
        Ok(present::resolved(&incident).render())
    }

    pub async fn acknowledge_page(&self, call: Call<'_>, page_id: &str) -> Result<String, Fault> {
        let id = input::uuid("page_id", page_id)?;
        let page = self
            .write(call, Write::Post(None), &["pages", &id.to_string(), "ack"])
            .await?;
        Ok(present::acknowledged(&page).render())
    }

    async fn write(&self, call: Call<'_>, how: Write<'_>, tail: &[&str]) -> Result<Value, Fault> {
        let org = call.org.to_string();
        let mut segments = vec!["api", "orgs", org.as_str()];
        segments.extend_from_slice(tail);
        match how {
            Write::Post(body) => self.api.post(call.token, &segments, body).await,
            Write::Patch(body) => self.api.patch(call.token, &segments, body).await,
        }
    }
}

enum Write<'a> {
    Post(Option<&'a Value>),
    Patch(&'a Value),
}
