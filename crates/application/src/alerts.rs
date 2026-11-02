use core::future::Future;
use std::collections::BTreeMap;

use ferristrace_domain::{
    Aggregation, AlertBook, AlertError, AlertEvent, Buckets, ChannelBook, ChannelId, ChannelName,
    ChannelView, Clock, Courier, Definition, Delivery, Destination, Drained, Evaluator, Followers,
    IssuedChannel, Labels, LogQuery, LogReader, LogSignal, MetricQuery, MetricReader, MetricSignal,
    Observation, Preview, ProbeError, ProjectId, Reader, Readers, RuleDraft, RuleId, RulePatch,
    ServiceFilter, SessionToken, Signal, SignalProbe, SpanReader, TenantId, Tick, TraceMeasure,
    TraceSignal, Watched, Whoami, Window,
};

use crate::registry::AlertRepos;
use crate::{Ferristrace, Stack, transactional};

const NANOS_PER_MILLI: f64 = 1_000_000.0;
const PERCENT: f64 = 100.0;

#[derive(Debug)]
pub struct SignalReaders<'a, R, L, M> {
    spans: Option<&'a R>,
    logs: Option<&'a L>,
    metrics: Option<&'a M>,
}

impl<'a, R, L, M> SignalReaders<'a, R, L, M> {
    pub fn new(spans: Option<&'a R>, logs: Option<&'a L>, metrics: Option<&'a M>) -> Self {
        Self {
            spans,
            logs,
            metrics,
        }
    }
}

impl<R, L, M> SignalProbe for SignalReaders<'_, R, L, M>
where
    R: SpanReader + Sync,
    L: LogReader + Sync,
    M: MetricReader + Sync,
{
    async fn observe(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        signal: &Signal,
        window: Window,
    ) -> Result<Vec<Observation>, ProbeError> {
        match signal {
            Signal::Traces(traces) => {
                let reader = self.spans.ok_or(ProbeError::Unavailable)?;
                let value = traced(reader, org, project, traces, window).await?;
                Ok(vec![unlabelled(value)])
            }
            Signal::Logs(logs) => {
                let reader = self.logs.ok_or(ProbeError::Unavailable)?;
                let value = logged(reader, org, project, logs, window).await?;
                Ok(vec![unlabelled(value)])
            }
            Signal::Metrics(metric) => {
                let reader = self.metrics.ok_or(ProbeError::Unavailable)?;
                measured(reader, org, project, metric, window).await
            }
        }
    }
}

fn unlabelled(value: Option<f64>) -> Observation {
    Observation {
        labels: Labels::none(),
        value,
    }
}

#[derive(Debug, Default)]
struct Measured {
    requests: u64,
    errors: u64,
    p50_nanos: Option<u64>,
    p95_nanos: Option<u64>,
    p99_nanos: Option<u64>,
}

async fn traced<R: SpanReader>(
    reader: &R,
    org: TenantId,
    project: Option<ProjectId>,
    traces: &TraceSignal,
    window: Window,
) -> Result<Option<f64>, ProbeError> {
    let filter = ServiceFilter {
        window,
        project,
        environment: traces.environment.clone(),
    };
    let measured = match (&traces.service, &traces.operation) {
        (Some(service), Some(operation)) => reader
            .operations(org, service, &filter)
            .await?
            .into_iter()
            .find(|summary| summary.name == *operation)
            .map(|summary| Measured {
                requests: summary.requests,
                errors: summary.errors,
                p50_nanos: Some(summary.p50_nanos),
                p95_nanos: Some(summary.p95_nanos),
                p99_nanos: Some(summary.p99_nanos),
            })
            .unwrap_or_default(),
        (service, _) => {
            let series = reader.series(org, service.as_deref(), &filter).await?;
            Measured {
                requests: series.requests,
                errors: series.errors,
                p50_nanos: series.p50_nanos,
                p95_nanos: series.p95_nanos,
                p99_nanos: series.p99_nanos,
            }
        }
    };
    let seen = measured.requests > 0;
    let millis = |nanos: Option<u64>| {
        nanos
            .filter(|_| seen)
            .map(|nanos| nanos as f64 / NANOS_PER_MILLI)
    };
    Ok(match traces.measure {
        TraceMeasure::Count => Some(measured.requests as f64),
        TraceMeasure::ErrorRate => {
            seen.then(|| measured.errors as f64 / measured.requests as f64 * PERCENT)
        }
        TraceMeasure::P50 => millis(measured.p50_nanos),
        TraceMeasure::P95 => millis(measured.p95_nanos),
        TraceMeasure::P99 => millis(measured.p99_nanos),
    })
}

async fn logged<L: LogReader>(
    reader: &L,
    org: TenantId,
    project: Option<ProjectId>,
    logs: &LogSignal,
    window: Window,
) -> Result<Option<f64>, ProbeError> {
    let mut query = LogQuery::over(window);
    query.services.clone_from(&logs.services);
    query.severities.clone_from(&logs.severities);
    query.text.clone_from(&logs.text);
    query.attributes.clone_from(&logs.attributes);
    query.project = project;
    let one = Buckets::try_from(1).map_err(|_| ProbeError::Unreachable)?;
    let count: u64 = reader
        .histogram(org, &query, one)
        .await?
        .iter()
        .map(ferristrace_domain::LogBucket::total)
        .sum();
    Ok(Some(count as f64))
}

async fn measured<M: MetricReader>(
    reader: &M,
    org: TenantId,
    project: Option<ProjectId>,
    metric: &MetricSignal,
    window: Window,
) -> Result<Vec<Observation>, ProbeError> {
    let Some(kind) = reader
        .catalogue(org, Some(&metric.name), project)
        .await?
        .into_iter()
        .find(|seen| seen.name == metric.name)
        .map(|seen| seen.kind)
    else {
        return Ok(Vec::new());
    };
    let mut query = MetricQuery::over(window, &metric.name);
    query.group_by.clone_from(&metric.group_by);
    for key in metric.filter.keys() {
        if !query.group_by.contains(key) {
            query.group_by.push(key.clone());
        }
    }
    query.aggregation = Some(metric.aggregation);
    query.project = project;
    let aggregation = query.resolved(kind)?;
    let set = reader.series(org, kind, aggregation, &query).await?;
    Ok(set
        .series
        .into_iter()
        .filter(|series| {
            metric
                .filter
                .iter()
                .all(|(key, value)| series.labels.get(key) == Some(value))
        })
        .map(|series| {
            let labels: BTreeMap<Box<str>, Box<str>> = series
                .labels
                .into_iter()
                .filter(|(key, _)| metric.group_by.contains(key))
                .collect();
            Observation {
                labels: Labels::new(labels),
                value: folded(
                    aggregation,
                    series.points.iter().filter_map(|point| point.1),
                ),
            }
        })
        .filter(|observation| observation.value.is_some())
        .collect())
}

fn folded(aggregation: Aggregation, values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<f64> = values.filter(|value| value.is_finite()).collect();
    if values.is_empty() {
        return None;
    }
    Some(match aggregation {
        Aggregation::Sum => values.iter().sum(),
        Aggregation::Min => values.iter().copied().fold(f64::INFINITY, f64::min),
        Aggregation::Max => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Aggregation::Avg
        | Aggregation::Rate
        | Aggregation::P50
        | Aggregation::P95
        | Aggregation::P99 => values.iter().sum::<f64>() / values.len() as f64,
    })
}

impl<S: Stack> Ferristrace<S>
where
    S::Repos: AlertRepos,
{
    fn signal_readers(&self) -> SignalReaders<'_, S::SpanStore, S::LogStore, S::MetricStore> {
        SignalReaders::new(
            self.span_store.as_ref(),
            self.log_store.as_ref(),
            self.metric_store.as_ref(),
        )
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn list_alerts(
        &self,
        who: &Reader,
        org: TenantId,
    ) -> Result<Vec<Watched>, AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .list(who, org)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn alert(
        &self,
        who: &Reader,
        org: TenantId,
        id: RuleId,
    ) -> Result<Watched, AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .get(who, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn alert_history(
        &self,
        who: &Reader,
        org: TenantId,
        id: RuleId,
        from_ms: Option<u64>,
        to_ms: Option<u64>,
    ) -> Result<Vec<AlertEvent>, AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .history(who, org, id, from_ms, to_ms, self.clock.now())
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn create_alert(
        &self,
        token: &SessionToken,
        org: TenantId,
        draft: &RuleDraft,
    ) -> Result<Watched, AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .create(token, org, draft)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn update_alert(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
        patch: RulePatch,
    ) -> Result<Watched, AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .update(token, org, id, patch)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn delete_alert(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
    ) -> Result<(), AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .delete(token, org, id)
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn preview_alert(
        &self,
        who: &Reader,
        org: TenantId,
        project: Option<ProjectId>,
        definition: &Definition,
    ) -> Result<Preview, AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .preview(
            who,
            org,
            project,
            definition,
            &self.signal_readers(),
            self.clock.now(),
        )
        .await
    }

    #[transactional(sessions, memberships, api_keys, projects, alert_rules, alert_states, alert_events, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn test_alert(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: RuleId,
    ) -> Result<Vec<Delivery>, AlertError> {
        AlertBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            projects,
            alert_rules,
            alert_states,
            alert_events,
            alert_channels,
            audit,
        )
        .rehearse(
            token,
            org,
            id,
            &self.mailer,
            &self.webhooks,
            self.clock.now(),
        )
        .await
    }

    #[transactional(sessions, memberships, api_keys, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn list_channels(
        &self,
        who: &Reader,
        org: TenantId,
    ) -> Result<Vec<ChannelView>, AlertError> {
        ChannelBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            alert_channels,
            audit,
        )
        .list(who, org)
        .await
    }

    #[transactional(sessions, memberships, api_keys, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn create_channel(
        &self,
        token: &SessionToken,
        org: TenantId,
        name: &ChannelName,
        destination: &Destination,
    ) -> Result<IssuedChannel, AlertError> {
        ChannelBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            alert_channels,
            audit,
        )
        .create(token, org, name, destination, &self.webhooks)
        .await
    }

    #[transactional(sessions, memberships, api_keys, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn update_channel(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ChannelId,
        name: Option<&ChannelName>,
        target: Option<&str>,
    ) -> Result<ChannelView, AlertError> {
        ChannelBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            alert_channels,
            audit,
        )
        .update(token, org, id, name, target, &self.webhooks)
        .await
    }

    #[transactional(sessions, memberships, api_keys, alert_channels, audit, unavailable = AlertError::StoreUnreachable)]
    pub async fn delete_channel(
        &self,
        token: &SessionToken,
        org: TenantId,
        id: ChannelId,
    ) -> Result<(), AlertError> {
        ChannelBook::new(
            Readers::new(Whoami::new(sessions, memberships), api_keys),
            alert_channels,
            audit,
        )
        .delete(token, org, id)
        .await
    }

    #[allow(clippy::manual_async_fn)]
    pub fn evaluate_alerts(&self) -> impl Future<Output = Result<Tick, AlertError>> + Send {
        async move { self.evaluate_once().await }
    }

    #[allow(clippy::manual_async_fn)]
    pub fn deliver_alerts(&self) -> impl Future<Output = Result<Drained, AlertError>> + Send {
        async move { self.deliver_once().await }
    }

    #[transactional(alert_rules, alert_states, alert_events, alert_outbox, alert_duty, incidents, incident_timeline, unavailable = AlertError::StoreUnreachable)]
    async fn evaluate_once(&self) -> Result<Tick, AlertError> {
        Evaluator::new(
            alert_rules,
            alert_states,
            alert_events,
            alert_outbox,
            alert_duty,
            self.signal_readers(),
            Followers::new(incidents, incident_timeline),
        )
        .tick(self.clock.now())
        .await
    }

    #[transactional(alert_outbox, alert_duty, unavailable = AlertError::StoreUnreachable)]
    async fn deliver_once(&self) -> Result<Drained, AlertError> {
        Courier::new(alert_outbox, alert_duty, &self.mailer, &self.webhooks)
            .drain(self.clock.now())
            .await
    }
}
