# Lot 12 — Uptime monitoring

Goal
  Measure availability from outside the application with scheduled HTTP
  probes, and let a status page component report a measured uptime instead
  of one inferred from the spans the application sends.

Context
  Lot 2 section 2E built status pages. A component derives its status from
  an alert's severity or from a service's error rate over
  `SERVICE_WINDOW_SECS`, through `status::derivation::derive`.

  That derivation has a blind spot, and it is in the code today:

  ```rust
  pub fn by_error_rate(requests: u64, errors: u64) -> ComponentStatus {
      if requests == 0 {
          return ComponentStatus::Operational;
      }
  ```

  A service that is completely down sends no spans. Zero requests, so the
  public page reads **Operational** during a total outage. The derivation
  cannot tell "healthy" from "silent", because it only sees what the
  application reports, and a dead application reports nothing. It is also
  blind to everything outside the process: DNS, an expired certificate, a
  CDN, a load balancer that stopped routing.

  The page has no history either: no uptime figure and no per-day bars,
  which is the element a visitor uses to judge whether a service is
  usually reliable.

  A probe answers both. It calls the URL and records what happened, so
  "no answer" is down, and a percentage of passed probes is a measured
  number rather than a derived one. That is what makes a plain uptime
  percentage defensible on the public page.

  No issue tracked this; the lot comes from the decision to put Monitoring
  beside Status page in Reliability.

Glossary
  Monitor      A definition: url, method, interval, timeout, expectation.
               Not "check" — a check is one run of it.
  Probe        One execution of a monitor, with its outcome and latency.
  Outcome      Up, Down with a reason, or Inconclusive with a fault.
               Never "error": a 500 the monitor expects is Up.
  Fault        A failure on this side — no egress, our resolver, a tick
               that overran. The target's health is unknown, not bad.
  Confirmation The rule that a component flips to down only after
               `CONFIRM_FAILURES` consecutive Down probes. One failed
               probe is a blip, not an outage.
  Reading      What the status page is told: Measured, Stale, or
               Unmeasured. A page that admits it does not know is
               reliable; one showing a stale green is worse than none,
               because it silences the people about to report the outage.
  Uptime       The share of a monitor's *conclusive* probes that passed
               over a window. Measured. Not derived from span error rates,
               and never called availability, which this does not measure.

Invariants
  - A monitor's interval is one of a fixed set, never free-form.  [type: `Interval` enum]
  - A monitor's timeout is between 1 and 30 seconds.  [type: `Timeout::new -> Result`]
  - A monitor's url is http or https.  [type: `ProbeUrl::try_from -> Result`]
  - A monitor never probes a host that resolves to a private, loopback,
    link-local, carrier-grade NAT, benchmarking or reserved address, so a
    monitor cannot read cloud metadata or reach inside the network.
    Reuses `ferristrace_infra::private`.  [test, and the adapter refuses at probe time]
  - A probe follows no redirect.  [type: `Policy::none()` in the adapter]
  - One replica probes a given monitor in a given tick.  [test, over `pg_try_advisory_xact_lock`]
  - A day with no probe is absent from the history, never reported as up.  [test]
  - A monitor belongs to one organisation and every read is narrowed to it.  [test]
  - A probe this side could not carry out is Inconclusive, never Down, so
    our own trouble never publishes someone else's outage.  [type: `Outcome::Inconclusive`]
  - A component flips to down only after `CONFIRM_FAILURES` consecutive
    Down probes; one Up in between resets the count.  [test, pure function]
  - An Inconclusive probe counts in neither the numerator nor the
    denominator of an uptime figure, so our trouble neither lowers nor
    flatters the number.  [test]
  - A monitor whose newest probe is older than `STALE_AFTER_INTERVALS`
    intervals reads Stale, and one that never probed reads Unmeasured.
    Neither ever reads as its last known status.  [type: `Reading` enum]

Out of scope
  - Agents in other regions, and the quorum between them. One origin only;
    the public page says so. A regional outage is indistinguishable from a
    global one in this lot, and a fault on this side takes every monitor
    with it at once — which is why such a probe is Inconclusive and not
    Down. Lot 13.
  - Hosting the status page outside the product's failure domain. A page
    served by the stack it describes cannot report that stack's own
    outage; the dead man's switch below is the honest admission, not a
    fix. Lot 13.
  - TCP, ICMP and DNS checks. HTTP(S) only, so the model carries one case
    rather than an enum of protocols nothing asks for yet.
  - Raising an alert or opening an incident when a monitor goes down.
    Alerts are rule-driven on stored signals; joining the two is its own lot.
  - Scheduled maintenance windows and component groups on the status page.
  - A public API for uptime. The figure reaches visitors through the
    status page only.

Design
  Domain: `crates/domain/src/monitoring/`
    `MonitorId` (identifier macro, as `status` does)
    `Monitor { id, org, project: Option<ProjectId>, name: MonitorName,
               url: ProbeUrl, method: ProbeMethod, interval: Interval,
               timeout: Timeout, expect: Expectation, enabled: bool,
               created_ms, updated_ms }`
    `ProbeUrl` — newtype over `Box<str>`, `TryFrom<&str>`, http/https, capped
    `ProbeMethod` — enum Get | Head
    `Interval` — enum Half | Minute | FiveMinutes | FifteenMinutes, with `secs()`
    `Timeout` — newtype over `u8` seconds, `new -> Result`
    `Expectation { status: ExpectedStatus, body: Option<ExpectedText> }`
    `ExpectedStatus` — enum Exact(u16) | Successful (2xx)
    `Probe { monitor: MonitorId, at_ms, outcome: Outcome, latency_ms: Option<u32> }`
    `Outcome` — enum Up | Down(Reason) | Inconclusive(Fault)
    `Reason` — enum Status(u16), Body, Timeout, Dns, Connect, Tls, Refused
                 — the target's failure
    `Fault`  — enum NoEgress, OurResolver, Overran, ShuttingDown
                 — this side's failure, so the target is unknown
    `CONFIRM_FAILURES: u8 = 3`
    `STALE_AFTER_INTERVALS: u32 = 3`
    `HISTORY_DAYS: u16 = 90`
    `Uptime { conclusive: u64, up: u64 }` with `percent() -> Option<f64>`,
      absent rather than 100 when nothing conclusive ran. Inconclusive
      probes are in neither field.
    `DayOfUptime { day_ms, up: u64, down: u64, inconclusive: u64 }` with
      `reading() -> Reading`, so a day we did not measure renders as its
      own thing and not as a green bar.
    `Reading` — enum Measured(ComponentStatus) | Stale { at_ms } | Unmeasured
    Pure functions the domain owns, all testable with no adapter:
      `due(&Monitor, last_ms, now_ms) -> bool`
      `confirmed(recent: &[Probe]) -> ComponentStatus` — down only when the
        newest `CONFIRM_FAILURES` probes are all Down; one Up resets it;
        Inconclusive probes are skipped, never counted as either
      `read(recent: &[Probe], interval: Interval, now_ms) -> Reading`
      `uptime_of(&[DayOfUptime]) -> Uptime`
    Judging a response against an `Expectation` is the adapter's job; the
    domain never sees an HTTP type.

  Ports: owned by `domain::monitoring::ports`, all generic (static dispatch,
    per rust-dev: no dyn, no async-trait)
    `Monitors`     — list, one, create, update, remove, `due(now_ms, limit)`
    `Prober`       — `probe(&Monitor) -> impl Future<Output = Probe>`
    `ProbeStore`   — `record(&[Probe])`,
                     `recent(&[MonitorId], take)` — the newest few per
                       monitor, because confirmation and freshness both
                       need more than the single latest probe,
                     `history(MonitorId, days) -> Vec<DayOfUptime>`

  Adapters
    `Monitors`    → `crates/postgres/src/monitoring.rs`, table `monitors`
    `ProbeStore`  → `crates/clickhouse/src/clickhouse/probe_*.rs`, table `probes`
    `Prober`      → `crates/infra/src/prober.rs`, reqwest, reusing
                    `private()` and `Policy::none()` exactly as `HttpWebhooks` does

  Errors: `MonitorError` at the application boundary, `StoreError` from the
    stores, `Probe` carries its own failure as `Outcome::Down(Reason)` — a
    failed probe is data, not an error.

  Status page link
    `status::entities::Link` gains `Monitor(MonitorId)`; the migration's
    `link_kind` CHECK gains `'monitor'`; `derive` gains an arm reading the
    monitor's latest outcome through `Observed`. `Link::Manual`,
    `Link::Service` and `Link::Alert` are untouched.

  The loop
    `apps/server/src/main.rs` spawns `run_probes` beside `evaluate_alerts`,
    on `ticking(PROBE_EVERY)`, calling `app.run_probes()` which returns
    `Tick::Skipped` or a count, guarded by an advisory lock on its own key.

Acceptance (Gherkin)
  @spec-l12-1  A monitor whose url answers 200 inside its timeout records
               Up with a latency.
  @spec-l12-2  A monitor expecting 2xx whose url answers 500 records Down
               with the status reason, and the body is not read.
  @spec-l12-3  A monitor whose url does not answer inside its timeout
               records Down with the timeout reason and no latency.
  @spec-l12-4  A monitor expecting a text that the body does not contain
               records Down with the body reason, although the status passed.
  @spec-l12-5  Creating a monitor whose host resolves only to a private,
               loopback or link-local address is refused, and no monitor
               is stored.
  @spec-l12-6  Two replicas ticking the same minute produce one probe per
               due monitor, not two.
  @spec-l12-7  The history of a monitor over ninety days returns one entry
               per day that holds a probe, and no entry for a day before
               its first probe.
  @spec-l12-8  A monitor's uptime over a window with no probe is absent,
               not a hundred per cent.
  @spec-l12-9  A status page component linked to a monitor reads down when
               the monitor's last three probes are all down, including when
               its service sent no spans at all.
  @spec-l12-10 A monitor read never crosses organisations.
  @spec-l12-11 Every monitor create, update and delete appears in the audit log.
  @spec-l12-12 A single failed probe between passing ones leaves the
               component operational, and a third consecutive failure
               flips it to down.
  @spec-l12-13 A probe this side could not carry out is stored as
               inconclusive, leaves the component's status unchanged, and
               moves neither the numerator nor the denominator of uptime.
  @spec-l12-14 A monitor whose newest probe is older than three intervals
               reads stale and not its last status, and one that never
               probed reads unmeasured.
  @spec-l12-15 A day holding only inconclusive probes renders as its own
               state in the ninety-day history, not as an operational day.
  @spec-l12-16 When no probe at all has been recorded for longer than
               three intervals, the public page says it is not measuring
               rather than showing the last known status.

Simulation
  Yes, and it now carries most of the lot's value. `due`, `confirmed` and
  `read` are pure domain functions over a sequence of probes and a clock,
  so the cases that matter are unit tests with `FixedClock` and no real
  time: a monitor not yet due; one due exactly on its interval; a single
  failure between passes; three consecutive failures; an Inconclusive in
  the middle of a failing run; a newest probe three intervals old; no probe
  at all. These are the states a false outage or a stale green comes from,
  and none of them needs a network. The prober is an adapter and is tested
  against a local server in an integration test, not simulated.

Workstreams
  W1 domain + ports        — owns `crates/domain/src/monitoring/**`, no deps
  W2 postgres adapter      — owns `crates/postgres/src/monitoring.rs` and the
                             migration, depends on W1's frozen contracts
  W3 clickhouse adapter    — owns `crates/clickhouse/src/clickhouse/probe_*.rs`
                             and its migration, depends on W1
  W4 prober + loop         — owns `crates/infra/src/prober.rs` and the
                             `run_probes` loop, depends on W1
  W5 api                   — owns `crates/api/src/monitor_routes.rs`, depends on W1-W4
  W6 web                   — owns `web/src/data/monitors.ts` and
                             `web/src/pages/Monitoring.tsx`, depends on W5
  W7 status page link      — owns the `Link::Monitor` arm, the CHECK change
                             and the editor's fourth link kind, depends on W1-W3
  Shared seams (ports.rs, mod.rs, read.rs impl blocks, explore_routes-style
  registration, nav.ts, routes.tsx) are written once, up front, before any
  workstream starts — the collision lesson from lot 11b.

Frozen contracts
  ```rust
  pub trait Prober {
      fn probe(&self, monitor: &Monitor) -> impl Future<Output = Probe> + Send;
  }

  pub trait ProbeStore {
      fn record(&self, probes: &[Probe]) -> impl Future<Output = Result<(), StoreError>> + Send;

      fn recent(
          &self,
          monitors: &[MonitorId],
          take: u8,
      ) -> impl Future<Output = Result<Vec<Probe>, StoreError>> + Send;

      fn history(
          &self,
          monitor: MonitorId,
          days: u16,
      ) -> impl Future<Output = Result<Vec<DayOfUptime>, StoreError>> + Send;
  }
  ```

Risks / open questions
  - The structural one: a status page served by the stack it describes
    cannot report that stack's own outage. If the server stops, no probe
    runs, and without the dead man's switch the page would sit on its last
    green while everything burned. `Reading::Stale` and `Unmeasured` are
    the admission; they are not a fix. Only a second origin outside this
    failure domain is, and that is lot 13. The public page says which
    origin measured it, or the number overstates what is known.
    Owner: W6, wording reviewed with the user.
  - `CONFIRM_FAILURES = 3` trades detection delay for false outages: at a
    thirty-second interval a real outage is published after about ninety
    seconds. Shrinking it publishes blips; growing it hides short
    outages. The number is a starting point to be measured, not a truth.
    Owner: W1.
  - No redirects means a monitor on an http url that permanently redirects
    to https records 301. Mitigation: `ExpectedStatus::Exact(301)` works,
    and the editor warns when a url is http. A safe redirect follower
    re-checking `private()` at every hop is deliberately not built here.
  - Probe retention in ClickHouse: the `probes` table needs a TTL. Ninety
    days of history means the TTL cannot be shorter than the window the
    status page shows. Owner: W3.
  - How many monitors one replica probes per tick before the tick overruns.
    `due(now_ms, limit)` caps it; the limit and the lag it causes are to be
    measured, not guessed. Owner: W4.
  - `Interval::Half` at thirty seconds is the floor. Anything faster is a
    load generator aimed at someone else's service, so it is not offered.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo test --workspace --locked
  pnpm --dir web typecheck
  The ClickHouse suites run against a live server, never with
  CLICKHOUSE_URL unset, or they return without executing a query.
