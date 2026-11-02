# Lot 7: MCP behind a policy gateway

Goal
  Let a person on call ask an MCP client (Claude Code, Claude Desktop, any
  MCP host) to read FerrisTrace and act on incidents, while FerrisKey
  decides, call by call, what that person may do through it. The rules
  are the customer's own, written as Cedar policies in their realm; this
  lot does not ship rules, it ships what a rule can test: who calls, as
  which team and role, whether they are on call, which tool, on which
  organisation, project, environment, service, namespace or incident of
  which severity, and when, in the organisation's time.

Context
  FerrisTrace has no MCP surface. It has the pieces: agent tokens verified
  against FerrisKey's JWKS with scopes `telemetry:read`,
  `incidents:comment`, `incidents:resolve` (lot 5, crates/infra/src/jwks.rs,
  crates/domain/src/agents), and every read an on-call needs behind
  `/api/orgs/{org}/...` answering agent tokens (traces, logs, metrics,
  cluster, alerts, incidents).
  FerrisKey is already a PDP: AuthZEN 1.0 evaluation and batch evaluation
  at `/realms/{realm}/authzen/access/v1/evaluation[s]`, decided by Cedar
  4.14 policies stored per realm, called by a service account holding
  `evaluate_authorization`; token exchange (RFC 8693) with an `act` claim;
  RFC 8414 metadata, RFC 8707 resource indicators, dynamic and
  metadata-document client registration for MCP clients. Relation tuples
  exist in its model; their evaluation is on its roadmap, not built.
  Autharie (same owner) runs an MCP gateway that enforces FerrisKey's
  decisions (apps/mcp-gateway, its ADR 0008); this lot builds FerrisTrace's
  own on the same AuthZEN mapping so one realm's policies read the same
  for both products.

  Decided with the owner (2026-11-02):
    - The gateway is a new app in this repository, apps/mcp-gateway.
    - The MCP server is a separate binary, apps/mcp, calling FerrisTrace's
      HTTP API with the exchanged token.
    - Version one reads, and on incidents comments, resolves and
      acknowledges a page.
    - Time windows are read in the organisation's time zone.
    - Rules are the customer's: "Dev reads checkout's logs on weekdays
      9-18" is one example among the rules the attributes below allow,
      not a feature of its own.

  One pull request per part:
    7A  FerrisTrace knows its clock and who an agent acts for
    7B  The MCP server
    7C  The gateway
    7D  The stack, the realm and the policy guide

Glossary
  PEP          the gateway: it lets a call through or refuses it.
  PDP          FerrisKey's AuthZEN endpoint: it decides.
  Delegated    a call an MCP client makes for a signed-in person; the
               token carries the person's `sub`, groups and roles, and
               after exchange an `act` naming the gateway.
  Tool         one MCP tool of apps/mcp, e.g. `search_logs`.
  Canonical    `https://<host>/orgs/{org}/mcp`, the resource an MCP client
  URI          asks a token for (RFC 8707) and the `aud` the gateway checks.
  PIP          the gateway's attribute lookup: facts a policy needs that
               neither the token nor the call carries (the caller's
               on-call state, an incident's severity and service, a
               service's project and environment), read from FerrisTrace
               before the PDP is asked.
  Vocabulary   the documented set of entity types, attributes and context
               keys a policy can test; it is the contract of this lot and
               only grows (an attribute once published is never renamed
               or removed without a new major version of the schema).

Invariants
  - Default deny: a call with no permit, a PDP that does not answer in
    time, or an answer the gateway cannot read is refused.  [test]
  - `tools/list` never shows a tool the same subject would be refused at
    that moment; it is filtered with one batch evaluation.  [test]
  - Nothing reaches apps/mcp without a valid token for the canonical URI;
    the token apps/mcp receives is the exchanged one, never the client's.
    [test]
  - FerrisTrace still enforces the organisation, the project and the
    agent's scopes on every call: the gateway narrows, it never widens.
    [test]
  - Every decision is recorded once: subject, groups, `act`, organisation,
    tool, the arguments the policy saw, permit or deny, the policies that
    decided, and the reason when denied.  [test]
  - A delegated call lands in FerrisTrace's audit log with the person it
    was made for, not only the gateway's client.  [test]
  - A tool's answer is bounded (rows and bytes) and says when it was cut.
    [test]
  - The time a policy tests is the organisation's local weekday and hour,
    computed by the gateway; the UTC instant is sent beside it.  [test]
  - Every attribute of the vocabulary is present on every request it
    applies to, with a documented type. An attribute that does not apply
    to a call (no service named, no incident) is left out, since Cedar
    has no null, and a policy tests it with `has`.  [test]
  - A PIP lookup that fails or times out refuses the call (reason
    `pip_unavailable`): an attribute that applies but could not be read
    is never left out, because a forbid rule testing it ("nobody
    resolves a sev1") would then not match and the call would pass.
    [test]

Out of scope
  - MCP servers other than FerrisTrace's behind this gateway.
  - Relation-based rules (ReBAC) until FerrisKey evaluates tuples; the
    request already carries the team (groups) such rules will need.
  - A policy builder UI in FerrisTrace; policies are Cedar in FerrisKey,
    with a guide and a gallery of examples here.
  - Shipping rules: beyond the seeded "members may read", every rule is
    the customer's.
  - MCP resources and prompts; tools only.
  - Silencing alerts, opening incidents, changing severity.

Design
  7A FerrisTrace knows its clock and who an agent acts for
    Organisations get a time zone: `orgs.timezone` (IANA name, default
    `UTC`, validated with chrono-tz), shown and edited in Settings >
    Organisation by admins, audited. `GET /api/orgs/{org}/clock` answers
    `{"timezone"}` to sessions, read keys and agent tokens.
    Delegation: the agent verifier reads `sub` and `act` from the token;
    when `act` is present the call is delegated, `AgentCall` carries the
    subject, and every audit entry an agent writes names it
    (`on_behalf_of`). The registry keeps keying agents by client id: the
    gateway's client is the registered agent of an organisation that
    turns MCP on.
    A fourth agent scope `oncall:acknowledge` lets an agent acknowledge a
    page of its organisation (`POST .../pages/{id}/acknowledge`); every
    other on-call write stays refused to agents.
    PIP routes, readable by sessions, read keys and agents:
    `GET .../oncall/now?email=` answers whether that person is on call
    now and on which schedules; `GET .../services/{name}/placement`
    answers the project and the environments the service was last seen
    in (from spans' `deployment.environment.name`).
    Settings > MCP (admins): turn MCP on for the organisation, which
    registers the gateway's client as an agent with the chosen scopes,
    shows the URL to give an MCP client, and turns off by revoking it.
  7B The MCP server (apps/mcp, binary `ferristrace-mcp`)
    rmcp, streamable HTTP, stateless, mounted at `/orgs/{org}/mcp`. Each
    tool is one or two calls to `FERRISTRACE_API_URL` with the bearer it
    received; it holds no credential of its own and no database.
    Tools, with MCP annotations (`readOnlyHint`, `destructiveHint`) and
    the arguments a policy may test as plain strings:
      list_services        window
      service_health       service, window
      search_traces        service, errors_only, min_duration_ms, window, limit
      get_trace            trace_id
      search_logs          service, severity, text, namespace, pod, window, limit
      query_metric         name, aggregation, group_by, service, window
      cluster_status       cluster, namespace
      list_alerts          state
      alert_history        alert_id, window
      list_incidents       status
      get_incident         incident_id
      who_is_on_call       schedule
      comment_incident     incident_id, text                (write)
      resolve_incident     incident_id                      (write, destructive)
      acknowledge_page     page_id                          (write)
    Answers are compact text tables or JSON capped at 200 rows and 64 KiB,
    with a `truncated` note; errors from the API come back as tool errors
    with the API's message, never as transport errors.
  7C The gateway (apps/mcp-gateway, binary `ferristrace-mcp-gateway`)
    Stateless axum service in front of apps/mcp:
      - `/.well-known/oauth-protected-resource/orgs/{org}/mcp` (RFC 9728)
        naming FerrisKey as authorization server; a 401 carries
        `WWW-Authenticate: Bearer resource_metadata=...`.
      - Verifies the bearer with the issuer's JWKS (RS256/ES256, `iss`,
        `exp`, `aud` = canonical URI), reusing the lot 5 verifier rules.
      - Parses the JSON-RPC body; `initialize`, `ping`, notifications pass;
        `tools/list` is forwarded then filtered by one AuthZEN batch;
        `tools/call` is evaluated before forwarding; anything else is
        refused.
      - Builds each AuthZEN request from the vocabulary below (the same
        shape as Autharie's, so one realm reads alike for both), the PIP
        filling what the token and the call do not carry. Lookups are
        cached per organisation for 60 s (on-call state 30 s), bounded
        by the PDP timeout, and done once per batch for `tools/list`.
      - Exchanges the caller's token (RFC 8693, `audience` = FerrisTrace's
        agent audience) and forwards with the exchanged one.
      - PDP timeout 1.5 s by default; refusal answers a JSON-RPC error the
        client can show, with the reason, never the policies' text.
      - Decisions are structured `tracing` events (`kind=mcp_decision`);
        with an ingest key configured they are also exported over OTLP
        logs to FerrisTrace itself, so they are searchable in Logs.
  7D The stack, the realm and the policy guide
    Compose: services `mcp` and `mcp-gateway`; the gateway is the only
    public MCP door. `ferriskey-init` additionally: enables authorization
    on the realm, creates the gateway's confidential client with a
    token-exchange delegation policy toward FerrisTrace's audience, a PEP
    service account with `evaluate_authorization`, adds the canonical URI
    to the realm's allowed resources, enables MCP client registration,
    extends the realm's Cedar schema with the FerrisTrace tool attributes,
    and seeds one policy: members of the realm may call read-only tools.
    Chart: optional `mcp.enabled` deploying both binaries.
    docs/mcp.md: connect Claude Code and Claude Desktop; the Cedar schema;
    worked policies, including "group Dev may call search_logs for service
    checkout on weekdays 9-18 in the organisation's time zone", "only the
    on-call group may resolve", "nobody resolves at night"; how to read
    the decision log.

  The vocabulary (what a policy can test)
    Subject
      User (delegated call): id = `sub`; properties `email`, `groups`
      (set), `roles` (set), from the token; `on_call` (bool: on call now
      in this organisation, any schedule) and `on_call_schedules` (set of
      schedule names), from the PIP.
      Client (autonomous agent): id = client id.
    Action
      `call_tool`; the tool's nature is on the resource, so a rule can
      say "any read" without listing tools.
    Resource: Tool
      id `ferristrace/<tool>`; `tool` (name), `organisation` (id),
      `organisation_name`, `access` (`read` | `write`), `destructive`
      (bool), from the tool's annotations.
    Context
      `agent` (client id), `delegated` (bool), `act` (when present).
      `target`: what the call is about, each key present only when the
      call names it or the PIP resolves it —
        `service`, `namespace`, `cluster`, `pod`, `environment`,
        `project` (name), `severity` (sev1..sev4, the incident's),
        `incident_status`, `alert_state`, `log_severity`.
      `arguments`: the primitive arguments as sent.
      `time`: `weekday` 1..7 and `hour` 0..23 and `minute` 0..59 local
      to the organisation, `timezone`, `iso_utc`.
    Examples the guide works through, each a few lines of Cedar and each
    an acceptance test against FerrisKey's real PDP:
      - group Dev may read logs and traces of service checkout only, on
        weekdays 9:00-18:00 local time;
      - only the person on call may resolve or acknowledge;
      - nobody resolves a sev1 through MCP, only comments on it;
      - group Support may read anything of environment staging, and
        nothing of production;
      - interns read nothing of namespace payments;
      - any write is forbidden between 22:00 and 07:00 unless on call.
    PIP sources in FerrisTrace (read with the gateway's own agent
    token, scope `telemetry:read`):
      on-call state       `GET /api/orgs/{org}/oncall/now?email=` (7A)
      incident target     `GET /api/orgs/{org}/incidents/{id}`
      service target      `GET /api/orgs/{org}/services/{name}/placement`
                          (7A: project and environment last seen)
      time zone           `GET /api/orgs/{org}/clock` (7A)

Acceptance
  @spec-l7-1  An MCP client signed in through FerrisKey lists only the
              tools its policies permit, and a call to a tool it may not
              use is refused with a reason, recorded as a decision.
  @spec-l7-2  Each worked example of the guide, loaded as a policy into
              FerrisKey's real PDP, permits and refuses exactly the calls
              it describes: e.g. a Dev member gets checkout's logs on
              Tuesday 10:00 local and is refused at 20:00, on Saturday,
              and for payments; a resolve of a sev1 is refused to the
              person on call while a comment is permitted.
  @spec-l7-7  A policy written by a customer that tests an attribute the
              vocabulary does not have is reported by FerrisKey's
              validation, not silently never matched; the guide's schema
              file is what FerrisKey validates against.
  @spec-l7-3  With FerrisKey's PDP down or slow, every call is refused and
              FerrisTrace is never reached.
  @spec-l7-4  A permitted resolve_incident resolves the incident; its
              timeline and the audit log name the person it was done for
              and the gateway as agent.
  @spec-l7-5  A token minted for another resource, an expired token, or
              no token, is refused with 401 and the resource metadata
              link, and apps/mcp is never called.
  @spec-l7-6  Even with a permit-all policy, an organisation that has not
              turned MCP on, or a project the agent is not bound to,
              answers nothing: FerrisTrace refuses.

Simulation
  Yes for the decision path: the gateway's time context and its PDP
  client take a clock and an HTTP port, so weekday, hour, time zone
  (DST included) and PDP timeouts are tested with a fixed clock and a
  fake PDP. End to end, a test stack runs FerrisKey's real PDP with the
  seeded and the worked policies.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  cd web && pnpm typecheck && pnpm build
  7D: docker compose up, connect an MCP client, run @spec-l7-1..4
