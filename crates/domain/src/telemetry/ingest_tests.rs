use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::sync::Mutex;
use uuid::Uuid;

use super::entities::{LogRecord, Span};
use super::errors::{IngestError, SinkError, UnknownKey};
use super::ingest::Ingest;
use super::ports::{LogSink, QuotaPolicy, SpanSink, TenantDirectory, Unmetered, UsageMeter};
use super::value_objects::{
    Admission, Carried, DecompressedBytes, IngestKey, LogPayload, Payload, RetentionDays, Scope,
    Severity, SpanId, SpanKind, SpanStatus, TenantId, TraceId,
};
use crate::metrics::{Metric, MetricData, MetricPayload, MetricPoint, MetricSink};
use crate::testing;

#[derive(Debug, Default)]
struct FakeDirectory(HashMap<String, TenantId>);

impl FakeDirectory {
    fn holding(plaintext: &str, tenant: TenantId) -> Self {
        Self(HashMap::from([(plaintext.to_owned(), tenant)]))
    }
}

impl TenantDirectory for FakeDirectory {
    fn resolve(&self, key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send {
        let found = self
            .0
            .get(key.exposed())
            .copied()
            .map(Scope::of)
            .ok_or(UnknownKey);
        async move { found }
    }
}

#[derive(Debug, Default)]
struct RecordingSink(Mutex<Vec<(Scope, Vec<Span>)>>);

impl RecordingSink {
    fn batches(&self) -> Vec<(Scope, Vec<Span>)> {
        self.0.lock().expect("sink lock").clone()
    }
}

impl SpanSink for RecordingSink {
    fn accept(
        &self,
        scope: Scope,
        spans: Vec<Span>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.0.lock().expect("sink lock").push((scope, spans));
        async { Ok(()) }
    }
}

#[derive(Debug)]
struct UnavailableSink;

impl SpanSink for UnavailableSink {
    async fn accept(&self, _scope: Scope, _spans: Vec<Span>) -> Result<(), SinkError> {
        Err(SinkError::Unavailable)
    }
}

#[derive(Debug, Default)]
struct RecordingMeter(Mutex<Vec<(TenantId, DecompressedBytes, Carried)>>);

impl RecordingMeter {
    fn readings(&self) -> Vec<(TenantId, DecompressedBytes)> {
        self.0
            .lock()
            .expect("meter lock")
            .iter()
            .map(|(tenant, decompressed, _)| (*tenant, *decompressed))
            .collect()
    }

    fn carried(&self) -> Vec<Carried> {
        self.0
            .lock()
            .expect("meter lock")
            .iter()
            .map(|(_, _, carried)| *carried)
            .collect()
    }
}

impl UsageMeter for RecordingMeter {
    fn record_ingest(
        &self,
        tenant: TenantId,
        decompressed: DecompressedBytes,
        carried: Carried,
    ) -> impl Future<Output = ()> + Send {
        self.0
            .lock()
            .expect("meter lock")
            .push((tenant, decompressed, carried));
        async {}
    }
}

fn a_span() -> Span {
    Span {
        trace_id: TraceId::try_from([7u8; 16].as_slice()).expect("sixteen bytes"),
        span_id: SpanId::try_from([3u8; 8].as_slice()).expect("eight bytes"),
        parent: None,
        name: "GET /".into(),
        service_name: "checkout".into(),
        kind: SpanKind::Server,
        status: SpanStatus::Ok,
        start_unix_nanos: 1,
        end_unix_nanos: 2,
        attributes: BTreeMap::new(),
        events: Vec::new(),
    }
}

fn a_payload(bytes: u64) -> Payload {
    Payload {
        spans: vec![a_span()],
        decompressed: DecompressedBytes::new(bytes),
    }
}

#[tokio::test]
async fn an_unknown_key_reaches_no_sink() {
    let sink = RecordingSink::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(FakeDirectory::default(), Unmetered, &sink, &meter);

    let outcome = ingest
        .accept(&IngestKey::from_plaintext("absent"), a_payload(512))
        .await;

    assert_eq!(outcome, Err(IngestError::UnknownKey(UnknownKey)));
    assert!(sink.batches().is_empty());
    assert!(meter.readings().is_empty());
}

#[tokio::test]
async fn a_known_key_sends_its_spans_under_its_tenant() {
    let tenant = TenantId::new(Uuid::from_u128(42));
    let sink = RecordingSink::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        &sink,
        &meter,
    );

    let resolved = ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(512))
        .await
        .expect("the key resolves");

    assert_eq!(resolved, tenant);
    assert_eq!(sink.batches(), vec![(Scope::of(tenant), vec![a_span()])]);
}

#[tokio::test]
async fn the_decompressed_size_is_metered_against_the_tenant() {
    let tenant = TenantId::new(Uuid::from_u128(7));
    let sink = RecordingSink::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        &sink,
        &meter,
    );

    ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(4096))
        .await
        .expect("the key resolves");

    assert_eq!(
        meter.readings(),
        vec![(tenant, DecompressedBytes::new(4096))]
    );
}

#[tokio::test]
async fn a_sink_failure_surfaces_to_the_caller() {
    let tenant = TenantId::new(Uuid::from_u128(1));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        UnavailableSink,
        &meter,
    );

    let outcome = ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(1))
        .await;

    assert_eq!(outcome, Err(IngestError::Sink(SinkError::Unavailable)));
}

#[tokio::test]
async fn a_sink_failure_meters_nothing() {
    let tenant = TenantId::new(Uuid::from_u128(1));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        UnavailableSink,
        &meter,
    );

    let outcome = ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(4096))
        .await;

    assert!(outcome.is_err());
    assert_eq!(meter.readings(), Vec::new());
}

#[derive(Debug, Clone, Copy)]
struct HardCap(u64);

impl QuotaPolicy for HardCap {
    fn admit(
        &self,
        _tenant: TenantId,
        incoming: DecompressedBytes,
    ) -> impl Future<Output = Admission> + Send {
        let allowance = DecompressedBytes::new(self.0);
        let decision = if incoming > allowance {
            Admission::Capped { allowance }
        } else {
            Admission::Within
        };
        async move { decision }
    }
}

#[derive(Debug, Clone, Copy)]
struct AllowOverage(u64);

impl QuotaPolicy for AllowOverage {
    fn admit(
        &self,
        _tenant: TenantId,
        incoming: DecompressedBytes,
    ) -> impl Future<Output = Admission> + Send {
        let over = incoming.get().saturating_sub(self.0);
        let decision = if over > 0 {
            Admission::Overage {
                over_by: DecompressedBytes::new(over),
            }
        } else {
            Admission::Within
        };
        async move { decision }
    }
}

#[tokio::test]
async fn a_capped_tenant_reaches_neither_the_meter_nor_the_sink() {
    let tenant = TenantId::new(Uuid::from_u128(4));
    let sink = RecordingSink::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        HardCap(1024),
        &sink,
        &meter,
    );

    let outcome = ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(4096))
        .await;

    assert_eq!(
        outcome,
        Err(IngestError::Capped {
            allowance: DecompressedBytes::new(1024)
        })
    );
    assert!(sink.batches().is_empty());
    assert!(meter.readings().is_empty());
}

#[tokio::test]
async fn an_allowed_overage_is_accepted_and_counted_in_full() {
    let tenant = TenantId::new(Uuid::from_u128(5));
    let sink = RecordingSink::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        AllowOverage(1024),
        &sink,
        &meter,
    );

    let resolved = ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(4096))
        .await
        .expect("an overage is still accepted");

    assert_eq!(resolved, tenant);
    assert_eq!(sink.batches().len(), 1);
    assert_eq!(
        meter.readings(),
        vec![(tenant, DecompressedBytes::new(4096))],
        "the whole payload counts, not only the part inside the allowance"
    );
}

#[tokio::test]
async fn a_payload_inside_the_allowance_passes_untouched() {
    let tenant = TenantId::new(Uuid::from_u128(6));
    let sink = RecordingSink::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        HardCap(8192),
        &sink,
        &meter,
    );

    ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(4096))
        .await
        .expect("the payload fits");

    assert_eq!(sink.batches().len(), 1);
    assert_eq!(meter.readings().len(), 1);
}

#[tokio::test]
async fn a_fake_directory_honours_the_directory_contract() {
    let tenant = TenantId::new(Uuid::from_u128(9));
    let directory = FakeDirectory::holding("live", tenant);
    let held = IngestKey::from_plaintext("live");
    let absent = IngestKey::from_plaintext("absent");

    testing::a_directory_resolves_the_key_it_holds(&directory, &held, tenant).await;
    testing::a_directory_refuses_a_key_it_does_not_hold(&directory, &absent).await;
    testing::a_directory_answers_the_same_key_the_same_way_twice(&directory, &held).await;
}

struct RetainingDirectory(Scope);

impl TenantDirectory for RetainingDirectory {
    fn resolve(&self, _key: &IngestKey) -> impl Future<Output = Result<Scope, UnknownKey>> + Send {
        let scope = self.0;
        async move { Ok(scope) }
    }
}

#[tokio::test]
async fn the_retention_the_directory_resolved_reaches_the_sink() {
    let tenant = TenantId::new(Uuid::from_u128(42));
    let week = RetentionDays::try_from(7).expect("a week");
    let sink = RecordingSink::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        RetainingDirectory(Scope::of(tenant).retained_for(Some(week))),
        Unmetered,
        &sink,
        &meter,
    );

    ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(512))
        .await
        .expect("the key resolves");

    assert_eq!(sink.batches()[0].0.retention, Some(week));
}

#[derive(Debug, Default)]
struct RecordingLogs(Mutex<Vec<(Scope, Vec<LogRecord>)>>);

impl RecordingLogs {
    fn batches(&self) -> Vec<(Scope, Vec<LogRecord>)> {
        self.0.lock().expect("sink lock").clone()
    }
}

impl LogSink for RecordingLogs {
    fn accept_logs(
        &self,
        scope: Scope,
        records: Vec<LogRecord>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.0.lock().expect("sink lock").push((scope, records));
        async { Ok(()) }
    }
}

impl LogSink for UnavailableSink {
    async fn accept_logs(&self, _scope: Scope, _records: Vec<LogRecord>) -> Result<(), SinkError> {
        Err(SinkError::Unavailable)
    }
}

fn a_log() -> LogRecord {
    LogRecord {
        time_unix_nanos: 5,
        observed_unix_nanos: 6,
        severity: Severity::Error,
        severity_number: 17,
        service_name: "checkout".into(),
        body: "payment declined".into(),
        attributes: BTreeMap::new(),
        resource: BTreeMap::new(),
        trace_id: None,
        span_id: None,
    }
}

fn some_logs(bytes: u64) -> LogPayload {
    LogPayload {
        records: vec![a_log(), a_log()],
        decompressed: DecompressedBytes::new(bytes),
    }
}

#[tokio::test]
async fn a_known_key_sends_its_logs_under_its_scope_and_meters_them_once() {
    let tenant = TenantId::new(Uuid::from_u128(42));
    let week = RetentionDays::try_from(7).expect("a week");
    let scope = Scope::of(tenant).retained_for(Some(week));
    let sink = RecordingLogs::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(RetainingDirectory(scope), Unmetered, &sink, &meter);

    let resolved = ingest
        .accept_logs(&IngestKey::from_plaintext("live"), some_logs(2048))
        .await
        .expect("the key resolves");

    assert_eq!(resolved, tenant);
    assert_eq!(sink.batches(), vec![(scope, vec![a_log(), a_log()])]);
    assert_eq!(
        meter.readings(),
        vec![(tenant, DecompressedBytes::new(2048))],
        "one export is one meter record, however many records it carries"
    );
}

#[tokio::test]
async fn logs_from_an_unknown_key_reach_neither_the_sink_nor_the_meter() {
    let sink = RecordingLogs::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(FakeDirectory::default(), Unmetered, &sink, &meter);

    let outcome = ingest
        .accept_logs(&IngestKey::from_plaintext("absent"), some_logs(512))
        .await;

    assert_eq!(outcome, Err(IngestError::UnknownKey(UnknownKey)));
    assert!(sink.batches().is_empty());
    assert!(meter.readings().is_empty());
}

#[tokio::test]
async fn logs_over_the_cap_are_refused_like_spans() {
    let tenant = TenantId::new(Uuid::from_u128(4));
    let sink = RecordingLogs::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        HardCap(1024),
        &sink,
        &meter,
    );

    let outcome = ingest
        .accept_logs(&IngestKey::from_plaintext("live"), some_logs(4096))
        .await;

    assert_eq!(
        outcome,
        Err(IngestError::Capped {
            allowance: DecompressedBytes::new(1024)
        })
    );
    assert!(sink.batches().is_empty());
    assert!(meter.readings().is_empty());
}

#[tokio::test]
async fn a_log_sink_failure_surfaces_and_meters_nothing() {
    let tenant = TenantId::new(Uuid::from_u128(1));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        UnavailableSink,
        &meter,
    );

    let outcome = ingest
        .accept_logs(&IngestKey::from_plaintext("live"), some_logs(64))
        .await;

    assert_eq!(outcome, Err(IngestError::Sink(SinkError::Unavailable)));
    assert!(meter.readings().is_empty());
}

#[derive(Debug, Default)]
struct RecordingMetrics(Mutex<Vec<(Scope, Vec<Metric>)>>);

impl RecordingMetrics {
    fn batches(&self) -> Vec<(Scope, Vec<Metric>)> {
        self.0.lock().expect("sink lock").clone()
    }
}

impl MetricSink for RecordingMetrics {
    fn accept_metrics(
        &self,
        scope: Scope,
        metrics: Vec<Metric>,
    ) -> impl Future<Output = Result<(), SinkError>> + Send {
        self.0.lock().expect("sink lock").push((scope, metrics));
        async { Ok(()) }
    }
}

impl MetricSink for UnavailableSink {
    async fn accept_metrics(&self, _scope: Scope, _metrics: Vec<Metric>) -> Result<(), SinkError> {
        Err(SinkError::Unavailable)
    }
}

fn a_metric() -> Metric {
    Metric {
        name: "queue.depth".into(),
        unit: "1".into(),
        description: "jobs waiting".into(),
        service_name: "worker".into(),
        data: MetricData::Gauge(vec![MetricPoint {
            time_unix_nanos: 5,
            value: 3.0,
            attributes: BTreeMap::new(),
            exemplars: Vec::new(),
        }]),
    }
}

fn some_metrics(bytes: u64) -> MetricPayload {
    MetricPayload {
        metrics: vec![a_metric(), a_metric()],
        decompressed: DecompressedBytes::new(bytes),
    }
}

#[tokio::test]
async fn a_known_key_sends_its_metrics_under_its_scope_and_meters_them_once() {
    let tenant = TenantId::new(Uuid::from_u128(43));
    let week = RetentionDays::try_from(7).expect("a week");
    let scope = Scope::of(tenant).retained_for(Some(week));
    let sink = RecordingMetrics::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(RetainingDirectory(scope), Unmetered, &sink, &meter);

    let resolved = ingest
        .accept_metrics(&IngestKey::from_plaintext("live"), some_metrics(1024))
        .await
        .expect("the key resolves");

    assert_eq!(resolved, tenant);
    assert_eq!(sink.batches(), vec![(scope, vec![a_metric(), a_metric()])]);
    assert_eq!(
        meter.readings(),
        vec![(tenant, DecompressedBytes::new(1024))],
        "one export is one meter record, however many metrics it carries"
    );
}

#[tokio::test]
async fn metrics_from_an_unknown_key_reach_neither_the_sink_nor_the_meter() {
    let sink = RecordingMetrics::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(FakeDirectory::default(), Unmetered, &sink, &meter);

    let outcome = ingest
        .accept_metrics(&IngestKey::from_plaintext("absent"), some_metrics(512))
        .await;

    assert_eq!(outcome, Err(IngestError::UnknownKey(UnknownKey)));
    assert!(sink.batches().is_empty());
    assert!(meter.readings().is_empty());
}

#[tokio::test]
async fn metrics_over_the_cap_are_refused_like_spans() {
    let tenant = TenantId::new(Uuid::from_u128(4));
    let sink = RecordingMetrics::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        HardCap(1024),
        &sink,
        &meter,
    );

    let outcome = ingest
        .accept_metrics(&IngestKey::from_plaintext("live"), some_metrics(4096))
        .await;

    assert_eq!(
        outcome,
        Err(IngestError::Capped {
            allowance: DecompressedBytes::new(1024)
        })
    );
    assert!(sink.batches().is_empty());
    assert!(meter.readings().is_empty());
}

#[tokio::test]
async fn a_metric_sink_failure_surfaces_and_meters_nothing() {
    let tenant = TenantId::new(Uuid::from_u128(1));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        UnavailableSink,
        &meter,
    );

    let outcome = ingest
        .accept_metrics(&IngestKey::from_plaintext("live"), some_metrics(64))
        .await;

    assert_eq!(outcome, Err(IngestError::Sink(SinkError::Unavailable)));
    assert!(meter.readings().is_empty());
}

#[tokio::test]
async fn an_export_whose_metrics_were_all_skipped_is_still_metered() {
    let tenant = TenantId::new(Uuid::from_u128(44));
    let sink = RecordingMetrics::default();
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        &sink,
        &meter,
    );

    ingest
        .accept_metrics(
            &IngestKey::from_plaintext("live"),
            MetricPayload {
                metrics: Vec::new(),
                decompressed: DecompressedBytes::new(300),
            },
        )
        .await
        .expect("an export of summaries only is accepted");

    assert_eq!(
        meter.readings(),
        vec![(tenant, DecompressedBytes::new(300))]
    );
}

#[tokio::test]
async fn spans_are_carried_as_the_decompressed_bytes_of_the_payload() {
    let tenant = TenantId::new(Uuid::from_u128(50));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        RecordingSink::default(),
        &meter,
    );

    ingest
        .accept(&IngestKey::from_plaintext("live"), a_payload(512))
        .await
        .expect("the key resolves");

    assert_eq!(meter.carried(), vec![Carried::spans(512)]);
}

#[tokio::test]
async fn logs_are_carried_as_the_decompressed_bytes_of_the_payload() {
    let tenant = TenantId::new(Uuid::from_u128(51));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        RecordingLogs::default(),
        &meter,
    );

    ingest
        .accept_logs(&IngestKey::from_plaintext("live"), some_logs(2048))
        .await
        .expect("the key resolves");

    assert_eq!(meter.carried(), vec![Carried::logs(2048)]);
}

#[tokio::test]
async fn metrics_are_carried_as_the_datapoints_of_the_payload() {
    let tenant = TenantId::new(Uuid::from_u128(52));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        Unmetered,
        RecordingMetrics::default(),
        &meter,
    );

    ingest
        .accept_metrics(&IngestKey::from_plaintext("live"), some_metrics(4096))
        .await
        .expect("the key resolves");

    assert_eq!(
        meter.readings(),
        vec![(tenant, DecompressedBytes::new(4096))]
    );
    assert_eq!(meter.carried(), vec![Carried::metrics(2)]);
}

#[tokio::test]
async fn a_capped_payload_carries_nothing() {
    let tenant = TenantId::new(Uuid::from_u128(53));
    let meter = RecordingMeter::default();
    let ingest = Ingest::new(
        FakeDirectory::holding("live", tenant),
        HardCap(1024),
        RecordingMetrics::default(),
        &meter,
    );

    let outcome = ingest
        .accept_metrics(&IngestKey::from_plaintext("live"), some_metrics(4096))
        .await;

    assert!(outcome.is_err());
    assert!(meter.carried().is_empty());
}
