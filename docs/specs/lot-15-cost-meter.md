# Lot 15: cost meter

Goal
  A Cost Meter page shows, per organisation and per month, what was
  ingested by signal: the size of log records, the size of spans, and the
  number of metric datapoints, with a daily chart for each.
  Decision: [ADR 0037](../adr/0037-the-cost-meter-counts-what-each-signal-carried-at-ingest.md).

Context
  Settings > Usage meters one number per organisation: decompressed
  payload bytes, all signals mixed (`UsageMeter::record_ingest`, Redis
  counter, hourly rollup into `usage_hourly`). The quota and the plan
  ceiling use that same number. Nothing says how much of it is logs,
  spans or metrics, and a metric payload's size says little about its
  datapoints.

Decisions
  D1  The total stays the quota's number. Per-signal figures are added
      beside it and never replace it.
  D2  Logs and spans are measured in decompressed payload bytes, the unit
      already metered. Metrics are measured in datapoints, because that is
      what a metric backend bills and what a user can act on.
  D3  A datapoint is one number, histogram, exponential histogram or
      summary point in an accepted payload. Exemplars are not counted.
  D4  The signal is a closed enum (`Spans`, `Logs`, `Metrics`) carried by
      the ingest use case, which already knows which sink it calls.
  D5  Counting stays best effort and off the request path's failure modes:
      ADR 0005 applies unchanged to the new counters.
  D6  No backfill. Hours rolled up before this lot have a total and no
      split; the page shows them as "not split", never as zero.
  D7  A new page, `/cost`, in the navigation. Settings > Usage is
      unchanged and keeps its note, corrected to point at the new page.

Invariants
  - The per-signal bytes of an hour never exceed that hour's total.
  - A payload refused by the quota counts in no signal.
  - The quota decision is the same with or without this lot.
  - An organisation reads only its own figures (ADR 0015 membership).

Design
  Domain (`telemetry`):
    `Signal` enum. `UsageMeter::record_ingest(tenant, signal, volume)`
    where `Volume` is bytes for spans and logs, datapoints for metrics,
    plus the existing decompressed total. `Ingest::metered` passes the
    signal. `MetricPayload` gains `datapoints()` computed from `metrics`.
  Counters (`redis`):
    The hourly key becomes a hash `usage:{tenant}:{hour}` with fields
    `total`, `spans_bytes`, `logs_bytes`, `metrics_points`, incremented in
    the existing atomic pipeline. The monthly total key is unchanged, so
    the quota read is untouched. `finished` reads the hash.
  Ledger (`postgres`):
    Migration adds nullable `spans_bytes`, `logs_bytes`, `metrics_points`
    bigint columns to `usage_hourly` (CHECK >= 0). Null means not split
    (D6). `record_hours` writes them.
  Application / API:
    `GET /api/orgs/{org}/usage` gains per hour `spans_bytes`, `logs_bytes`,
    `metrics_points` (nullable) and month totals per signal. Existing
    fields are kept.
  Web:
    Page `/cost` with a month selector. Three meters (logs size, spans
    size, metric datapoints) with month total, share of the ceiling where
    one exists, and projection at month end. A daily chart per signal and
    a table per day and signal. Hours without a split are marked.

Out of scope
  - Money. The page counts volume, not a price.
  - Per project or per service breakdown (the counter is per organisation).
  - Stored size in ClickHouse, retention or cold-storage cost.
  - Per-signal quotas or alerts on the meters.

Acceptance
  @spec-l15-1  An accepted spans payload adds its decompressed bytes to
               `spans_bytes` and to the total, and to no other signal.
  @spec-l15-2  An accepted logs payload adds to `logs_bytes` and the
               total only.
  @spec-l15-3  An accepted metrics payload adds its datapoint count to
               `metrics_points` and its decompressed bytes to the total.
  @spec-l15-4  A payload refused by the quota changes no counter.
  @spec-l15-5  The hourly rollup writes the three figures to
               `usage_hourly`; an hour already present is not doubled.
  @spec-l15-6  Hours rolled up before the migration read as null, and the
               API returns null, not zero.
  @spec-l15-7  The usage endpoint returns month totals per signal equal
               to the sum of its hours, and refuses a non-member.
  @spec-l15-8  `/cost` shows the three meters, the three daily charts and
               the table for the chosen month, and an empty state when
               nothing has been rolled up.
  @spec-l15-9  With the counter store down, ingestion still admits the
               payload (ADR 0005).

Slices
  15a  domain + redis + postgres: `Signal`, counters, ledger, migration.
  15b  api: usage endpoint carries the split.
  15c  web: `/cost` page and navigation entry.

Verify (exit)
  cargo fmt --check, cargo clippy --workspace -- -D warnings
  cargo test --workspace  (Redis and Postgres suites included)
  pnpm --dir web build and the page checked in a browser against a
  local stack.
