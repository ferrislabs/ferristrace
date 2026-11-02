# Lot 5: AI agents

Goal
  Let an AI agent sign in as itself through FerrisKey, read telemetry and
  comment on incidents within scopes an admin ticked, with every read and
  write audited and one-click revocation.

Context
  Milestone "5 · AI agents": #188, #189, #190. People sign in over OIDC
  (crates/infra/src/oidc.rs, code flow, id token only); scripts use API
  keys with `ingest` and `read` scopes (lot 0). Read routes take a
  `Reader` (session or key) and a `ReadGrant` (org, optional project).
  An API key cannot comment on an incident (`IncidentError::ReadOnly`).
  Incident authors already include `agent`; the audit `Actor` has no
  agent. Nothing verifies an access token. FerrisKey issues access tokens
  by client credentials and token exchange, with `scope`, `azp` and
  `client_id` claims, signed with keys published at
  `/realms/{realm}/protocol/openid-connect/certs`.

  One pull request per part:
    5A  Agent identity, scopes and reads      #188 #189 #190 (reads)
    5B  Agents on incidents                   #190

Glossary
  Agent        a FerrisKey client registered in one FerrisTrace
               organisation: name, client id, scopes, optional project.
  Agent token  a JWT access token FerrisKey issued to the agent's client.
  Agent scope  `telemetry:read`, `incidents:comment`,
               `incidents:resolve`. There is no other agent scope, so an
               admin right cannot be granted to an agent.

Invariants
  - An agent token is accepted only if its signature verifies against the
    issuer's JWKS, `iss` is the configured issuer, it has not expired,
    `aud` contains the configured audience when one is set, and its
    `azp` (else `client_id`) is an agent of the organisation in the path
    that is not revoked.  [test]
  - An agent's effective scopes are the scopes in the token's `scope`
    claim that the agent was granted in FerrisTrace; anything else is
    refused with 403.  [test]
  - An agent bound to a project reads only that project, like a read key.
    [test]
  - A revoked agent is refused on its next request, whatever its token's
    expiry.  [test]
  - Every request an agent makes, read or write, writes one audit entry
    with the agent and the token id (`jti`) that allowed it.  [test]
  - An agent never reaches session-only or admin routes.  [test]

Out of scope
  - Registering the client in FerrisKey from FerrisTrace: an admin
    creates the client in FerrisKey and pastes its client id.
  - Agents ingesting telemetry (they use an ingest key like any script).
  - Running agents inside FerrisTrace, LLM summaries written by
    FerrisTrace itself.

Design
  5A Identity, scopes and reads
    Config: `FERRISTRACE_AGENTS__ISSUER` (defaults to the OIDC issuer),
    `FERRISTRACE_AGENTS__AUDIENCE` (optional). Without an issuer, agent
    routes answer 401 and the Agents settings say agents are off.
    Infra: a JWKS verifier (`jsonwebtoken`, keys fetched from the
    issuer's discovery `jwks_uri` with reqwest, cached 10 minutes,
    refetched on an unknown `kid` at most once a minute; RS256 and ES256
    only) behind a domain port `AgentTokens::verify(token) ->
    VerifiedToken {client_id, scopes, jti, expires_at}`.
    Domain `agents` context: `Agent` (id, org, name, client id unique per
    org, scopes, project, created_by, created_at, last_used_at,
    revoked_at); register, list, revoke (admins, audited); resolving a
    token for an org into an `AgentGrant` (agent id, effective scopes,
    project) and touching `last_used_at`.
    Access: `Reader` gains `Agent(AgentToken)`; a bearer that is a JWT
    (three dot-separated parts) is an agent token, otherwise an API key.
    `Readers::grant` accepts an agent with `telemetry:read`, narrowed to
    its project, and records the audit entry `agent.read` with the route
    template and the `jti`. Audit `Actor::Agent(AgentId)` with the jti in
    the entry detail; Postgres check constraints widened.
    Reads open to agents: traces, services, operations, logs, metrics,
    cluster, releases, deploys, LLM calls and spend, issues, incidents
    and their timelines (everything `reading()` serves today).
    HTTP: `GET/POST /api/orgs/{org}/agents`, `DELETE
    /api/orgs/{org}/agents/{id}` (revoke) for admins; `GET
    /api/orgs/{org}/agents/me` for an agent to see its own grant.
    Web: Settings > Agents (register with name, client id, scopes,
    project; list with last use; revoke), the audit page shows agent
    actors; deploy/README.md explains the FerrisKey client (client
    credentials, scopes, audience mapper) and the config.
  5B Agents on incidents
    `POST .../incidents/{id}/comments` accepts an agent with
    `incidents:comment`; the entry's author is the agent (kind `agent`,
    agent id and name); changing status to `resolved` accepts an agent
    with `incidents:resolve`; other incident writes stay refused to
    agents. Each is audited with the jti. The web timeline shows agent
    entries with an "agent" badge and the agent's name.

Acceptance
  @spec-l5-1  An agent registered with `telemetry:read` reads the traces
              of its organisation with a FerrisKey client credentials
              token, and the read is in the audit log with its jti.
  @spec-l5-2  The same token is refused by another organisation, by a
              route outside its scopes, and after the agent is revoked.
  @spec-l5-3  A token whose `scope` lacks `telemetry:read` is refused
              even when the agent was granted it, and an expired or
              wrongly signed token is refused.
  @spec-l5-4  An agent with `incidents:comment` posts a comment that the
              timeline shows as written by that agent; resolving needs
              `incidents:resolve`.

Simulation
  No: token verification is tested with keys generated in the tests and
  a JWKS served by a local test server.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck && pnpm build
Amended: 2026-11-02 5A as built: agents are on only when an issuer and an audience are both set, otherwise sign-in keeps working, agent tokens get 401 and Settings says why; the OIDC sign-in client cannot be registered as an agent; the verifier also requires `jti` and `azp` or `client_id`, refuses a `typ` other than Bearer and checks `nbf`; the JWKS URI must be on the issuer host and documents are capped at 1 MiB; a failed refetch keeps the cached keys; an unknown, revoked or other-organisation client gets 401, a registered agent out of scope 403; agents are listed by admins only; the audit entry names the matched route.
Amended: 2026-11-02 5B as built: an agent comment is never public (403 when asked), since publishing reaches status page subscribers; a project-bound agent gets 404 on incidents of another project or none; a resolve PATCH carrying any other field, or any other status, is refused 403; resolving an already resolved incident changes nothing and is still audited; the write and its audit row share the request's unit of work; old agent timeline rows without an agent id still read.
