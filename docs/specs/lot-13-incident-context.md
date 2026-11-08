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
    13B  Slack and Discord                    13B1 and 13B2 delivered, 13B3 next
    13C  Insights: MTTA, MTTR, frequency per service    delivered

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

  13B Slack and Discord
    Three steps, each its own pull request, cheapest first.
    13B1  Alerts to a chat channel. New channel kinds `slack` and `discord`
          next to `email` and `webhook`. The target is the incoming webhook
          URL the service gives; it is accepted only on the service's own
          host and path (`hooks.slack.com/services/`, `discord.com/api/
          webhooks/` and its ptb, canary and discordapp.com spellings), over
          https. No app, no OAuth, no secret: it works the same on a cloud
          and a self-hosted deployment. Delivery rides the alert outbox
          (retries, abandonment, private-address guard) with a payload per
          service: a coloured Slack attachment, a Discord embed, each with
          the alert, severity, change, value, labels, culprit pods and
          versions, and a link to the alert. User-controlled text is
          escaped (Slack) or sent with `allowed_mentions` empty (Discord),
          so a rule name cannot ping a channel.
    13B1b A Discord message designer. A Discord channel carries an optional
          design: text templates (message text, title, description, author,
          footer), a colour per state, a thumbnail, which standard facts to
          show (severity, change, value, labels, culprits, link, time),
          up to 5 custom fields, and roles or users to ping when an alert
          fires. Templates take placeholders `{alert}` `{severity}`
          `{state}` `{from}` `{to}` `{value}` `{link}` `{pods}`
          `{versions}` `{nodes}` and `{label.<key>}`; an unknown name is
          refused when the design is saved. Substituted values can never
          mention anyone (an `@` or `<` from alert data is neutralised);
          only the roles and users the admin names are pinged, and only on
          firing, never `@everyone` or `@here`. No design means today's
          message. The channel editor gets a design panel with a live
          preview rendered by the server, so the preview and the delivery
          cannot drift. Slack keeps its fixed look for now.
    13B2  Incident updates in chat. A Slack or Discord channel can opt in
          ("Also post incident updates here"). From then on, every entry
          appended to an incident of the organisation that a person would
          want to read is queued for it: opened, status, severity, owner,
          title, public flag, comments, paging and acknowledgement. Alert
          state changes, deploys and attachments are left out: alerts
          already notify their own channels and deploys are noise. The
          queue is an outbox filled in the transaction that writes the
          entry, drained by a loop that takes its own lead, with the same
          retries as alerts. The message is a coloured attachment or
          embed with the incident title, severity, status, a link, and the
          comment text; nothing in it can mention anyone. Decided against
          the first draft: a channel opts in instead of a project naming
          one, so incidents with no project are heard too, and several
          channels may listen. Every comment is posted, since the channel
          is the team's own; per-comment public flags still arrive with
          the status page.
    13B3  Commands. A Slack app (slash command, buttons) and a Discord
          application (interactions endpoint) to declare, acknowledge,
          resolve and comment. Inbound calls are verified (Slack signing
          secret HMAC with a 5 minute window, Discord Ed25519). A chat
          user acts as a FerrisTrace account only after linking: Slack by
          verified email, Discord by a one-time code typed in the app.
          Needs a hosted app per deployment; self-hosted admins bring
          their own app credentials.

  13C Insights
    `GET /api/orgs/{org}/incidents/insights?from_ms&to_ms&project`, default
    the last 30 days, at most 366. Over the incidents opened in the window:
    count, open and resolved, and for each group the time to acknowledge
    and the time to resolve as a mean and a median with their sample count.
    Groups: overall, by severity, by service, and a count per UTC day.
    Time to resolve is opened to resolved, over resolved incidents only.
    Time to acknowledge is opened to the first response: a status change,
    an owner taken, or an acknowledged page (an owner cleared does not
    count). An incident with no response is left out of that average, not
    counted as zero. By service uses the services of the alert that opened
    the incident, counts the incident under each of them, and groups a
    hand-declared incident under none; the total counts it once. Read-only,
    narrowed to the caller's project, nothing new is stored.
    Web: an Insights tab under Reliability.

Open
  Q1  Who uses it: an SRE team living in Slack, or small teams wanting one
      tool? Decides whether 13B comes before 13C.
  Q2  13B3 needs an app per deployment. Cloud only, or also self-hosted
      with the admin's own app credentials (the draft assumes both)?
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

Delivered with 13B1
  Channel kinds `slack` and `discord` (migration `20261121000001`). A chat
  channel has no secret and needs no at-rest seal. Alerts only; incident
  events follow in 13B2.
  @spec-l13-7  A Slack or Discord channel accepts only its service's own
               webhook URL and carries no secret.
  @spec-l13-8  An alert state change reaches a chat channel as that
               service's message, and a rule name cannot mention anyone.

Delivered with 13B1b
  @spec-l13-9   A design with a template, a colour, extra fields and a role
                ping renders into the Discord message that is delivered.
  @spec-l13-10  Alert data cannot mention anyone through a placeholder.
  @spec-l13-11  A design naming an unknown placeholder, a bad colour, a
                non-https thumbnail or more than 5 custom fields is refused.

Delivered with 13B1c
  A rule carries `mentions {roles, users}` (Discord ids). When it starts
  firing, every Discord channel it uses pings them, joined with the pings
  of the channel design and without duplicates. Never on resolution, never
  `@everyone`. Slack ids differ, so a Slack channel does not use them yet.
  @spec-l13-12  A rule keeps its mentions, refuses a bad id, and a firing
                notification pings the rule's and the channel's ids once.

Delivered with 13B2
  Migration `20261122000002` adds `incident_outbox`. The opt-in lives in the
  channel design (`incident_updates`), so no column was added to channels.
  @spec-l13-13  An announced entry reaches each subscribed chat channel once
                and no other channel; alerts, deploys and attachments are
                not announced.

Delivered with 13C
  @spec-l13-14  The insights count the incidents opened in a window and time
                their acknowledgement and resolution, per severity and per
                service.
