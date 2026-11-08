# Lot 13: the incident knows what happened around it

Goal
  When an incident opens, show what changed and what broke around the
  service it concerns, without the responder searching: deploys, new
  errors, the infrastructure it runs on, the failing traces. Then let the
  team run the incident from the chat they already live in.

Context
  Lot 2 gave incidents, a timeline, on-call, postmortems and status pages.
  Already there: `deploy` entries join the timeline of an incident whose
  rule names a service (releases watch); an alert carries its culprits
  (pods, nodes, versions). Missing: new issues, the resource the service
  runs on (lot 12), failing traces, and any chat surface.

  One pull request per part, in this order:
    13A  Incident context, read when the incident page opens
    13B  Chat surface (Slack first)           spec only, see Open
    13C  Insights: MTTA, MTTR, frequency per service

Glossary
  Context     what the product knows about an incident's services over a
              window: deploys, new issues, resources, failing traces.
  Window      from 2 hours before the incident opened to its resolution,
              or now while it is open.
  Concerned   a service named by the opening rule, an attachment, or a
              trace attached to the incident.

Invariants
  - Context is computed at read time from the organisation's own data and
    narrowed to the caller's project.  [test]
  - Context never writes: the timeline stays append-only and records only
    what a person or the system chose to add.  [test]
  - An incident with no concerned service shows an empty context, not an
    error or a guess.  [test]
  - A silent resource carries no figures, as in the infrastructure list.
    [test]
  - Every list is capped and says how many it left out.  [test]

Out of scope
  - Root cause by an agent (lot 5 can post hypotheses through the API).
  - Writing to the context: the responder attaches what matters by hand.
  - Services that no telemetry names.

Design
  13A Context
    `GET /api/orgs/{org}/incidents/{id}/context` answers
      {services:[s], window:{from_ms,to_ms},
       deploys:[{service,version,environment,at_ms}],
       issues:[{id,title,service,first_seen_ms,occurrences}],
       resources:[{kind,id,name,silent,cpu_percent,memory_bytes}],
       traces:[{trace_id,service,operation,duration_ms,at_ms}],
       left_out:{deploys,issues,resources,traces}}
    deploys: ReleaseReader over the window. issues: issues first seen
    inside the window (not older ones that recur). resources: the
    infrastructure resources whose `services` list holds a concerned
    service (lot 12F). traces: the slowest failing traces of the concerned
    services in the window, at most 5.
    Composed in the domain by an `IncidentContext` service over existing
    ports; no new table, no new migration.
    Web: a "Context" card on the incident page, each row a link to the
    release, issue, resource or trace. Empty sections are hidden.

  13B Chat surface
    Declare, update, resolve and comment from Slack; one channel per
    incident; the timeline mirrors both ways. Needs a Slack app, OAuth and
    an inbound endpoint. See Open.

  13C Insights
    MTTA, MTTR and incident count per service and per severity over a
    window, from the timeline. Read-only, no new data.

Open
  Q1  Who uses it: an SRE team living in Slack, or small teams wanting one
      tool? Decides whether 13B comes before 13C.
  Q2  Slack needs a hosted app and OAuth. Cloud only, or also self-hosted
      with the admin's own Slack app?
  Q3  Is "first seen inside the window" the right test for a new issue, or
      also an issue whose rate rose sharply?

Acceptance
  @spec-l13-1  An incident opened by a rule on `checkout` shows the deploys
               of `checkout` in the last 2 hours.
  @spec-l13-2  An issue first seen after the window opens appears; one first
               seen before does not.
  @spec-l13-3  The host or container that served `checkout` appears with its
               figures; a silent one appears with none.
  @spec-l13-4  An incident with no concerned service answers an empty
               context with status 200.
  @spec-l13-5  A read key bound to another project sees none of it.
  @spec-l13-6  More than the cap is cut and `left_out` counts the rest.

Simulation
  No. The window and the composition are pure functions of the readers.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck

Delivered with 13A
  `GET /api/orgs/{org}/incidents/{id}/context` and the "Context" card on the
  incident page. The concerned services are the ones the opening rule names
  (`alert.services`); attachments do not add services yet. The window is two
  hours before the incident opened until it resolved, or now. Resource
  figures are read over the five minutes before the end of the window.
  Q3 stays open: a new issue is one first seen inside the window.
