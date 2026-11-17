# Lot 20: billing

Goal
  An organisation on the hosted service upgrades to Pro through a hosted
  Stripe checkout, manages its card and invoices in a hosted Stripe portal,
  and is billed for the volume it ingests beyond what its plan includes. A
  self-hosted stack with no Stripe configuration behaves exactly as today.
  Decisions: [ADR 0039](../adr/0039-allowances-refresh-on-a-timer-not-only-at-start.md),
  [ADR 0040](../adr/0040-stripe-mirrors-into-postgres-and-never-sits-on-the-ingest-path.md).

Context
  `plans` holds one row, `self-hosted` (`ingest_gb` and `retention_days`
  both NULL, `features = {}`), and `load_allowances` joins it to `orgs`
  once at startup (ADR 0004) into the map `StaticQuota` holds in memory.
  `Allowance` already has three variants: `Unlimited`, `Capped` (refuses
  past the limit) and `Overage` (never refuses, reports how far over), and
  `load_allowances` already picks `Overage` when a plan's `features`
  carries `"overage"` (`OVERAGE_FEATURE`) — nothing exercises that branch
  today, because no plan sets the feature. Issue #345.

Decisions
  D1  Plans other than `self-hosted` are declared in the server config
      (`[[plans]]`, or `FERRISTRACE_PLANS`) and written to `plans` on every
      start, updated in place by slug and never deleted. No migration
      inserts one, so a self-hosted database keeps exactly the
      `self-hosted` row. The hosted service declares `free`
      (`ingest_gb = 30`, `retention_days = 7`) and `pro` (`ingest_gb = 100`,
      `retention_days = 30`, `overage = true`, a `price`). A plan never
      gates anything but volume and retention.
  D2  Free beyond 30 GB is refused by the `Capped` branch that already
      exists. Pro beyond 100 GB is admitted and reported by the `Overage`
      branch that already exists; slice 20c reports that overage to
      Stripe, which prices and bills it. Neither branch changes.
  D3  Prices live in Stripe only. `plans.price_ref` names the payment
      provider's price for a paid plan, and is the only place that id
      lives; no amount, currency or price is
      written anywhere else — not in a migration, a config file, a log
      line, a spec or a test.
  D4  A Stripe customer is created the first time an organisation starts
      checkout, before the checkout session (ADR 0002: the organisation is
      the tenant Stripe bills), and the subscription carries the
      organisation in its metadata. Free has no Stripe object. The mapping
      lives in a new table, `billing_subscriptions`, one row per
      organisation that has ever talked to Stripe, holding the customer
      ref, the subscription ref, the mirrored status and period, and the
      creation time of the last event applied.
  D5  Subscription state moves one way, Stripe to Postgres, through
      webhooks verified by Stripe's signature and applied inside one
      transaction each (ADR 0024). `billing_events` records the event id
      in that same transaction, so a retried delivery changes nothing the
      second time. The mirror follows `customer.subscription.created`,
      `.updated` and `.deleted`: active or trialing moves the organisation
      to the plan carrying the subscription's price, past due keeps it,
      unpaid, canceled, paused or incomplete_expired moves it to `free`. An
      event older than the mirrored one is skipped. Nothing on the ingest
      path calls Stripe.
  D6  A plan change — an upgrade through checkout, a downgrade applied by
      a webhook — takes effect for ingestion within the refresh interval
      ADR 0039 sets (20 seconds), not at the next restart.
  D7  After Stripe's payment retries are exhausted, the webhook that
      reports it moves the organisation's `plan_id` back to `free`.
      Nothing is deleted; retention shortens only once the next midnight
      pass enforces the new plan's `retention_days`, never mid-request.
  D8  A monthly budget cap is still money, so FerrisTrace never computes
      or stores the amount. The organisation sets it on its Billing
      settings page; the server's one call to Stripe creates or updates a
      billing alert on the usage meter for that amount and holds nothing
      but Stripe's alert id. Crossing it arrives as a webhook that sets
      `billing_subscriptions.budget_cap_reached`; the next allowance
      refresh serves that organisation `Capped(DecompressedBytes::new(0))`
      until a new billing period clears the flag. The exact Stripe alert
      object and webhook
      event are confirmed against Stripe's API when 20d is implemented;
      this decision fixes only that FerrisTrace ends up holding a boolean,
      never a figure.
  D9  The 80 % and 100 % warnings are volume warnings against the plan's
      `ingest_gb`, computed by FerrisTrace from `usage_hourly` the way the
      Cost meter already does (lot 15), identical on Free and Pro. The
      budget cap crossing (D8) is a second, separate warning that exists
      only on Pro and never carries an amount.
  D10 Checkout and the customer portal are Stripe's hosted pages:
      FerrisTrace asks for a URL and redirects to it, and renders no card
      field, no price and no tax line. Tax is Stripe Tax.

Invariants
  - With no Stripe secret key and webhook secret configured, no billing
    route does anything beyond reporting each organisation's own plan, and
    the ingest and control plane paths are byte-for-byte what they are
    today.
  - The quota decision for a given `Allowance` is the same with or without
    this lot: 20a changes when the map is read, never what a `Capped` or
    an `Overage` allowance does.
  - A counter, control plane or Stripe outage never refuses a payload the
    plan allows (ADR 0005, extended to the allowance refresh and to the
    billing adapter: neither sits on the path that decides admission).
  - A webhook event id is applied at most once, whether delivered once or
    many times.
  - No amount, currency or Stripe price appears in the code, a migration,
    this spec or a test.

Design
  Domain (`billing`, new context in `ferristrace-domain`, alongside
  `telemetry`):
    Value objects: `CustomerRef`, `SubscriptionRef`, `PriceRef`,
    `BillingEventId`, `PlanSlug` (opaque newtypes, named by role so the
    domain does not take the shape of one provider), `SubscriptionStatus`
    (closed enum: `Active`, `PastDue`, `Canceled`).
    Entity: `Subscription { org: TenantId, customer, subscription, status,
    price, period_end }`, gaining `budget_cap_reached` in 20d.
    Ports: `BillingGateway` (`checkout_url`, `portal_url`,
    `report_usage(org, hour, over_by)`, `verify(signature, body) ->
    Result<WebhookEvent, WebhookError>`); `BillingAccounts`, a repository
    port for `Subscription`, registered like any other (ADR 0023/0024).
    Use cases on `Ferristrace<S>`: `start_checkout`, `open_portal`,
    `apply_webhook` (verifies, checks `billing_events`, upserts
    `Subscription`, and on exhausted retries moves `orgs.plan_id` to
    `free`, all in one transaction).

  Allowances (20a, no Stripe):
    `load_allowances`'s map moves behind a holder the quota only reads
    (`std::sync::RwLock`, no new dependency). A new periodic task in
    `apps/server`, `REFRESH_ALLOWANCES_EVERY = 20s`, re-reads
    `load_allowances` and replaces the map; a failed read logs and keeps
    the map it has (ADR 0039). `StaticQuota::admit` is unchanged: a map
    lookup, no I/O.

  Postgres:
    Migration: `plans.price_ref text UNIQUE NULL`. No plan row is
    inserted; `sync_plans` writes the configured ones on start.
    Migration: `billing_subscriptions` (`org_id` PK/FK `orgs`,
    `stripe_customer_id` UNIQUE, `stripe_subscription_id` UNIQUE, `status`,
    `period_end` bigint). 20d adds `budget_cap_reached boolean default
    false` to this table in its own migration.
    Migration: `billing_events` (`event_id` text PRIMARY KEY, `received_at`
    bigint). `apply_webhook` inserts into it before anything else in the
    same transaction; a unique violation means the event was already
    applied, and the use case returns `Ok(())` without touching
    `billing_subscriptions` again.

  Stripe adapter (`ferristrace-stripe`, new crate, sibling of `postgres`,
  `clickhouse`, `redis`, `infra`):
    Implements `BillingGateway` against Stripe's HTTP API: checkout
    sessions, billing portal sessions, meter events for usage, and
    `Stripe-Signature` verification. Depends on nothing but `domain` and an
    HTTP client, the same shape ADR 0023 gives every other backend.

  `apps/server`:
    `Stack::Billing: BillingGateway + Send + Sync + 'static`;
    `Adapters<S>.billing: Option<S::Billing>`, `None` without a configured
    secret key and webhook secret — the same optionality `Users`,
    `Agents` and the span stores already have. With `[stripe]` set, the
    server refuses to start without a control plane, a `free` plan and at
    least one plan with a `price`. A `[stripe]` config section
    (secret key, webhook secret, meter name) documented in
    `deploy/README.md` and the Helm chart values, both as secret
    references, never a literal value.

  `crates/api`:
    `POST /api/orgs/{org}/billing/checkout`, `POST
    /api/orgs/{org}/billing/portal` (member-authenticated, return a URL),
    `POST /api/billing/webhook` (no session; authenticated by Stripe's
    signature instead). Refused with 404, not 501, when no billing adapter
    is configured, so a self-hosted deployment sees no trace of the route.

  Usage reporting and overage (20c):
    Migration: `usage_hourly.stripe_reported_at bigint NULL`. After
    `record_hours`, `roll_up_usage` reports every unreported Pro hour past
    its included volume — the hour just rolled up and any earlier hour
    still carrying `stripe_reported_at IS NULL` — to
    `BillingGateway::report_usage`, keyed by the organisation and
    `BillingHour::key()`, the key Stripe's meter event idempotency uses so
    a retried delivery never double-counts. `stripe_reported_at` is set
    only once the call succeeds; a failed call is logged (ADR 0005's
    shape: billing accuracy is what is at risk, never ingestion) and the
    hour stays unreported for the next rollup to try again. Skipped for a
    Free organisation, and for every organisation when no billing adapter
    is configured.

  Budget cap and warnings (20d):
    Web: a cap field on the Billing settings page; saving it calls a new
    route that asks Stripe to create or update the alert (D8) and stores
    only its id. A crossing webhook sets `budget_cap_reached`; the
    allowance refresh (20a) serves `Capped(DecompressedBytes::new(0))` for
    that organisation while it is set, clearing it when a new billing
    period's webhook arrives.
    80 %/100 % volume warnings: a check beside the existing monthly
    rollup compares the month's total (already read for the Cost meter,
    lot 15) to the plan's `ingest_gb` and fires once per threshold per
    month, delivered through the existing alert mailer and the inbox —
    the same channels an alert notice already uses, not a new one.

  Web (`Billing.tsx`, already routed as a static mock):
    Reads the organisation's plan, status and period from a new endpoint.
    Free shows "Upgrade to Pro" to checkout; Pro shows "Manage billing" to
    the portal, the budget cap field (20d), and the volume warnings (20d).

Out of scope
  - Any amount, currency, invoice or tax computed by FerrisTrace.
  - Per seat pricing, a price per signal or per project, a third plan.
  - The paid 90 day retention option: it waits for cold data to be on
    managed object storage (ADR 0036), a separate lot.
  - A second payment provider, or a trait designed ahead of one.
  - Backfilling a Stripe customer or subscription for an organisation that
    upgraded before this lot: there are none yet.

Open questions
  Q1  Does the 90 day retention option, once it ships, apply to the whole
      volume or only to the overage past 100 GB?
      Leaning: the whole volume. A plan sets one `retention_days` today
      (D1), read uniformly by the span store; billing only the overage at
      30 days and the included volume at 90 would need a second retention
      figure per organisation that nothing else in the system has, for a
      distinction a customer reading "90 day retention" would not expect.
      Decide when that lot is specified, not here.
  Q2  What does the OTLP response say when a Free organisation reaches its
      limit, over HTTP and gRPC?
      Options:
      (a) Keep today's exact wording, "the tenant has used its allowance
          of {N} bytes" (`IngestError::Capped`'s message, already shared
          by HTTP 429 and gRPC `RESOURCE_EXHAUSTED`) — unchanged for every
          deployment, billing-aware or not.
      (b) Change that message to name the plan and the monthly volume —
          pulls a `billing` concept into the `telemetry` error, the
          ingest use case's own layer, for a self-hosted stack that has no
          plan to name.
      (c) Keep (a) in the domain, and let `crates/api`'s existing
          domain-error-to-response mapping add a line naming the plan and
          suggesting an upgrade, only when a billing adapter is
          configured. Self-hosted keeps (a) exactly.
      Recommendation: (c). It keeps `telemetry` free of `billing`
      (hexagonal boundaries, ADR 0023) and keeps the self-hosted response
      unchanged, while a hosted sender is told why in the same response.

Slices
  20a  Allowances refresh every 20 seconds while the server runs. No
       Stripe. Supersedes ADR 0004.
  20b  `billing` context, `ferristrace-stripe`, `free` and `pro` rows,
       checkout, portal, webhooks, the Billing settings page. Pro is
       capped at 100 GB in this slice — the `Overage` branch exists but
       20c is what reports it.
  20c  `roll_up_usage` reports hourly overage to Stripe's meter; Stripe
       bills it.
  20d  The budget cap and the 80 %/100 % warnings.

Acceptance (20a)
  @spec-l20a-1  A plan change written to `orgs`/`plans` is reflected in
                `admit`'s decision within one refresh interval, with no
                restart.
  @spec-l20a-2  A refresh that cannot reach the control plane logs an
                error and leaves the previous map in place; `admit`'s
                decision for every tenant is unchanged.
  @spec-l20a-3  `admit` makes no call to Postgres or Redis beyond what it
                makes today; only the refresh task does.
  @spec-l20a-4  Two consecutive successful refreshes with the same data
                leave `admit`'s decision unchanged.

Acceptance (20b)
  @spec-l20b-1  A new organisation is on `free`; ingestion past 30 GB in a
                month is refused, HTTP 429 and gRPC `RESOURCE_EXHAUSTED`.
  @spec-l20b-2  Starting checkout for an organisation with no Stripe
                customer creates one and returns a URL; a second checkout
                for the same organisation reuses the customer.
  @spec-l20b-3  An active `customer.subscription.*` webhook, applied once,
                moves the organisation to `pro` and records the
                subscription; applied again with the same event id changes
                nothing.
  @spec-l20b-4  A webhook with an invalid signature is refused and applies
                nothing.
  @spec-l20b-5  A webhook reporting exhausted payment retries moves the
                organisation back to `free`, deletes nothing, and the next
                allowance refresh enforces 30 GB again.
  @spec-l20b-6  With no Stripe secret key or webhook secret configured,
                the checkout, portal and webhook routes answer 404 and
                every organisation reads as its own plan on the settings page.
  @spec-l20b-7  A Pro organisation ingests past 100 GB in the month and is
                admitted; `Admission::Overage` is reported with the bytes
                over.
  @spec-l20b-8  The Billing settings page shows Free with an upgrade link,
                or Pro with a portal link and the current period.

Acceptance (20c)
  @spec-l20c-1  An hour rolled up for a Pro organisation with usage past
                its included volume is reported once to the billing
                gateway, keyed by organisation and hour.
  @spec-l20c-2  Rolling up the same hour twice reports it to the gateway
                once.
  @spec-l20c-3  An hour with no usage past the included volume, a Free
                organisation's hour, and any hour when no billing adapter
                is configured, are never reported.
  @spec-l20c-4  A billing gateway outage during the rollup does not stop
                `record_hours` from writing `usage_hourly`. The hour's
                `stripe_reported_at` stays null, and the next rollup
                reports it.

Acceptance (20d)
  @spec-l20d-1  Setting a budget cap calls the billing gateway once and
                stores only an alert id, never the amount.
  @spec-l20d-2  A webhook reporting the cap crossed sets
                `budget_cap_reached`; the next allowance refresh refuses
                further ingestion for that organisation with
                `Admission::Capped`.
  @spec-l20d-3  A webhook for the new billing period clears
                `budget_cap_reached`; the next refresh admits again within
                the plan's volume.
  @spec-l20d-4  An organisation that crosses 80 % then 100 % of its plan's
                `ingest_gb` in a month is warned once per threshold, by
                mail and in the interface, on Free and on Pro alike.
  @spec-l20d-5  No warning or stored state in 20d carries a currency
                amount.

Verify (exit)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace       (Postgres suite included; Stripe is never
                                called, only an in-memory `BillingGateway`
                                and recorded webhook bodies)
  pnpm --dir web typecheck      (and the Billing page checked in a browser
                                against a local stack with Stripe test
                                keys for 20b onward)
  Run before every slice's own pull request, not once at the end.
