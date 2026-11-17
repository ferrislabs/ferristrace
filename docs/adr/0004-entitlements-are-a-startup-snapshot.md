# 0004. Entitlements are read once at startup, not once per payload

Status: superseded by 0039
Date: 2026-10-24

## Context
Every ingested payload must be checked against its tenant allowance, so whatever holds that allowance sits on the hottest path in the gateway. The measured cost of the write path is 0.11s for 60 000 spans (docs/benchmarks/clickhouse-batching.md); a control plane round trip per payload would dominate it by orders of magnitude. Plans change rarely: a tenant changes plan when a human acts, not when traffic arrives.

## Decision
The gateway reads `orgs` joined to `plans` once at startup and holds the result in memory as a map from tenant to allowance. The quota policy consults that map and never queries Postgres on the request path. Without a configured control plane the configuration file supplies the same map.

## Alternatives rejected
- A query per payload: adds a network round trip to a path measured in microseconds per span, and makes ingestion fail when the control plane does.
- A query per payload behind a TTL cache: the same failure mode at the TTL boundary, plus a cache to size, invalidate and reason about, for data that changes when a human clicks.
- Postgres `LISTEN` on a plan change: the right answer eventually, but it needs a connection held for the life of the process and a reconnection policy, neither of which this part of #62 pays for.

## Consequences
- Gains: the quota check costs a map lookup. A control plane outage cannot stop ingestion, because nothing on the request path talks to it.
- Costs: the control plane is unreachable at startup means the gateway refuses to start, which is deliberate: starting with no allowances would silently serve every tenant as unlimited.
- What becomes harder: a plan change takes effect on the next restart. A tenant upgraded mid-month stays capped until then, which is the gap `LISTEN` or a periodic refresh closes.
