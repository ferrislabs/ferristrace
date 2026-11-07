# 0033. A backend page reports only what the span carries

Status: accepted
Date: 2026-11-07

## Context
Backend held five tabs drawn from `web/src/mocks/signals.ts`: requests,
jobs, emails, queries and system metrics. Cache was the first to read
the span store ([0032](0032-a-cache-hit-is-what-the-instrumentation-says.md));
requests, jobs and queries follow here, and emails left the product
because nothing reads an `email.send` span.

Each mock promised numbers the span store cannot answer. Jobs showed a
queue wait time p95, a retry count and the next scheduled run. Queries
showed an average row count and a "possible N+1" tile. Requests showed a
share of total time, which is derivable, beside 2xx/4xx/5xx counts, which
are. OpenTelemetry defines no attribute for queue wait, for a retry
counter, or for a next-run time; `db.response.returned_rows` exists but
almost nothing fills it; and N+1 needs a per-trace count, which is a
second query with its own shape.

Carrying a mock forward reads as a working feature to anyone who has not
read the code, so a page that keeps the tile and feeds it a plausible
number is worse than a page that drops the tile.

## Decision
A backend page shows a measure only when a span the instrumentation
already emits carries it, or when it follows from the window by
arithmetic (a rate, a share of total time, a percentile).

So the three pages select spans this way, each on top of the shared
window, project, environment and Kubernetes placement filter:

- **Requests**: a server span carrying `http.request.method` (or the
  older `http.method`), or any server span with a status code. The route
  is `http.route`, falling back to the span name. The code is the
  `http_status_code` column, which the sink fills from
  `http.response.status_code` (or the older `http.status_code`). A 5xx is
  `http_status_code >= 500`, falling back to `status = 'error'` when no
  code was recorded.
- **Jobs**: a consumer span, or any span whose
  `messaging.operation.type` is `process` (with the older
  `messaging.operation` as a fallback). The queue is
  `messaging.destination.name`, the system `messaging.system`.
- **Queries**: a client span whose `db.system.name` (or `db.system`) is
  set and is not `redis`, `memcached` or `valkey` — those belong to
  Cache. The statement is `db.query.text` (or `db.statement`) with its
  literals replaced by `?`.

Wait time, retries, next run, row counts and N+1 are not shown, and no
tile stands in their place.

## Consequences
- The three pages are narrower than the mocks they replace. Each says in
  its footnote which attribute it reads, so a stack seeing an empty table
  knows what to instrument rather than that FerrisTrace is broken.
- Grouping is per page, and every grouping is a `GROUP BY` over the same
  selection, so adding one is a SQL constant.
- A Redis call reaches Cache and not Queries; a queue pop traced as a
  Redis client span reaches Cache and not Jobs. Both follow from
  `db.system.name` alone, which is what the span carries.
- N+1 is answered by a second query rather than a column: `count()` per
  `(trace_id, statement)`, keeping the statements that pass a threshold.
  It is a finding, not a measure of a row, so it reads as its own card.
- Nothing is left of the backend mock. `SignalPage`, `data/signals.ts`
  and `mocks/signals.ts` are gone, and System metrics reads the metric
  store — see
  [0034](0034-a-host-is-what-the-hostmetrics-receiver-reports.md).
