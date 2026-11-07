# 0034. A host is what the hostmetrics receiver reports

Status: accepted
Date: 2026-11-07

## Context
System metrics was the last backend page on mock data
([0033](0033-a-backend-page-reports-only-what-the-span-carries.md)). Its
numbers do not come from spans at all: hosts and containers arrive as
metrics, through the OpenTelemetry Collector's `hostmetrics` receiver.

`MetricReader` already answers a named metric over a window, grouped by
attributes, with one aggregation. That is enough for a page — but not for
the obvious reading of CPU.

`system.cpu.utilization` is a ratio reported per core **and** per state:
`user`, `system`, `idle`, `steal` and the rest, each a fraction of that
one core. Summing it across cores and states gives the core count, not a
percentage. Getting a host's busy share needs two aggregations at once —
mean across cores, sum across the non-idle states — and the port offers
one.

Two ways out were available. Add a second aggregation level to the metric
port, which is a change to every reader and every caller for one page. Or
pick a shape one aggregation can answer honestly.

`system.memory.utilization` and `system.filesystem.utilization` carry a
`state` too, so they have the same question with an easier answer: one
state is the one that matters.

## Decision
A host is a value of the `host.name` attribute. The page asks four
questions, each a single metric with a single aggregation:

- **CPU**: `avg` of `system.cpu.utilization` filtered to `state = idle`,
  grouped by `host.name`, shown as `1 − idle`. The average is over
  cores, so the figure is the busy share of one core on average, not of
  the whole machine.
- **Memory**: `sum` of `system.memory.utilization` filtered to
  `state = used`.
- **Disk**: `max` of `system.filesystem.utilization` filtered to
  `state = used` — the fullest mounted filesystem of that host.
- **Load**: `avg` of `system.cpu.load_average.1m`, which carries no
  sub-dimension and needs no filter.

The page says all four of those readings in its footnote. A metric whose
scraper is off reads `not reported`, never nought.

## Consequences
- The page needs no new port, no new route and no new SQL. It is a
  composition over `GET /api/orgs/{org}/metrics/{name}/series`, so it
  inherits the project, environment and Kubernetes placement filters for
  free.
- CPU is per core on average. A sixteen-core host pinned on one core
  reads about 6 %, which is correct and is not what a reader expecting a
  machine-level gauge assumes — hence the footnote. A machine-level
  figure needs the two-level aggregation this decision declined.
- A collector that sends `system.*` without `host.name` as a resource
  attribute gets a page that says so, rather than one empty table.
- The thresholds (amber from 75 % CPU, 85 % disk) live in the page, so
  they are not a stored setting and cannot be tuned per host yet.
- Containers are not covered. `container.*` metrics have their own names
  and their own identity attribute, and Infra already owns Kubernetes.
