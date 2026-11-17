# 0040. Stripe mirrors into Postgres and never sits on the ingest path

Status: proposed
Date: 2026-11-16

## Context
#345 asks the hosted service to sell a plan, take a payment and bill a
volume, while a self-hosted stack with no payment provider configured keeps
behaving exactly as it does today (ADR 0030: one build, nothing reserved to
the hosted service). The ingest path already has one hard rule (ADR 0004,
superseded by 0039 for the refresh, not for this): it calls no adapter it
does not already call for every payload. Stripe is a third-party HTTP API,
so a call to it on that path turns a Stripe outage into an ingest outage,
which is the one failure mode ADR 0005 was written to avoid for Redis.

The workspace is already split one crate per backend (ADR 0023): Postgres,
ClickHouse, Redis, and `infra` for S3, SMTP, OIDC and the rest. Nothing
reserved to the hosted build has existed since ADR 0030, and billing must
not reopen that.

## Decision
Billing is a bounded context in `ferristrace-domain` (`billing`), alongside
`telemetry`, `organisation` and the others, behind two ports:

- `BillingGateway` for the three calls FerrisTrace makes outward: open a
  checkout session, open a portal session, report a rolled-up hour to the
  usage meter. Nothing on this port returns a price or a currency; it takes
  identifiers and volumes and returns identifiers and URLs.
- `BillingAccounts`, a repository port (ADR 0024: one use case, one
  transaction) for the subscription state mirrored locally: plan, status,
  the Stripe customer and subscription id, the current period.

`ferristrace-stripe` is a new adapter crate, a sibling of `postgres`,
`clickhouse`, `redis` and `infra`, implementing `BillingGateway` against
Stripe's API and verifying a webhook's signature before anything is
applied. `apps/server` selects it only when a secret key and a webhook
secret are configured, the same shape as the existing choice between
`Meter::Memory` and `Meter::Redis`; with neither configured, no adapter is
built, the billing routes report Free for every organisation, and nothing
on the ingest or control plane path changes. The crate compiles into every
image regardless (ADR 0030): the choice is made at runtime, not by a
feature flag.

Subscription state moves in one direction, from Stripe to Postgres, carried
by webhooks the adapter verifies and applies through one transaction each.
A webhook names the Stripe event id; applying one records that id first, in
the same transaction, and a second delivery of the same id changes nothing
(ADR 0024's `settle = always` shape, the same one the second-step attempt
counter and the consumed OIDC state already use for a write that must not
be replayed). Checkout and the portal are Stripe's hosted pages: FerrisTrace
asks Stripe for a URL and redirects to it, and never renders a card field or
a price.

## Alternatives rejected
- A `billing` feature flag and a crate reserved to the hosted build: ADR
  0030 retired that shape once already, for a crate that never held a line
  of code; billing is exactly the kind of feature a self-hosted operator
  may also want to turn on.
- Reading subscription status from Stripe on the request path that needs
  it, instead of mirroring it: the read path (does this org get overage,
  is it suspended) runs far more often than a subscription changes, and it
  would make every one of those reads depend on Stripe being reachable.
- One generic `PaymentProvider` trait with Stripe as its only
  implementation today, written speculatively for a second provider: no
  second provider is asked for, and the crate-per-backend split already
  gives a new provider its own crate the day one is needed, without a
  trait designed ahead of it.
- Storing the price or the amount anywhere in FerrisTrace, to show it back
  without a round trip: #345 is explicit that Stripe computes the money.
  Postgres stores the Stripe price id on the plan row and nothing priced.

## Consequences
- Gains: a Stripe outage degrades checkout and the portal, never ingestion
  or a read already served from the mirrored state. A self-hosted operator
  never sees Stripe mentioned unless they configure it.
- Costs: a `billing_events` table and the discipline of checking it inside
  every webhook's transaction; a new crate to keep green in CI; tests for
  the port use a recorded webhook body and an in-memory `BillingGateway`,
  never Stripe's API.
- What becomes harder: the mirrored state can lag Stripe's by however long
  a webhook delivery takes. FerrisTrace already accepts that kind of lag
  for the plan map itself (0039); billing inherits it rather than adding a
  second one.
