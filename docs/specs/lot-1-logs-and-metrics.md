# Lot 1: logs and metrics

Goal
  Accept, keep and query OpenTelemetry logs and metrics the way traces are
  accepted, kept and queried today, and show them in the Logs, Metrics,
  Services and Overview pages instead of mock data.

Context
  Milestone "1 · Logs and metrics": #175, #176, #122, #123, #119, #120,
  #121. Only `/v1/traces` and the gRPC trace service exist
  (crates/api/src/http.rs, grpc.rs); `opentelemetry-proto` is built with the
  `trace` feature only. Spans and span events live in ClickHouse with a
  per-row `expire_at` (lot 0 made it per organisation). Logs, Metrics and
  Overview read `web/src/mocks`. Ingest goes through `Ingest::accept`
  (key, quota, sink, meter) and must stay off control plane transactions
  (ADR 0004).

  One pull request per part, each releasable:
    1A  Logs: ingest, store, query, Logs page            #175 #122
    1B  Metrics: ingest, store, query, Metrics page      #176 #123
    1C  Links and views: trace <-> logs, exemplars,
        Services and Overview from real data             #119 #120 #121

Glossary
  Log record   one OTLP LogRecord with its resource and scope, stored as a
               row: time, observed time, severity, body, attributes, trace
               and span ids.
  Data point   one OTLP metric point: a gauge or sum value, or a histogram
               (bucket counts and bounds), with its attributes and time.
  Series       the data points of one metric name and one attribute set.

Invariants
  - Logs and metrics are admitted by the same key (`ingest` scope), quota
    and metering as spans; their decompressed bytes count in usage.  [test]
  - A log record or data point is stored with its organisation and
    project, and every read filters on the organisation.  [test]
  - Logs and data points expire like spans: `expire_at` from the scope's
    retention.  [test]
  - A read key bound to a project reads only that project's logs and
    metrics, like traces (lot 0).  [test]
  - The ingest path opens no control plane transaction.  [review]
  - No log body or attribute is truncated silently: an oversize payload is
    refused with 413 like traces.  [test]

Out of scope
  - Exponential histograms and summaries: accepted (200) and counted, but
    not stored, with a warning log once per process; a follow-up issue.
  - Live tail over a push channel: the Logs page polls the query with the
    "newer than" cursor every few seconds.
  - Derived metrics from spans (RED metrics) beyond what Services and
    Overview compute with SQL over the spans table.
  - Log pipelines, parsing, sampling.

Design
  1A Logs
    otlp: decode `ExportLogsServiceRequest` (feature `logs`) into domain
    `LogRecord`s: time (time_unix_nano, else observed), severity number
    and text normalised to `trace|debug|info|warn|error|fatal`, body as a
    string (AnyValue rendered as JSON when it is not a string), attributes
    and resource attributes merged like spans, service.name, trace id and
    span id when present, project from the scope.
    domain telemetry: `LogRecord`, a `LogSink` port (accept scope + records),
    `Ingest` generalised to accept a `Payload` of spans or logs (one admit,
    one meter record).
    api: `POST /v1/logs` (protobuf, gzip, same auth and limits as traces;
    OTLP/JSON is #198 for both signals), gRPC `LogsService`.
    clickhouse: migration `logs` table (tenant_id, project_id,
    service_name, time, observed_time, severity, severity_number, body,
    attributes Map(String,String), resource Map(String,String), trace_id
    FixedString(16) default zeros, span_id FixedString(8) default zeros,
    expire_at) ENGINE MergeTree PARTITION BY toDate(time) ORDER BY
    (tenant_id, service_name, time) TTL expire_at DELETE, bloom indexes on
    trace_id and a tokenbf_v1 index on body; a `ClickhouseLogSink` batching
    like the span sink; a `LogReader`.
    read: domain `LogQuery` (window, services, severities, text, attribute
    equals, trace id, project, limit, cursor) and `LogReader::search`
    (newest first, cursor) and `LogReader::histogram` (counts per bucket
    and severity over the window). Use cases behind `Reader` (session or
    read key), like explore.
    HTTP: `GET /api/orgs/{org}/logs?from&to&service&severity&q&attr&trace&project&limit&before`
    -> `{records:[{time_ns, observed_ns, severity, service, body,
    trace_id|null, span_id|null, attributes:{}, resource:{}}], next}`;
    `GET /api/orgs/{org}/logs/histogram?from&to&...&buckets=60`
    -> `{buckets:[{start_ns, counts:{trace,debug,info,warn,error,fatal}}]}`.
    web: Logs page wired: query bar (service, severity, text, attributes),
    histogram that narrows the window on click, table with expandable rows,
    a trace link when `trace_id` is set, live toggle polling every 5 s.
  1B Metrics
    otlp: decode `ExportMetricsServiceRequest` (feature `metrics`): gauge,
    sum (monotonic flag, temporality), histogram (explicit bounds, bucket
    counts, sum, count, min, max), exemplars with trace and span ids.
    clickhouse: tables `metric_points` (tenant_id, project_id,
    service_name, name, kind Enum(gauge, sum), temporality, monotonic,
    attributes, time, value Float64, expire_at) and `metric_histograms`
    (same keys plus count, sum, min, max, bounds Array(Float64), counts
    Array(UInt64)); `metric_exemplars` (name, time, value, trace_id,
    span_id, attributes); a `metric_catalogue` table filled at ingest
    (ReplacingMergeTree by tenant and name: kind, unit, description,
    last_seen).
    read: catalogue (search by name), series for one name: group by one or
    more attribute keys, aggregation `sum|avg|min|max|rate` for points,
    `p50|p95|p99|avg|rate` for histograms (quantiles from the buckets),
    step chosen from the window (60 points), exemplars in the window.
    HTTP: `GET /api/orgs/{org}/metrics?q` -> `{metrics:[{name, kind, unit,
    description, last_seen_ns}]}`; `GET /api/orgs/{org}/metrics/{name}/series?from&to&group_by&agg&project`
    -> `{step_ns, series:[{labels:{}, points:[[t_ns, v|null]]}],
    exemplars:[{time_ns, value, trace_id, span_id}]}`.
    web: Metrics page wired: picker, group by, aggregation, chart with
    legend, metadata panel, exemplar dots that open the trace.
  1C Links and views
    Trace view lists the logs of the trace (`logs?trace=`), a log row opens
    its trace, an exemplar opens its trace. Services (#120) and Overview
    (#119) computed from spans in ClickHouse: per service request rate,
    error rate, p50/p95/p99 over the window, last seen, operations of one
    service, and the overview tiles (requests, errors, p95, open issues)
    with sparklines; deploy markers stay for lot 3.

Acceptance
  @spec-l1-1  An OTLP/HTTP and an OTLP/gRPC logs export with an ingest key
              is accepted and its records are returned by the logs query,
              newest first, with severity, body, attributes and trace id.
  @spec-l1-2  A logs query filters by service, severity, text and
              attribute, and the histogram counts per severity match the
              records.
  @spec-l1-3  Logs of org A are never returned to org B, and a project-bound
              read key reads only its project's logs.
  @spec-l1-4  A gauge, a sum and a histogram exported over OTLP appear in
              the catalogue with kind and unit, and their series, grouped
              by an attribute, return the values sent (quantiles from
              bucket bounds within one bucket width).
  @spec-l1-5  An exemplar sent with a histogram point is returned with its
              trace id.
  @spec-l1-6  The decompressed bytes of logs and metrics count in usage and
              in the quota like spans.
  @spec-l1-7  The trace view shows the logs carrying its trace id, and the
              Services table shows request rate, error rate and p95 per
              service computed from the spans sent.

Simulation
  No: batching and flushing reuse the span sink's tested behaviour.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck && pnpm build

Amended: 2026-11-02 metrics: resource attributes are merged into each point's attributes (the point wins), so series group by host or Kubernetes keys; exemplars without a trace id are dropped at ingest; `agg` is optional and defaults to avg (gauge), rate (sum), p95 (histogram); `metric_catalogue` is keyed by (tenant, name, project) so a project-bound key keeps sight of a name shared across projects; histograms accept p50, p95, p99, avg, rate only.
