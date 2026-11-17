# 0039. Allowances refresh on a timer, not only at start

Status: proposed
Date: 2026-11-16

## Context
[ADR 0004](0004-entitlements-are-a-startup-snapshot.md) reads `orgs` joined
to `plans` once at startup and holds the map in memory, because a query per
payload would dominate a write path measured in microseconds and a control
plane outage would stop ingestion. Its accepted cost was that a plan change
takes effect on the next restart.

Billing (#345) makes that cost real: an organisation that upgrades through
Stripe checkout, that is moved back to Free after its payment retries are
exhausted, or that crosses its budget cap expects ingestion to notice within
the session, not at the next deploy. Nothing on the ingest path may call
Stripe or Postgres (that decision is unchanged), so the map itself has to
move, not the path that reads it.

## Decision
The map stays in memory and the quota check stays a lookup with no I/O. What
changes is who updates the map: a periodic task, separate from the request
path, re-reads `load_allowances` every 20 seconds and replaces the map behind
a lock the quota only ever reads. Each replica refreshes on its own; a plan
change is visible on every replica within one interval, comfortably under
the minute #345 asks for.

A refresh that cannot reach the control plane logs an error and keeps the
map it already has, rather than clearing it or stopping ingestion: the same
choice ADR 0005 makes for the usage counter, applied to the allowance map. A
tenant added after a dropped refresh is unreachable until a refresh
succeeds, same as today between startup and the first write; a tenant whose
plan changed keeps the stale allowance for a few more intervals, not
forever.

Startup is unchanged: the gateway still refuses to start if the first load
fails, because starting with an empty map would silently serve every tenant
as unlimited.

## Alternatives rejected
- Postgres `LISTEN` on a plan change: ADR 0004 already rejected it for the
  connection it holds for the life of the process and the reconnection
  policy that connection needs; #345 does not change that cost.
- A query per payload, cached with a short TTL: the same outage coupling at
  the TTL boundary that ADR 0004 rejected, for data that still changes only
  when a human, or a webhook standing in for one, acts.
- A message published by the billing webhook handler straight into the
  gateway's process: ties two use cases together over a channel that does
  not survive a restart or a second replica, for a case a 20 second poll
  already covers.

## Consequences
- Gains: a plan change, a budget cap, or a payment failure reaches
  ingestion within 20 seconds, with the request path unchanged (a map
  lookup, no network call). The periodic read costs one query most of the
  stack already pays once at startup.
- Costs: one more background task per replica, and `cargo test` gains a
  test for the swap being visible without a restart.
- What becomes harder: nothing is instantaneous. A tenant can ingest for up
  to one interval past a downgrade or a suspension, which is the gap #345's
  slice d (the budget cap) accepts in exchange for never calling Stripe or
  Postgres from the ingest path.
