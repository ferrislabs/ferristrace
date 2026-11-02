use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::header::COOKIE;
use axum::http::{Request, StatusCode};
use clickhouse::Client;
use ferristrace_api::{AppState, SessionCookie, explore_router};
use ferristrace_application::registry::{RepoFor, domain};
use ferristrace_application::testing::{Inert, Untransacted};
use ferristrace_application::{Adapters, Ferristrace, Stack};
use ferristrace_clickhouse::{BatchLimits, ClickhouseMigrator, ClickhouseReader, ClickhouseSink};
use ferristrace_domain::{
    AccountId, Agent, LiveSession, Membership, Memberships, NoPhotos, OrgRole, ProjectId,
    ProvisionPolicy, Scope, ServiceFilter, ServicePoint, Session, SessionError, SessionId,
    SessionRoll, SessionStore, SessionToken, Span, SpanId, SpanKind, SpanReader, SpanSink,
    SpanStatus, TenantId, TraceId, UnixSeconds, WhoamiError, Window,
};
use ferristrace_infra::FixedClock;
use ferristrace_otlp::spans_of;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};
use tower::ServiceExt;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MILLI: u64 = 1_000_000;
const MINUTE: u64 = 60 * SECOND;
const HOUR: u64 = 60 * MINUTE;
const TOKEN: &str = "ft_ss_views";
const MINE: u128 = 1;
const THEIRS: u128 = 2;

fn connected(url: &str) -> Client {
    let mut client = Client::default().with_url(url);
    if let Ok(user) = std::env::var("CLICKHOUSE_USER") {
        client = client.with_user(user);
    }
    if let Ok(password) = std::env::var("CLICKHOUSE_PASSWORD") {
        client = client.with_password(password);
    }
    client
}

async fn prepared(name: &str) -> Option<Client> {
    let url = backend::clickhouse_url()?;
    let root = connected(&url);
    root.query(&format!("DROP DATABASE IF EXISTS {name}"))
        .execute()
        .await
        .expect("dropping a leftover database");
    root.query(&format!("CREATE DATABASE {name}"))
        .execute()
        .await
        .expect("creating the test database");
    let client = connected(&url).with_database(name);
    ClickhouseMigrator::new(client.clone())
        .up()
        .await
        .expect("the migrations apply");
    Some(client)
}

fn org(seed: u128) -> TenantId {
    TenantId::new(Uuid::from_u128(seed))
}

fn project(seed: u128) -> ProjectId {
    ProjectId::new(Uuid::from_u128(seed))
}

struct Sent {
    seed: u32,
    service: &'static str,
    name: &'static str,
    at: u64,
    took: u64,
    failed: bool,
    kind: SpanKind,
    environment: Option<&'static str>,
}

impl Sent {
    fn served(seed: u32, service: &'static str, name: &'static str, at: u64, took: u64) -> Self {
        Self {
            seed,
            service,
            name,
            at,
            took,
            failed: false,
            kind: SpanKind::Server,
            environment: None,
        }
    }

    fn failing(mut self) -> Self {
        self.failed = true;
        self
    }

    fn of_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    fn in_environment(mut self, environment: &'static str) -> Self {
        self.environment = Some(environment);
        self
    }

    fn span(&self) -> Span {
        let id = self.seed.to_be_bytes();
        let mut trace = [9u8; 16];
        trace[12..].copy_from_slice(&id);
        let mut span = [7u8; 8];
        span[4..].copy_from_slice(&id);
        let mut attributes = BTreeMap::new();
        if let Some(environment) = self.environment {
            attributes.insert("deployment.environment".into(), environment.into());
        }
        Span {
            trace_id: TraceId::try_from(trace.as_slice()).expect("sixteen bytes"),
            span_id: SpanId::try_from(span.as_slice()).expect("eight bytes"),
            parent: None,
            name: self.name.into(),
            service_name: self.service.into(),
            kind: self.kind,
            status: if self.failed {
                SpanStatus::Error
            } else {
                SpanStatus::Ok
            },
            start_unix_nanos: BASE + self.at,
            end_unix_nanos: BASE + self.at + self.took,
            attributes,
            events: Vec::new(),
        }
    }
}

async fn written(client: &Client, scope: Scope, sent: &[Sent]) {
    stored(client, scope, sent.iter().map(Sent::span).collect()).await;
}

async fn stored(client: &Client, scope: Scope, spans: Vec<Span>) {
    let sink = ClickhouseSink::new(
        client,
        BatchLimits {
            max_rows: 100_000,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    );
    sink.accept(scope, spans).await.expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

fn text(key: &str, value: &str) -> KeyValue {
    KeyValue {
        key: key.to_owned(),
        value: Some(AnyValue {
            value: Some(Value::StringValue(value.to_owned())),
        }),
        ..KeyValue::default()
    }
}

fn exported_under(environment: &str, spans: Vec<(u8, u64, Option<&str>)>) -> Vec<Span> {
    let request = ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: Some(Resource {
                attributes: vec![
                    text("service.name", "checkout"),
                    text("deployment.environment", environment),
                ],
                ..Resource::default()
            }),
            scope_spans: vec![ScopeSpans {
                spans: spans
                    .into_iter()
                    .map(|(seed, at, own)| ProtoSpan {
                        trace_id: vec![seed; 16],
                        span_id: vec![seed; 8],
                        name: "pay".to_owned(),
                        kind: 2,
                        start_time_unix_nano: BASE + at,
                        end_time_unix_nano: BASE + at + 10 * MILLI,
                        attributes: own
                            .map(|value| vec![text("deployment.environment", value)])
                            .unwrap_or_default(),
                        ..ProtoSpan::default()
                    })
                    .collect(),
                ..ScopeSpans::default()
            }],
            ..ResourceSpans::default()
        }],
    };
    spans_of(request).expect("a valid export")
}

fn the_hour() -> ServiceFilter {
    ServiceFilter::over(Window::new(BASE, BASE + HOUR).expect("an hour"))
}

fn checkout_spans() -> Vec<Sent> {
    let mut sent: Vec<Sent> = (1..=100)
        .map(|n| {
            let paid = Sent::served(
                n,
                "checkout",
                "POST /pay",
                u64::from(n) * SECOND,
                u64::from(n) * MILLI,
            );
            if n % 10 == 0 { paid.failing() } else { paid }
        })
        .collect();
    sent.extend(
        (101..=120).map(|n| Sent::served(n, "checkout", "GET /cart", 30 * MINUTE, 5 * MILLI)),
    );
    sent.push(
        Sent::served(121, "checkout", "SELECT orders", 40 * MINUTE, 2 * MILLI)
            .of_kind(SpanKind::Client),
    );
    sent.push(Sent::served(
        122,
        "billing",
        "charge",
        41 * MINUTE,
        9 * MILLI,
    ));
    sent
}

fn near(found: u64, expected_millis: u64) -> bool {
    found.abs_diff(expected_millis * MILLI) <= 2 * MILLI
}

#[tokio::test]
async fn the_operations_of_a_service_count_and_time_each_span_name_it_served() {
    let Some(client) = prepared("ferristrace_views_operations").await else {
        return;
    };
    written(&client, Scope::of(org(MINE)), &checkout_spans()).await;

    let operations = ClickhouseReader::new(client)
        .operations(org(MINE), "checkout", &the_hour())
        .await
        .expect("the operations read");

    let names: Vec<&str> = operations
        .iter()
        .map(|operation| &*operation.name)
        .collect();
    assert_eq!(
        names,
        vec!["POST /pay", "GET /cart"],
        "busiest first, and a client span is no operation the service served"
    );
    let pay = &operations[0];
    assert_eq!((pay.requests, pay.errors), (100, 10));
    assert!(near(pay.p50_nanos, 50), "p50 {}", pay.p50_nanos);
    assert!(near(pay.p95_nanos, 95), "p95 {}", pay.p95_nanos);
    assert!(near(pay.p99_nanos, 99), "p99 {}", pay.p99_nanos);
    assert_eq!(pay.last_seen_unix_nanos, BASE + 100 * SECOND);
    let cart = &operations[1];
    assert_eq!((cart.requests, cart.errors), (20, 0));
    assert_eq!(cart.p95_nanos, 5 * MILLI);
    assert_eq!(cart.last_seen_unix_nanos, BASE + 30 * MINUTE);
}

#[tokio::test]
async fn a_service_series_counts_the_requests_and_errors_of_each_step() {
    let Some(client) = prepared("ferristrace_views_series").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "checkout", "pay", 10 * SECOND, 100 * MILLI),
            Sent::served(2, "checkout", "pay", 20 * SECOND, 100 * MILLI),
            Sent::served(3, "checkout", "pay", 59 * SECOND, 100 * MILLI),
            Sent::served(4, "checkout", "pay", 5 * MINUTE, 300 * MILLI).failing(),
            Sent::served(5, "checkout", "pay", 5 * MINUTE + SECOND, 300 * MILLI),
            Sent::served(6, "billing", "charge", 5 * MINUTE, 50 * MILLI),
        ],
    )
    .await;
    let reader = ClickhouseReader::new(client);

    let checkout = reader
        .series(org(MINE), Some("checkout"), &the_hour())
        .await
        .expect("the series reads")
        .filled(&the_hour());
    let everything = reader
        .series(org(MINE), None, &the_hour())
        .await
        .expect("the series reads")
        .filled(&the_hour());

    assert_eq!(checkout.step_nanos, MINUTE);
    assert_eq!(checkout.points.len(), 60);
    assert_eq!(
        (
            checkout.points[0].start_unix_nanos,
            checkout.points[0].requests,
            checkout.points[0].errors
        ),
        (BASE, 3, 0)
    );
    assert!(near(checkout.points[0].p95_nanos.expect("a p95"), 100));
    assert_eq!(
        (checkout.points[5].requests, checkout.points[5].errors),
        (2, 1)
    );
    assert!(near(checkout.points[5].p95_nanos.expect("a p95"), 300));
    assert_eq!(checkout.points[1], ServicePoint::idle(BASE + MINUTE));
    assert_eq!((checkout.requests, checkout.errors), (5, 1));
    assert!(near(
        checkout.p95_nanos.expect("a p95 over the window"),
        300
    ));
    assert_eq!(
        everything.points[5].requests, 3,
        "no service means every one"
    );
    assert_eq!(everything.requests, 6);
}

#[tokio::test]
async fn an_environment_narrows_services_operations_and_series_to_its_spans() {
    let Some(client) = prepared("ferristrace_views_environment").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)),
        &[
            Sent::served(1, "checkout", "pay", SECOND, 10 * MILLI).in_environment("production"),
            Sent::served(2, "checkout", "pay", 2 * SECOND, 10 * MILLI)
                .in_environment("production")
                .failing(),
            Sent::served(3, "checkout", "pay", 3 * SECOND, 10 * MILLI).in_environment("staging"),
            Sent::served(4, "checkout", "refund", 4 * SECOND, 10 * MILLI),
            Sent::served(5, "billing", "charge", 5 * SECOND, 10 * MILLI).in_environment("staging"),
        ],
    )
    .await;
    stored(
        &client,
        Scope::of(org(MINE)),
        exported_under(
            "production",
            vec![
                (0xa1, 6 * SECOND, None),
                (0xa2, 7 * SECOND, Some("staging")),
            ],
        ),
    )
    .await;
    let reader = ClickhouseReader::new(client);
    let mut production = the_hour();
    production.environment = Some("production".into());

    let services = reader
        .services(org(MINE), &production)
        .await
        .expect("the services read");
    let operations = reader
        .operations(org(MINE), "checkout", &production)
        .await
        .expect("the operations read");
    let series = reader
        .series(org(MINE), None, &production)
        .await
        .expect("the series reads");
    let unfiltered = reader
        .services(org(MINE), &the_hour())
        .await
        .expect("the services read");

    assert_eq!(services.len(), 1);
    assert_eq!(
        (&*services[0].name, services[0].requests, services[0].errors),
        ("checkout", 3, 1),
        "an environment set only on the resource counts, and a span's own value wins over it"
    );
    assert_eq!(operations.len(), 1);
    assert_eq!((&*operations[0].name, operations[0].requests), ("pay", 3));
    assert_eq!((series.requests, series.errors), (3, 1));
    assert_eq!(
        unfiltered.len(),
        2,
        "no environment asked for means every one"
    );
}

#[tokio::test]
async fn operations_and_series_never_cross_organisations_or_projects() {
    let Some(client) = prepared("ferristrace_views_isolation").await else {
        return;
    };
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(11))),
        &[Sent::served(1, "checkout", "pay", SECOND, 10 * MILLI)],
    )
    .await;
    written(
        &client,
        Scope::of(org(MINE)).in_project(Some(project(12))),
        &[Sent::served(2, "checkout", "pay", 2 * SECOND, 10 * MILLI)],
    )
    .await;
    written(
        &client,
        Scope::of(org(THEIRS)),
        &[
            Sent::served(3, "checkout", "pay", 3 * SECOND, 10 * MILLI),
            Sent::served(4, "checkout", "steal", 4 * SECOND, 10 * MILLI),
        ],
    )
    .await;
    let reader = ClickhouseReader::new(client);
    let mut storefront = the_hour();
    storefront.project = Some(project(11));

    let mine = reader
        .operations(org(MINE), "checkout", &the_hour())
        .await
        .expect("the operations read");
    let narrowed = reader
        .operations(org(MINE), "checkout", &storefront)
        .await
        .expect("the operations read");
    let series = reader
        .series(org(MINE), Some("checkout"), &the_hour())
        .await
        .expect("the series reads");
    let narrowed_series = reader
        .series(org(MINE), None, &storefront)
        .await
        .expect("the series reads");

    assert_eq!(mine.len(), 1);
    assert_eq!((&*mine[0].name, mine[0].requests), ("pay", 2));
    assert_eq!(narrowed[0].requests, 1);
    assert_eq!(series.requests, 2);
    assert_eq!(narrowed_series.requests, 1);
}

struct OneSession;

impl SessionStore for OneSession {
    async fn issue(
        &self,
        _account: AccountId,
        _org: Option<TenantId>,
        _from: Option<&Agent>,
    ) -> Result<SessionToken, SessionError> {
        Ok(SessionToken::from_plaintext(TOKEN))
    }

    async fn resolve(&self, token: &SessionToken) -> Result<Session, SessionError> {
        if token.exposed() != TOKEN {
            return Err(SessionError::Unknown);
        }
        Ok(Session {
            account: AccountId::new(Uuid::from_u128(9)),
            email: ferristrace_domain::Email::try_from("person@example.com").expect("an address"),
            name: None,
            photo: None,
            org: None,
            expires_at: UnixSeconds::new(0),
        })
    }

    async fn revoke(&self, _token: &SessionToken) -> Result<(), SessionError> {
        Ok(())
    }
}

impl SessionRoll for OneSession {
    async fn live(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<Vec<LiveSession>, SessionError> {
        Ok(Vec::new())
    }

    async fn revoke_one(&self, _account: AccountId, _id: SessionId) -> Result<bool, SessionError> {
        Ok(false)
    }

    async fn revoke_others(
        &self,
        _account: AccountId,
        _current: &SessionToken,
    ) -> Result<u64, SessionError> {
        Ok(0)
    }
}

struct MemberOfMine;

impl Memberships for MemberOfMine {
    async fn of(&self, _account: AccountId) -> Result<Vec<Membership>, WhoamiError> {
        Ok(vec![Membership {
            org: org(MINE),
            name: "Acme".into(),
            role: OrgRole::Member,
        }])
    }
}

struct Fakes;

impl Untransacted for Fakes {}

impl RepoFor<domain::Sessions> for Fakes {
    type Repo = OneSession;

    fn build(&self, (): &()) -> Self::Repo {
        OneSession
    }
}

impl RepoFor<domain::Memberships> for Fakes {
    type Repo = MemberOfMine;

    fn build(&self, (): &()) -> Self::Repo {
        MemberOfMine
    }
}

impl RepoFor<domain::ApiKeys> for Fakes {
    type Repo = Inert;

    fn build(&self, (): &()) -> Self::Repo {
        Inert
    }
}

struct Views;

impl Stack for Views {
    type Repos = Fakes;
    type Clock = Inert;
    type Mailer = Inert;
    type Photos = NoPhotos;
    type SpanStore = ClickhouseReader;
    type IssueStore = Inert;
    type LogStore = Inert;
    type MetricStore = Inert;
    type Meter = Inert;
    type Oidc = Inert;
    type Agents = Inert;
    type Sink = Inert;
    type Quota = Inert;
    type Directory = Inert;
    type Webhooks = Inert;
}

async fn asked(reader: ClickhouseReader, uri: &str) -> serde_json::Value {
    let adapters = Adapters {
        clock: Inert,
        mailer: Inert,
        webhooks: Inert,
        photos: NoPhotos,
        meter: Inert,
        span_store: Some(reader),
        issue_store: None,
        log_store: None,
        metric_store: None,
        oidc: None,
        provision: ProvisionPolicy::default(),
    };
    let state = AppState::<Views>::new(
        Arc::new(Ferristrace::new(Fakes, adapters)),
        SessionCookie::default(),
    );
    let request = Request::builder()
        .uri(uri)
        .header("x-requested-with", "fetch")
        .header(COOKIE, format!("ft_session={TOKEN}"))
        .body(Body::empty())
        .expect("a request");
    let response = explore_router(state)
        .oneshot(request)
        .await
        .expect("a response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    serde_json::from_slice(&body).expect("json")
}

#[tokio::test]
async fn spec_l1_7_the_services_table_shows_the_rate_errors_and_p95_of_the_spans_sent() {
    let Some(client) = prepared("ferristrace_views_spec_l1_7").await else {
        return;
    };
    written(&client, Scope::of(org(MINE)), &checkout_spans()).await;
    let from_ms = BASE / MILLI;
    let uri = format!(
        "/api/orgs/{}/services?from={from_ms}&to={}",
        Uuid::from_u128(MINE),
        from_ms + 3_600_000
    );

    let found = asked(ClickhouseReader::new(client), &uri).await;

    let services = found["services"].as_array().expect("the services");
    assert_eq!(services.len(), 2);
    let checkout = &services[0];
    assert_eq!(checkout["name"], "checkout");
    assert_eq!(checkout["requests"], 120);
    assert_eq!(checkout["errors"], 10);
    let rate = checkout["per_second"].as_f64().expect("a rate");
    assert!((rate - 120.0 / 3600.0).abs() < 1e-9, "{rate}");
    let errors = checkout["error_rate"].as_f64().expect("an error rate");
    assert!((errors - 10.0 / 120.0).abs() < 1e-9, "{errors}");
    let p95_us = checkout["p95_us"].as_u64().expect("a p95");
    assert!(p95_us.abs_diff(95_000) <= 2_000, "{p95_us}");
    let billing = &services[1];
    assert_eq!(billing["name"], "billing");
    assert_eq!(billing["error_rate"], 0.0);
}
