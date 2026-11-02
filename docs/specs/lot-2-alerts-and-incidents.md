# Lot 2: alerts and incidents

Goal
  Alert on any signal FerrisTrace keeps, deliver the alert to a person or a
  system, and run the incident it starts from the first page to the
  postmortem, in the same tool as the traces, logs and metrics.

Context
  Milestone "2 · Alerts and incidents": #126, #127, #177, #178, #179, #180.
  Lot 1 made traces, logs and metrics queryable per organisation and
  project. The `alerts` table exists in Postgres (name, definition jsonb,
  enabled) with no code behind it; the Alerts, Status page editor and public
  status pages are mock screens. Mail goes through the `Mailer` port
  (SMTP); there is no outbound webhook. The server is one binary that may
  run as several replicas (chart from lot 0).

  One pull request per part, in this order:
    2A  Alert rules, evaluation, history, channels     #126
    2B  Incidents and their timeline                   #177 #178
    2C  On-call schedules, pages, escalation           #179
    2D  Postmortem drafts                              #180
    2E  Public status pages                            #127

Glossary
  Rule        what to watch: a signal query, a condition, an evaluation
              window, a pending duration, a severity, channels.
  Evaluation  one run of a rule at a time t: the observed value and the
              resulting state.
  State       ok, pending (condition true for less than the pending
              duration), firing, or no_data.
  Channel     where a notification goes: email addresses or a webhook URL.
  Notification one delivery attempt of one state change to one channel,
              kept in an outbox until delivered or abandoned.
  Incident    a tracked disruption: title, severity (sev1..sev4), status
              (investigating, identified, monitoring, resolved), owner,
              public flag, timeline, attachments.
  Page        a notification to the person on call, acknowledged or
              escalated.

Invariants
  - A rule is evaluated by exactly one replica at a time.  [test: a
    Postgres advisory lock per evaluation tick]
  - An evaluation reads only its organisation's (and project's) data.  [test]
  - A state change produces exactly one notification per channel, and a
    failed delivery is retried with backoff, then abandoned and recorded,
    never silently dropped.  [test]
  - A webhook never targets a private, loopback or link-local address
    unless the deployment allows it; it is signed with HMAC-SHA256 of the
    body under the channel's secret.  [test]
  - Every incident timeline entry is appended, never rewritten; status and
    severity changes write one entry each.  [test]
  - A public status page shows only incidents marked public and components
    marked public, and never needs a session.  [test]
  - Every rule, channel, incident, schedule and status page write is
    audited (lot 0 audit log).  [test]

Out of scope
  - Anomaly detection, forecasts, composite rules.
  - SMS, phone calls, Slack and PagerDuty integrations (webhook covers
    them for now).
  - LLM-written summaries (lot 5 agents post them through the API).
  - Status page email subscriptions with double opt-in beyond a simple
    subscribe/unsubscribe link (2E keeps the simple form).

Design
  2A Alerts
    Rule definition (stored in `alerts.definition`, validated by the
    domain): `signal` one of
      traces: `{service?, operation?, environment?, project?, measure:
        count|error_rate|p50|p95|p99}`;
      logs: `{query: the logs search filters of lot 1, measure: count}`;
      metrics: `{name, aggregation, group_by: [], filter: {k: v}}`;
    `condition` `{op: >|>=|<|<=, threshold}`, `window` (1 min..24 h),
    `pending_for` (0..1 h), `severity` critical|warning|info (incidents use
    sev1..sev4), `channels` [ids],
    `no_data` (ok|no_data|firing). A rule with group_by yields one state
    per series (labels).
    Evaluator: a loop in the server every 60 s; each tick takes
    `pg_try_advisory_lock(ferristrace_alerts)`, lists enabled rules, reads
    the value through the same readers as the APIs (SpanReader, LogReader,
    MetricReader) over `[now - window, now]`, applies the condition and
    the pending duration, writes `alert_states` (current state per rule and
    label set) and `alert_events` (one row per state change, kept 30 days),
    and enqueues notifications in `alert_outbox`.
    Delivery: a second loop drains the outbox (same lock), email through
    the `Mailer` port, webhook through a new `WebhookSender` port (infra,
    reqwest, 10 s timeout, private-address guard, HMAC header
    `X-FerrisTrace-Signature: sha256=<hex>`), retries at 1, 5, 15, 60 min,
    then `abandoned`.
    Channels: table `alert_channels` (org, name, kind email|webhook,
    target, secret sealed with the at-rest seal when present).
    API (admins write, members read, read keys read):
      `GET/POST /api/orgs/{org}/alerts`, `GET/PATCH/DELETE
      /api/orgs/{org}/alerts/{id}`, `POST /api/orgs/{org}/alerts/{id}/test`
      (sends a test notification to its channels),
      `GET /api/orgs/{org}/alerts/{id}/history?from&to` (events),
      `GET/POST /api/orgs/{org}/channels`, `PATCH/DELETE .../channels/{id}`,
      `POST /api/orgs/{org}/alerts/preview` (evaluates a definition over the
      last window without saving, for the editor's chart).
    HTTP contract (frozen for the web):
      rule JSON `{"id","name","project":uuid|null,"enabled":bool,
        "signal": {"kind":"traces","service":s|null,"operation":s|null,
          "environment":s|null,"measure":"count|error_rate|p50|p95|p99"}
          | {"kind":"logs","services":[s],"severities":[sev],"q":s|null,
          "attrs":["k=v"],"measure":"count"}
          | {"kind":"metrics","name":s,"aggregation":agg,"group_by":[s],
          "filter":{k:v}},
        "condition":{"op":">|>=|<|<=","threshold":number},
        "window_s":60..86400,"pending_s":0..3600,
        "severity":"critical|warning|info","no_data":"ok|no_data|firing",
        "channels":[uuid],
        "state":{"status":"ok|pending|firing|no_data","value":number|null,
          "since_ms":number,"evaluated_ms":number|null,
          "series":[{"labels":{k:v},"status":..,"value":number|null}]}}`
        (`state` is read-only, absent from POST/PATCH bodies);
      list `{"alerts":[rule]}`; history `{"events":[{"at_ms","from","to",
        "value":number|null,"labels":{}}]}` newest first;
      channel JSON `{"id","name","kind":"email|webhook","target":s,
        "has_secret":bool}`, POST of a webhook returns `"secret"` once;
      preview body = a rule without id/state, answers `{"step_ns",
        "points":[[t_ns,value|null]],"threshold":number,"would_fire":bool}`;
      test answers `{"deliveries":[{"channel":uuid,"status":"sent|failed",
        "error":s|null}]}`; errors 400/401/403/404/422 like other routes.
    Web: Alerts page and editor wired (signal picker, query, condition,
    preview chart, channels), Settings > Project > Alerts channels.
  2B Incidents
    Tables `incidents` (org, project?, title, severity, status, owner,
    public, opened_at, resolved_at, opened_from_alert?), `incident_events`
    (append-only: kind opened|status|severity|owner|comment|alert|
    attachment|deploy, author account|api_key|agent|system, body, at),
    `incident_attachments` (trace id, logs query, metric query, label).
    Opening from a firing alert links the alert; later state changes of
    that alert append `alert` entries. Comments by members; status,
    severity, owner by members (an incident is everybody's job), deletion
    never (resolve instead).
    API: `GET/POST /api/orgs/{org}/incidents`, `GET/PATCH
    /api/orgs/{org}/incidents/{id}`, `POST .../incidents/{id}/comments`,
    `POST .../incidents/{id}/attachments`, `GET .../incidents/{id}/timeline`.
    Web: incidents list and incident page (timeline, attachments rendered
    as links to traces, logs and metrics, status and severity controls),
    "Declare incident" on a firing alert.
  2C On-call
    Tables `oncall_schedules` (team, timezone, rotation: members ordered,
    handoff weekday and time, length in days), `oncall_overrides` (person,
    from, to), `pages` (incident or alert event, target account, state
    triggered|acknowledged|escalated|resolved, escalation step).
    A rule or incident with `page: true` pages the team's current on-call
    through email (and the account's notification webhook when set);
    unacknowledged after `escalate_after` (default 15 min) pages the next
    in rotation, then the team admins. Acknowledge by link (signed token)
    or in the app.
    API: schedules CRUD per team, `GET .../oncall/now`, page ack/resolve.
    Web: Team > On-call (rotation, overrides, who is on call now).
  2D Postmortems
    `postmortems` (incident, status draft|published, body markdown,
    author, updated_at). Drafting is deterministic, from the timeline:
    summary placeholder, impact window (opened → resolved), timeline table,
    contributing alerts and deploys, attachments, action items section.
    Editing in the app (markdown), publish (audited); published ones are
    linkable from the incident and the status page when public.
  2E Status pages
    `status_pages` (org, name, slug unique, public, hide_from_search,
    theme), `status_components` (page, name, linked service or alert,
    order), `status_subscribers` (page, email, token). Component status
    derives from its alert (firing → down or degraded by severity) or
    service error rate; public incidents and their updates (status changes
    and comments marked public) show on the page.
    Public routes without a session: `GET /status/{slug}` (HTML from the
    web app), `GET /api/status/{slug}` (JSON), subscribe/unsubscribe by
    email with a signed link. Editor wired in the app.

Acceptance
  @spec-l2-1  A rule "error_rate of checkout > 5 % over 5 min, pending 2 min"
              goes ok → pending → firing as spans with errors arrive, and
              back to ok, with one history event per change.
  @spec-l2-2  Each firing and recovery sends one email and one signed
              webhook per channel; a failing webhook is retried and then
              recorded as abandoned.
  @spec-l2-3  Two server replicas never evaluate the same tick twice.
  @spec-l2-4  An incident opened from a firing alert carries the alert,
              records every status and severity change and comment in its
              timeline in order, and later alert changes append to it.
  @spec-l2-5  The person on call at the incident time is paged; without
              acknowledgement the page escalates after the delay.
  @spec-l2-6  Resolving an incident offers a postmortem draft containing
              its impact window and timeline.
  @spec-l2-7  A public status page served without a session shows its
              components' status and only public incidents.
  @spec-l2-8  Every write above appears in the audit log.

Simulation
  Yes for 2A and 2C: the evaluator and escalation depend on time. Their
  state machines are pure domain functions driven by a FixedClock in unit
  tests (pending, recovery, no data, escalation steps), so no real time
  passes in tests.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck && pnpm build

Amended: 2026-11-02 2B: an incident timeline records alert changes into or out of firing only, like notifications; `deploy` entries arrive with lot 3 deploy markers; per-comment public flags arrive with 2E.
Amended: 2026-11-02 2C as built: a rule or an incident pages a named schedule (`"page": {"schedule": uuid} | null` in the rule JSON, `page` on incident creation); the current on-call is `GET .../schedules/{id}/now`; pages resolve with their alert or incident, there is no resolve route; escalation goes through the rotation once, then pages the organisation's owners and admins once and stops; the delay is 15 minutes; teams have no admin role of their own, so organisation admins write schedules; acknowledgement links are HMAC-signed under a key derived from the at-rest key and last 24 hours.
Amended: 2026-11-02 2D as built: one postmortem per incident; drafting is deterministic markdown from the incident, its chronicle and attachments; redrafting an edited draft needs `?force=true` by an admin and never overrides a published one; `GET /api/orgs/{org}/postmortems` lists which incidents have one; the author is kept nullable so deleting an account keeps the postmortem; deploy entries join the draft with lot 3; the status page link is 2E.
Amended: 2026-11-02 2E as built: the overall status is the worst public component, raised to at least degraded while a public incident is open; an incident's opening shows as an `investigating` update; editors get a preview route before a page is public; a published postmortem of a public incident is served at `/api/status/{slug}/postmortems/{incident}` with email addresses redacted and names kept, since an admin both made the incident public and published it; confirm links last 7 days and unsubscribe links 90; `confirm` and `unsubscribe` are reserved slugs; making an incident public later does not email subscribers.
