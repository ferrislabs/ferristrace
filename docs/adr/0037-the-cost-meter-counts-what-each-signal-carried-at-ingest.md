# 0037. The cost meter counts what each signal carried at ingest

Status: proposed
Date: 2026-11-14

## Context
Usage is one number per organisation, the decompressed bytes of every accepted payload. The quota and the plan ceiling read it. Users want to know how much of it is logs, spans and metrics, and for metrics the number they act on is datapoints, not bytes. Two sources exist: the ingest path, which sees each payload once, and ClickHouse, which holds what was stored.

## Decision
The ingest use case reports the signal and its volume next to the existing total. Redis keeps them in the hourly hash, the hourly rollup writes them to `usage_hourly`, and the usage endpoint returns them. Spans and logs are counted in decompressed bytes, metrics in datapoints. The monthly total key the quota reads does not change.

History before the counters existed is rebuilt once, on request, from the span store: `ferristrace backfill-usage` measures the stored rows per tenant and hour and fills only hours that have a rolled-up row and no split. It never touches the billed total. Those figures measure stored rows, not payloads. On the local stack the stored spans and logs came to 5.43 GB against 5.68 GB metered, about 4 % under.

## Alternatives rejected
- Read sizes and row counts from ClickHouse on every request: it measures what is stored, after TTL and retried exports that may duplicate spans (ADR 0016), so it would drift from the number the quota enforces. It is used once, for history, and the counters take over from there.
- Count metric bytes like the others: a payload's size says little about how many series points it carries, and datapoints are the unit users compare against other backends.
- One counter key per signal and per month: it multiplies keys and scans for a figure only the hourly rollup needs.

## Consequences
- Gains: one source for quota, billing and the cost page, and a split that adds up to the total.
- Costs: a datapoint count computed on every metrics payload, and backfilled hours that are an estimate from stored rows, with no mark on the page to tell them apart. Data already expired by TTL cannot be rebuilt.
- What becomes harder: changing what a datapoint is. The definition is fixed here and in the spec.
