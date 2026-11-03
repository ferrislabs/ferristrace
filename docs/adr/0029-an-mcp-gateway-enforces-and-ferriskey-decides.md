# 0029. An MCP gateway enforces and FerrisKey decides

Status: accepted
Date: 2026-11-03

## Context
Lot 7 lets a person on call ask an MCP client to read FerrisTrace and act on
incidents. Which tools that person may use, on which service, in which
environment, at which hour, is the customer's decision, written as Cedar
policies in their FerrisKey realm. FerrisKey already evaluates those policies
behind AuthZEN 1.0 (`/realms/{realm}/authzen/access/v1/evaluation[s]`), issues
RFC 8693 token exchanges with an `act` claim, and publishes the metadata MCP
clients discover (RFC 8414, RFC 8707, RFC 9728 through the resource).

FerrisTrace's API already enforces the organisation, the project and an
agent's scopes on every call (lot 5). What it cannot see is the question a
policy asks: who is calling for whom, whether they are on call, which tool,
which incident's severity, in the organisation's local time.

Autharie, from the same owner, solved the same problem with a stateless MCP
gateway (`apps/mcp-gateway`, its ADR 0008). Its mapping of a tool call to an
AuthZEN request is the one a realm already reads for Autharie; keeping it
means one realm's policies read the same for both products.

## Decision
`apps/mcp-gateway` (package and binary `ferristrace-mcp-gateway`) is the only
public MCP door. It enforces; FerrisKey decides; the gateway holds no policy.

- It is an OAuth resource server for `https://<host>/orgs/{org}/mcp`. It
  serves RFC 9728 metadata naming FerrisKey as authorization server. It
  verifies the bearer against the issuer's JWKS (RS256 and ES256 only, `iss`,
  `exp`, `nbf`, and `aud` equal to that canonical URI). Anything else is a 401
  carrying `WWW-Authenticate: Bearer resource_metadata=...`, and nothing
  downstream is called.
- `initialize`, `server/discover`, `ping` and `notifications/*` pass. `tools/list`
  is forwarded and its answer filtered by one AuthZEN batch (`execute_all`).
  `tools/call` is evaluated before it is forwarded. Every other method is
  refused. GET and DELETE are 405: the server is stateless.
- The request follows the vocabulary of `docs/specs/lot-7-mcp.md`: subject
  `User` (delegated) or `Client`, action `call_tool`, resource `Tool`, and a
  context of agent, target, arguments and the organisation's local time.
  A tool's `access` and `destructive` come from the annotations in the
  `tools/list` answer of apps/mcp, cached ten minutes; a tool without a
  `readOnlyHint` is a destructive write.
- Default deny. A PDP that times out (1.5 s), is unavailable or answers
  something the gateway cannot read refuses the call with `pdp_timeout`,
  `pdp_unavailable` or `pdp_malformed`. A tool the server does not have is
  refused `unknown_tool`.
- The facts a policy needs and neither the token nor the call carries come
  from a PIP: FerrisTrace, read with the gateway's own client-credentials
  token for FerrisTrace's agent audience (the organisation's time zone, the
  caller's on-call state, an incident's severity and services, a service's
  project and environment). A lookup that applies and fails, times out, is
  refused or answers unreadably refuses the call with `pip_unavailable`. An
  attribute that applies is never left out. Only a definite 404 (an incident
  or a service FerrisTrace does not know) leaves its attributes out, since
  there is then nothing to resolve and FerrisTrace answers the same 404.
- The caller's token is exchanged (RFC 8693, audience FerrisTrace's agent
  audience, the gateway as actor, so the exchanged token carries `act`) and
  only the exchanged token is forwarded. Hop-by-hop headers and the caller's
  credentials are dropped.
- A refusal is a JSON-RPC error with the reason code, never policy text. HTTP
  status 200: a 403 reads as an OAuth step-up to several MCP clients.
- Every decision is one `tracing` event (`kind=mcp_decision`) and, with an
  ingest key configured, an OTLP log record sent to FerrisTrace itself through
  a bounded queue. A full queue drops and counts; a decision never waits for
  the export.
- Time is injected: the gateway takes a clock and computes the local weekday,
  hour and minute from the organisation's IANA zone with jiff, DST included.

## Provenance
The structure (verifier, JSON-RPC parsing and `Mcp-Method`/`Mcp-Name`
agreement, AuthZEN builders and PDP client, RFC 8693 exchange with a
client-credentials actor token and a token cache, RFC 9728 metadata and
challenge, decision record) is adapted from Autharie's `apps/mcp-gateway`,
same owner. The JWKS verifier is copied from `crates/infra/src/jwks.rs`
rather than reused: depending on `ferristrace-infra` would pull lettre,
openidconnect and the whole adapter layer into a leaf binary, and that
verifier returns an agent identity (`VerifiedToken`) without the `groups`,
`roles`, `email` and `sid` a policy needs. The rules are kept: algorithms,
`kid` handling, single-flight refetch, stale keys kept, a one MiB cap, the
`jwks_uri` host equal to the issuer's. Autharie's legacy-protocol bridge is
not needed: apps/mcp (rmcp 3.8) speaks the 2026-07-28 and the legacy
streamable HTTP handshakes itself, and the gateway passes both.

## Alternatives rejected
- Policies inside apps/mcp or the gateway: a second source of truth beside the
  IAM the customer already runs, and a customer's rules in our code.
- Forwarding the caller's token: it erases which agent acted, and an
  audience-bound token is not for FerrisTrace.
- Treating a failed PIP lookup as an absent attribute: a forbid rule testing
  "nobody resolves a sev1" would stop matching exactly when the severity could
  not be read, and the call would pass.
- A tool result with `isError` for a refusal: friendlier to a model, but the
  spec asks for a JSON-RPC error carrying the reason, and one function
  (`rpc::refusal`) changes it.

## Consequences
- Gains: one door, one decision log, rules the customer owns and reads the
  same in both products, and a gateway that narrows and never widens:
  FerrisTrace still checks the organisation, project and scopes.
- Costs: a PDP round trip per call and per list, up to four PIP lookups per
  call (cached 5 minutes for the zone, 30 s for on-call, 60 s for incident
  and service facts; a write never reads the incident cache), and a token
  exchange per distinct caller token. A PDP or a FerrisTrace that is down
  denies everything.
- A cold gateway fetches `tools/list` from apps/mcp, with the exchanged token,
  to learn a tool's nature before it can ask the PDP; that request runs no
  tool and reaches no FerrisTrace route.
- `tools/list` carries no arguments, so a rule that needs `target.service`
  hides the tool from the list; a rule meant to show it must allow the call
  without a target (`!(context.target has service)`) and forbid at call time.
- The target also carries `services` (every service of an incident) and
  `environments` (every environment a service was seen in), because
  `service` and `environment` are only set when there is exactly one. A
  forbid on a single value should test the set.
- The gateway buffers a `tools/list` answer (at most one MiB) and, for other
  methods, streams the server's answer back.
