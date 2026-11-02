use std::collections::BTreeMap;
use std::time::Duration;

use clickhouse::Client;
use ferristrace_clickhouse::{BatchLimits, ClickhouseIssues, ClickhouseMigrator, ClickhouseSink};
use ferristrace_domain::{
    CallQuery, LlmReader, ProjectId, Scope, Span, SpanId, SpanSink, SpanStatus, TenantId, TraceId,
    Window,
};
use ferristrace_infra::FixedClock;
use uuid::Uuid;

mod backend;

const NOW: u64 = 4_000_000_000;
const BASE: u64 = 1_700_000_000_000_000_000;
const SECOND: u64 = 1_000_000_000;
const MINUTE: u64 = 60 * SECOND;
const DAY: u64 = 24 * 60 * MINUTE;

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

async fn bare(name: &str) -> Option<Client> {
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
    Some(connected(&url).with_database(name))
}

async fn prepared(name: &str) -> Option<Client> {
    let client = bare(name).await?;
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

fn everything() -> Window {
    Window::new(BASE - 10 * DAY, BASE + 10 * DAY).expect("a window")
}

fn all() -> CallQuery {
    CallQuery::over(everything())
}

fn trace_of(seed: u32) -> TraceId {
    let mut trace = [9u8; 16];
    trace[12..].copy_from_slice(&seed.to_be_bytes());
    TraceId::try_from(trace.as_slice()).expect("sixteen bytes")
}

fn span_of(seed: u32) -> SpanId {
    let mut span = [7u8; 8];
    span[4..].copy_from_slice(&seed.to_be_bytes());
    SpanId::try_from(span.as_slice()).expect("eight bytes")
}

struct Sent {
    seed: u32,
    service: &'static str,
    at: u64,
    failed: bool,
    attributes: BTreeMap<Box<str>, Box<str>>,
}

impl Sent {
    fn new(seed: u32, at: u64) -> Self {
        Self {
            seed,
            service: "assistant",
            at,
            failed: false,
            attributes: BTreeMap::new(),
        }
    }

    fn llm(seed: u32, at: u64, model: &str) -> Self {
        Self::new(seed, at)
            .with("gen_ai.provider.name", "openai")
            .with("gen_ai.operation.name", "chat")
            .with("gen_ai.request.model", model)
            .with("gen_ai.usage.input_tokens", "100")
            .with("gen_ai.usage.output_tokens", "20")
    }

    fn with(mut self, key: &str, value: &str) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    fn failing(mut self) -> Self {
        self.failed = true;
        self
    }

    fn span(self) -> Span {
        Span {
            trace_id: trace_of(self.seed),
            span_id: span_of(self.seed),
            parent: None,
            name: "chat".into(),
            service_name: self.service.into(),
            kind: ferristrace_domain::SpanKind::Client,
            status: if self.failed {
                SpanStatus::Error
            } else {
                SpanStatus::Ok
            },
            start_unix_nanos: BASE + self.at,
            end_unix_nanos: BASE + self.at + 2 * SECOND + 500_000_000,
            attributes: self.attributes,
            events: Vec::new(),
        }
    }
}

async fn written(client: &Client, scope: Scope, sent: Vec<Sent>) {
    let sink = ClickhouseSink::new(
        client,
        BatchLimits {
            max_rows: 100_000,
            period: Duration::from_secs(60),
        },
        Duration::from_secs(7 * 24 * 60 * 60),
        FixedClock::at(NOW),
    );
    sink.accept(scope, sent.into_iter().map(Sent::span).collect())
        .await
        .expect("the sink accepts");
    sink.flush().await.expect("the sink flushes");
}

fn scope_of(tenant: u128, bound: Option<u128>) -> Scope {
    Scope {
        tenant: org(tenant),
        project: bound.map(project),
        retention: None,
    }
}

#[tokio::test]
async fn spec_l4_1_a_genai_span_is_listed_as_a_call_with_every_field() {
    let Some(client) = prepared("llm_every_field").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        vec![
            Sent::llm(1, MINUTE, "gpt-4o")
                .with("gen_ai.response.model", "gpt-4o-2024-08-06")
                .with("gen_ai.usage.cache_read.input_tokens", "40")
                .with("ferristrace.feature", "ticket-summary")
                .with("ferristrace.customer", "acme")
                .with("enduser.id", "user-7")
                .with("ferristrace.prompt.version", "v3")
                .with("deployment.environment.name", "prod")
                .failing(),
        ],
    )
    .await;

    let page = ClickhouseIssues::new(client)
        .calls(org(1), &all())
        .await
        .expect("calls");

    assert_eq!(page.calls.len(), 1);
    assert_eq!(page.next, None);
    let call = &page.calls[0].call;
    assert_eq!(page.calls[0].cost, None);
    assert_eq!(call.time_unix_nanos, BASE + MINUTE);
    assert_eq!(call.trace, trace_of(1));
    assert_eq!(call.span, span_of(1));
    assert_eq!(&*call.service, "assistant");
    assert_eq!(&*call.provider, "openai");
    assert_eq!(&*call.operation, "chat");
    assert_eq!(&*call.request_model, "gpt-4o");
    assert_eq!(&*call.response_model, "gpt-4o-2024-08-06");
    assert_eq!(call.model(), "gpt-4o-2024-08-06");
    assert_eq!(call.input_tokens, 100);
    assert_eq!(call.output_tokens, 20);
    assert_eq!(call.cached_input_tokens, 40);
    assert_eq!(call.duration_nanos, 2 * SECOND + 500_000_000);
    assert!(call.error);
    assert_eq!(&*call.feature, "ticket-summary");
    assert_eq!(&*call.customer, "acme");
    assert_eq!(&*call.user, "user-7");
    assert_eq!(&*call.prompt_version, "v3");
    assert_eq!(&*call.environment, "prod");
}

#[tokio::test]
async fn spec_l4_1_spans_without_genai_attributes_are_not_calls() {
    let Some(client) = prepared("llm_only_genai").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        vec![
            Sent::new(1, MINUTE).with("http.method", "GET"),
            Sent::new(2, MINUTE).with("gen_ai.operation.name", "chat"),
            Sent::new(3, MINUTE).with("gen_ai.request.model", ""),
            Sent::new(4, MINUTE).with("gen_ai.system", "anthropic"),
            Sent::new(5, MINUTE).with("gen_ai.request.model", "claude-sonnet"),
            Sent::new(6, MINUTE).with("gen_ai.provider.name", "openai"),
        ],
    )
    .await;

    let page = ClickhouseIssues::new(client)
        .calls(org(1), &all())
        .await
        .expect("calls");

    let mut spans: Vec<SpanId> = page.calls.iter().map(|priced| priced.call.span).collect();
    spans.sort();
    assert_eq!(spans, vec![span_of(4), span_of(5), span_of(6)]);
}

#[tokio::test]
async fn spec_l4_1_feature_customer_prompt_and_provider_fall_back_through_their_aliases() {
    let Some(client) = prepared("llm_fallbacks").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        vec![
            Sent::new(1, MINUTE)
                .with("gen_ai.system", "anthropic")
                .with("gen_ai.request.model", "claude")
                .with("gen_ai.agent.name", "triage-agent")
                .with("enduser.id", "user-1")
                .with("gen_ai.prompt.version", "p9")
                .with("deployment.environment", "staging"),
            Sent::new(2, 2 * MINUTE).with("gen_ai.request.model", "claude"),
            Sent::new(3, 3 * MINUTE)
                .with("gen_ai.request.model", "claude")
                .with("ferristrace.feature", "explicit")
                .with("gen_ai.agent.name", "ignored")
                .with("ferristrace.customer", "acme")
                .with("enduser.id", "user-2")
                .with("ferristrace.prompt.version", "v1")
                .with("gen_ai.prompt.version", "ignored"),
        ],
    )
    .await;

    let page = ClickhouseIssues::new(client)
        .calls(org(1), &all())
        .await
        .expect("calls");

    let by_span = |seed: u32| {
        page.calls
            .iter()
            .map(|priced| &priced.call)
            .find(|call| call.span == span_of(seed))
            .expect("a call")
    };
    let agent = by_span(1);
    assert_eq!(&*agent.provider, "anthropic");
    assert_eq!(&*agent.feature, "triage-agent");
    assert_eq!(&*agent.customer, "user-1");
    assert_eq!(&*agent.user, "user-1");
    assert_eq!(&*agent.prompt_version, "p9");
    assert_eq!(&*agent.environment, "staging");
    let bare = by_span(2);
    assert_eq!(&*bare.provider, "");
    assert_eq!(&*bare.feature, "assistant");
    assert_eq!(&*bare.customer, "");
    assert_eq!(&*bare.user, "");
    assert_eq!(&*bare.prompt_version, "");
    assert_eq!(&*bare.environment, "");
    let explicit = by_span(3);
    assert_eq!(&*explicit.feature, "explicit");
    assert_eq!(&*explicit.customer, "acme");
    assert_eq!(&*explicit.user, "user-2");
    assert_eq!(&*explicit.prompt_version, "v1");
}

#[tokio::test]
async fn spec_l4_1_calls_are_found_by_user_trace_model_prompt_feature_customer_and_provider() {
    let Some(client) = prepared("llm_filters").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        vec![
            Sent::llm(1, MINUTE, "gpt-4o")
                .with("gen_ai.response.model", "gpt-4o-2024-08-06")
                .with("enduser.id", "user-1")
                .with("ferristrace.prompt.version", "v1")
                .with("ferristrace.feature", "search")
                .with("ferristrace.customer", "acme"),
            Sent::llm(2, 2 * MINUTE, "gpt-4o-mini")
                .with("enduser.id", "user-2")
                .with("ferristrace.prompt.version", "v2")
                .with("ferristrace.feature", "summary")
                .with("ferristrace.customer", "globex"),
            Sent::llm(3, 3 * MINUTE, "claude")
                .with("gen_ai.provider.name", "anthropic")
                .with("enduser.id", "user-1")
                .with("ferristrace.prompt.version", "v2")
                .with("ferristrace.feature", "summary")
                .with("ferristrace.customer", "acme"),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);
    let seeds = |page: ferristrace_domain::CallPage| {
        let mut found: Vec<SpanId> = page.calls.iter().map(|priced| priced.call.span).collect();
        found.sort();
        found
    };

    let mut by_user = all();
    by_user.user = Some("user-1".into());
    let mut by_trace = all();
    by_trace.trace = Some(trace_of(2));
    let mut by_request_model = all();
    by_request_model.model = Some("gpt-4o".into());
    let mut by_response_model = all();
    by_response_model.model = Some("gpt-4o-2024-08-06".into());
    let mut by_prompt = all();
    by_prompt.prompt_version = Some("v2".into());
    let mut by_feature = all();
    by_feature.feature = Some("summary".into());
    let mut by_customer = all();
    by_customer.customer = Some("acme".into());
    let mut by_provider = all();
    by_provider.provider = Some("anthropic".into());
    let mut by_both = all();
    by_both.user = Some("user-1".into());
    by_both.prompt_version = Some("v2".into());

    assert_eq!(
        seeds(reader.calls(org(1), &by_user).await.expect("calls")),
        vec![span_of(1), span_of(3)]
    );
    assert_eq!(
        seeds(reader.calls(org(1), &by_trace).await.expect("calls")),
        vec![span_of(2)]
    );
    assert_eq!(
        seeds(
            reader
                .calls(org(1), &by_request_model)
                .await
                .expect("calls")
        ),
        vec![span_of(1)]
    );
    assert_eq!(
        seeds(
            reader
                .calls(org(1), &by_response_model)
                .await
                .expect("calls")
        ),
        vec![span_of(1)]
    );
    assert_eq!(
        seeds(reader.calls(org(1), &by_prompt).await.expect("calls")),
        vec![span_of(2), span_of(3)]
    );
    assert_eq!(
        seeds(reader.calls(org(1), &by_feature).await.expect("calls")),
        vec![span_of(2), span_of(3)]
    );
    assert_eq!(
        seeds(reader.calls(org(1), &by_customer).await.expect("calls")),
        vec![span_of(1), span_of(3)]
    );
    assert_eq!(
        seeds(reader.calls(org(1), &by_provider).await.expect("calls")),
        vec![span_of(3)]
    );
    assert_eq!(
        seeds(reader.calls(org(1), &by_both).await.expect("calls")),
        vec![span_of(3)]
    );
}

#[tokio::test]
async fn spec_l4_1_calls_come_newest_first_and_page_by_cursor() {
    let Some(client) = prepared("llm_paging").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        (1..=5)
            .map(|seed| Sent::llm(seed, u64::from(seed) * MINUTE, "gpt-4o"))
            .collect(),
    )
    .await;
    let reader = ClickhouseIssues::new(client);
    let mut query = all().at_most(2).expect("a limit");

    let mut seen = Vec::new();
    for _ in 0..3 {
        let page = reader.calls(org(1), &query).await.expect("calls");
        seen.extend(page.calls.iter().map(|priced| priced.call.span));
        match page.next {
            Some(cursor) => query.before = Some(cursor),
            None => break,
        }
    }

    assert_eq!(
        seen,
        vec![span_of(5), span_of(4), span_of(3), span_of(2), span_of(1)]
    );
    assert!(query.before.is_some());
}

#[tokio::test]
async fn spec_l4_1_the_window_bounds_the_calls() {
    let Some(client) = prepared("llm_window").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        vec![
            Sent::llm(1, MINUTE, "gpt-4o"),
            Sent::llm(2, 10 * MINUTE, "gpt-4o"),
            Sent::llm(3, 20 * MINUTE, "gpt-4o"),
        ],
    )
    .await;
    let narrow = Window::new(BASE + 5 * MINUTE, BASE + 20 * MINUTE).expect("a window");

    let page = ClickhouseIssues::new(client)
        .calls(org(1), &CallQuery::over(narrow))
        .await
        .expect("calls");

    assert_eq!(
        page.calls
            .iter()
            .map(|priced| priced.call.span)
            .collect::<Vec<_>>(),
        vec![span_of(2)]
    );
}

#[tokio::test]
async fn spec_l4_1_another_organisation_never_sees_the_calls() {
    let Some(client) = prepared("llm_isolation").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        vec![Sent::llm(1, MINUTE, "gpt-4o")],
    )
    .await;
    written(
        &client,
        scope_of(2, None),
        vec![Sent::llm(2, MINUTE, "claude")],
    )
    .await;
    let reader = ClickhouseIssues::new(client);
    let mut theirs_by_trace = all();
    theirs_by_trace.trace = Some(trace_of(1));

    let ours = reader.calls(org(1), &all()).await.expect("calls");
    let leaked = reader.calls(org(2), &theirs_by_trace).await.expect("calls");
    let nobody = reader.calls(org(3), &all()).await.expect("calls");
    let models = reader
        .models(org(2), everything(), None)
        .await
        .expect("models");

    assert_eq!(ours.calls.len(), 1);
    assert_eq!(&*ours.calls[0].call.request_model, "gpt-4o");
    assert!(leaked.calls.is_empty());
    assert!(nobody.calls.is_empty());
    assert_eq!(models.len(), 1);
    assert_eq!(&*models[0].request_model, "claude");
}

#[tokio::test]
async fn spec_l4_1_a_project_narrows_calls_and_models() {
    let Some(client) = prepared("llm_projects").await else {
        return;
    };
    written(
        &client,
        scope_of(1, Some(10)),
        vec![Sent::llm(1, MINUTE, "gpt-4o")],
    )
    .await;
    written(
        &client,
        scope_of(1, Some(20)),
        vec![
            Sent::llm(2, MINUTE, "claude"),
            Sent::llm(3, 2 * MINUTE, "claude"),
        ],
    )
    .await;
    let reader = ClickhouseIssues::new(client);
    let mut first = all();
    first.project = Some(project(10));
    let mut second = all();
    second.project = Some(project(20));

    let whole = reader.calls(org(1), &all()).await.expect("calls");
    let one = reader.calls(org(1), &first).await.expect("calls");
    let two = reader.calls(org(1), &second).await.expect("calls");
    let models = reader
        .models(org(1), everything(), Some(project(20)))
        .await
        .expect("models");

    assert_eq!(whole.calls.len(), 3);
    assert_eq!(one.calls.len(), 1);
    assert_eq!(&*one.calls[0].call.request_model, "gpt-4o");
    assert_eq!(two.calls.len(), 2);
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].calls, 2);
}

#[tokio::test]
async fn spec_l4_2_models_are_counted_by_provider_request_and_response_model() {
    let Some(client) = prepared("llm_models").await else {
        return;
    };
    written(
        &client,
        scope_of(1, None),
        vec![
            Sent::llm(1, MINUTE, "gpt-4o").with("gen_ai.response.model", "gpt-4o-2024-08-06"),
            Sent::llm(2, 3 * MINUTE, "gpt-4o").with("gen_ai.response.model", "gpt-4o-2024-08-06"),
            Sent::llm(3, 2 * MINUTE, "gpt-4o"),
            Sent::new(4, MINUTE).with("http.method", "GET"),
        ],
    )
    .await;

    let models = ClickhouseIssues::new(client)
        .models(org(1), everything(), None)
        .await
        .expect("models");

    assert_eq!(models.len(), 2);
    assert_eq!(&*models[0].provider, "openai");
    assert_eq!(&*models[0].request_model, "gpt-4o");
    assert_eq!(&*models[0].response_model, "gpt-4o-2024-08-06");
    assert_eq!(models[0].calls, 2);
    assert_eq!(models[0].last_unix_nanos, BASE + 3 * MINUTE);
    assert_eq!(&*models[1].response_model, "");
    assert_eq!(models[1].calls, 1);
}

#[tokio::test]
async fn spec_l4_1_a_replayed_span_is_one_call() {
    let Some(client) = prepared("llm_replayed").await else {
        return;
    };
    for _ in 0..2 {
        written(
            &client,
            scope_of(1, None),
            vec![Sent::llm(1, MINUTE, "gpt-4o")],
        )
        .await;
    }
    let reader = ClickhouseIssues::new(client);

    let page = reader.calls(org(1), &all()).await.expect("calls");
    let models = reader
        .models(org(1), everything(), None)
        .await
        .expect("models");

    assert_eq!(page.calls.len(), 1);
    assert_eq!(models[0].calls, 1);
}

#[tokio::test]
async fn spec_l4_1_spans_stored_before_the_migration_are_backfilled() {
    let Some(client) = bare("llm_backfill").await else {
        return;
    };
    let migrator = ClickhouseMigrator::new(client.clone());
    migrator.up().await.expect("the migrations apply");
    migrator.down_to(10).await.expect("the last one reverts");
    written(
        &client,
        scope_of(1, Some(10)),
        vec![
            Sent::llm(1, MINUTE, "gpt-4o").with("ferristrace.feature", "old"),
            Sent::new(2, MINUTE).with("http.method", "GET"),
        ],
    )
    .await;

    migrator.up().await.expect("the migration applies again");
    written(
        &client,
        scope_of(1, Some(10)),
        vec![Sent::llm(3, 2 * MINUTE, "gpt-4o").with("ferristrace.feature", "new")],
    )
    .await;

    let page = ClickhouseIssues::new(client)
        .calls(org(1), &all())
        .await
        .expect("calls");

    let mut features: Vec<&str> = page
        .calls
        .iter()
        .map(|priced| &*priced.call.feature)
        .collect();
    features.sort_unstable();
    assert_eq!(features, vec!["new", "old"]);
}
