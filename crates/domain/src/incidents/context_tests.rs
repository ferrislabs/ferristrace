use core::future::Future;
use std::sync::Mutex;

use uuid::Uuid;

use super::context::{MAX_DEPLOYS, Surroundings};
use super::entities::Incident;
use super::value_objects::{IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle};
use crate::catalogue::ProjectId;
use crate::explore::{
    ServiceFilter, ServiceSeries, ServiceSummary, SpanReader, TraceFilter, TraceSummary, Window,
};
use crate::infrastructure::{InfraFilter, InfraReader, Resource, ResourceKind};
use crate::issue::{Counted, Fingerprint, IssueFilter, IssueReader, Occurrence};
use crate::releases::{Deploy, Release, ReleaseDetail, ReleaseFilter, ReleaseReader};
use crate::store::StoreError;
use crate::telemetry::{Span, TenantId, TraceId};

const MILLI: u64 = 1_000_000;
const OPENED_MS: u64 = 10_000_000_000;
const NOW_MS: u64 = OPENED_MS + 600_000;

fn org() -> TenantId {
    TenantId::new(Uuid::from_u128(1))
}

fn incident(project: Option<ProjectId>) -> Incident {
    Incident {
        id: IncidentId::new(Uuid::from_u128(5)),
        org: org(),
        project,
        title: IncidentTitle::try_from("Checkout errors").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: IncidentStatus::Investigating,
        owner: None,
        public: false,
        opened_ms: OPENED_MS,
        resolved_ms: None,
        opened_from_alert: None,
        paged_schedule: None,
    }
}

fn service(name: &str) -> Vec<Box<str>> {
    vec![name.into()]
}

type Asked = (Option<Box<str>>, Option<ProjectId>);

#[derive(Default)]
struct Releases {
    deploys: Vec<Deploy>,
    asked: Mutex<Vec<Asked>>,
}

impl ReleaseReader for Releases {
    async fn releases(
        &self,
        _org: TenantId,
        _filter: &ReleaseFilter,
    ) -> Result<Vec<Release>, StoreError> {
        Ok(Vec::new())
    }

    async fn release(
        &self,
        _org: TenantId,
        _service: &str,
        _version: &str,
        _project: Option<ProjectId>,
    ) -> Result<Option<ReleaseDetail>, StoreError> {
        Ok(None)
    }

    async fn deploys(
        &self,
        _org: TenantId,
        filter: &ReleaseFilter,
    ) -> Result<Vec<Deploy>, StoreError> {
        self.asked
            .lock()
            .expect("the lock")
            .push((filter.service.clone(), filter.project));
        Ok(self
            .deploys
            .iter()
            .filter(|deploy| filter.service.as_deref() == Some(&*deploy.service))
            .cloned()
            .collect())
    }
}

#[derive(Default)]
struct Issues(Vec<Counted>);

impl IssueReader for Issues {
    async fn issues(
        &self,
        _tenant: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Counted>, StoreError> {
        Ok(self
            .0
            .iter()
            .filter(|issue| filter.service.as_deref() == Some(&*issue.service))
            .cloned()
            .collect())
    }

    async fn occurrences(
        &self,
        _tenant: TenantId,
        _project: ProjectId,
        _print: Fingerprint,
        _window: Window,
        _limit: u32,
    ) -> Result<Vec<Occurrence>, StoreError> {
        Ok(Vec::new())
    }
}

#[derive(Default)]
struct Spans(Vec<TraceSummary>);

impl SpanReader for Spans {
    async fn services(
        &self,
        _tenant: TenantId,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn operations(
        &self,
        _tenant: TenantId,
        _service: &str,
        _filter: &ServiceFilter,
    ) -> Result<Vec<ServiceSummary>, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn series(
        &self,
        _tenant: TenantId,
        _service: Option<&str>,
        _filter: &ServiceFilter,
    ) -> Result<ServiceSeries, StoreError> {
        Err(StoreError::Unreachable)
    }

    async fn traces(
        &self,
        _tenant: TenantId,
        filter: &TraceFilter,
    ) -> Result<Vec<TraceSummary>, StoreError> {
        assert!(filter.only_errors);
        Ok(self
            .0
            .iter()
            .filter(|trace| filter.service.as_deref() == Some(&*trace.root_service))
            .cloned()
            .collect())
    }

    async fn trace(
        &self,
        _tenant: TenantId,
        _id: TraceId,
        _project: Option<ProjectId>,
    ) -> Result<Vec<Span>, StoreError> {
        Err(StoreError::Unreachable)
    }
}

#[derive(Default)]
struct Infra(Vec<Resource>);

impl InfraReader for Infra {
    fn resources(
        &self,
        _org: TenantId,
        _figures: Window,
        _filter: &InfraFilter,
    ) -> impl Future<Output = Result<Vec<Resource>, StoreError>> + Send {
        let found = self.0.clone();
        async move { Ok(found) }
    }
}

fn deploy(service: &str, version: &str, at_ms: u64) -> Deploy {
    Deploy {
        service: service.into(),
        version: version.into(),
        environment: "prod".into(),
        at_unix_nanos: at_ms * MILLI,
    }
}

fn issue(service: &str, first_seen_ms: u64, seed: u8) -> Counted {
    Counted {
        project: ProjectId::new(Uuid::from_u128(7)),
        print: Fingerprint::new([seed; crate::issue::PRINT_BYTES]),
        exception_type: "TimeoutError".into(),
        message: "upstream".into(),
        service: service.into(),
        count: 3,
        first_seen_unix_nanos: first_seen_ms * MILLI,
        last_seen_unix_nanos: first_seen_ms * MILLI,
    }
}

fn trace(service: &str, duration_ms: u64, seed: u8) -> TraceSummary {
    TraceSummary {
        trace_id: TraceId::try_from([seed; 16].as_slice()).expect("a trace"),
        root_service: service.into(),
        root_name: "POST /pay".into(),
        status_code: Some(500),
        started_unix_nanos: (OPENED_MS + 1_000) * MILLI,
        duration_nanos: duration_ms * MILLI,
        spans: 4,
        errors: 1,
    }
}

fn host(name: &str, serving: &[&str], last_seen_ms: u64) -> Resource {
    Resource {
        kind: ResourceKind::Host,
        id: name.into(),
        name: name.into(),
        cluster: "".into(),
        environment: "".into(),
        provider: "".into(),
        region: "".into(),
        zone: "".into(),
        host_type: "".into(),
        first_seen_unix_nanos: 0,
        last_seen_unix_nanos: last_seen_ms * MILLI,
        silent: false,
        cpu_percent: Some(40.0),
        memory_bytes: Some(1_000),
        services: serving.iter().map(|name| (*name).into()).collect(),
    }
}

#[tokio::test]
async fn the_deploys_of_the_concerned_service_inside_the_window_appear_newest_first() {
    let releases = Releases {
        deploys: vec![
            deploy("checkout", "1.0", OPENED_MS - 1_000),
            deploy("checkout", "1.1", OPENED_MS - 500),
            deploy("payments", "9.9", OPENED_MS - 100),
        ],
        ..Releases::default()
    };
    let (issues, spans, infra) = (Issues::default(), Spans::default(), Infra::default());

    let found = Surroundings::new(&releases, &issues, &spans, &infra)
        .around(&incident(None), &service("checkout"), NOW_MS)
        .await
        .expect("a context");

    let versions: Vec<&str> = found.deploys.iter().map(|d| &*d.version).collect();
    assert_eq!(versions, vec!["1.1", "1.0"]);
}

#[tokio::test]
async fn only_an_issue_first_seen_inside_the_window_is_new() {
    let issues = Issues(vec![
        issue("checkout", OPENED_MS - 1_000, 1),
        issue("checkout", OPENED_MS - 3 * 60 * 60 * 1_000, 2),
    ]);
    let (releases, spans, infra) = (Releases::default(), Spans::default(), Infra::default());

    let found = Surroundings::new(&releases, &issues, &spans, &infra)
        .around(&incident(None), &service("checkout"), NOW_MS)
        .await
        .expect("a context");

    assert_eq!(found.issues.len(), 1);
    assert_eq!(
        found.issues[0].first_seen_unix_nanos,
        (OPENED_MS - 1_000) * MILLI
    );
}

#[tokio::test]
async fn a_resource_that_served_the_service_appears_and_a_silent_one_carries_no_figures() {
    let infra = Infra(vec![
        host("web-1", &["checkout"], NOW_MS),
        host("old-1", &["checkout"], NOW_MS - 3_600_000),
        host("other", &["payments"], NOW_MS),
    ]);
    let (releases, issues, spans) = (Releases::default(), Issues::default(), Spans::default());

    let found = Surroundings::new(&releases, &issues, &spans, &infra)
        .around(&incident(None), &service("checkout"), NOW_MS)
        .await
        .expect("a context");

    let names: Vec<&str> = found.resources.iter().map(|r| &*r.name).collect();
    assert_eq!(names, vec!["old-1", "web-1"]);
    assert!(found.resources[0].silent);
    assert_eq!(found.resources[0].cpu_percent, None);
    assert_eq!(found.resources[0].memory_bytes, None);
    assert_eq!(found.resources[1].cpu_percent, Some(40.0));
}

#[tokio::test]
async fn the_slowest_failing_traces_come_first() {
    let spans = Spans(vec![
        trace("checkout", 100, 1),
        trace("checkout", 900, 2),
        trace("payments", 5_000, 3),
    ]);
    let (releases, issues, infra) = (Releases::default(), Issues::default(), Infra::default());

    let found = Surroundings::new(&releases, &issues, &spans, &infra)
        .around(&incident(None), &service("checkout"), NOW_MS)
        .await
        .expect("a context");

    let durations: Vec<u64> = found
        .traces
        .iter()
        .map(|t| t.duration_nanos / MILLI)
        .collect();
    assert_eq!(durations, vec![900, 100]);
}

#[tokio::test]
async fn no_concerned_service_reads_nothing() {
    let releases = Releases {
        deploys: vec![deploy("checkout", "1.0", OPENED_MS - 1)],
        ..Releases::default()
    };
    let (issues, spans, infra) = (Issues::default(), Spans::default(), Infra::default());

    let found = Surroundings::new(&releases, &issues, &spans, &infra)
        .around(&incident(None), &[], NOW_MS)
        .await
        .expect("a context");

    assert!(found.deploys.is_empty() && found.issues.is_empty());
    assert!(found.resources.is_empty() && found.traces.is_empty());
    assert!(releases.asked.lock().expect("the lock").is_empty());
}

#[tokio::test]
async fn the_incidents_project_narrows_every_read() {
    let project = ProjectId::new(Uuid::from_u128(30));
    let releases = Releases::default();
    let (issues, spans, infra) = (Issues::default(), Spans::default(), Infra::default());

    Surroundings::new(&releases, &issues, &spans, &infra)
        .around(&incident(Some(project)), &service("checkout"), NOW_MS)
        .await
        .expect("a context");

    assert_eq!(
        *releases.asked.lock().expect("the lock"),
        vec![(Some("checkout".into()), Some(project))]
    );
}

#[tokio::test]
async fn more_deploys_than_the_cap_are_cut_and_counted() {
    let releases = Releases {
        deploys: (0..MAX_DEPLOYS as u64 + 3)
            .map(|n| deploy("checkout", "v", OPENED_MS - 1_000 + n))
            .collect(),
        ..Releases::default()
    };
    let (issues, spans, infra) = (Issues::default(), Spans::default(), Infra::default());

    let found = Surroundings::new(&releases, &issues, &spans, &infra)
        .around(&incident(None), &service("checkout"), NOW_MS)
        .await
        .expect("a context");

    assert_eq!(found.deploys.len(), MAX_DEPLOYS);
    assert_eq!(found.left_out.deploys, 3);
}
