# MCP behind a policy gateway

`ferristrace-mcp` lets an MCP client (Claude Code, Claude Desktop, any MCP
host) read FerrisTrace and act on incidents. `ferristrace-mcp-gateway` stands
in front of it and is the only public door: it verifies who is calling, asks
FerrisKey whether the call is permitted, and only then forwards it with a token
made for FerrisTrace. The rules are yours, written as Cedar policies in your
FerrisKey realm; FerrisTrace ships none beyond one default (members may call
read-only tools). FerrisTrace itself still checks the organisation, the project
and the agent's scopes on every call, so the gateway can only narrow.

```
MCP client --(token for https://host/orgs/{org}/mcp)--> gateway
gateway --(AuthZEN)--> FerrisKey (Cedar policies)
gateway --(exchanged token, act = gateway)--> ferristrace-mcp --> FerrisTrace API
```

## Quick start with the compose stack

`deploy/docker-compose.yml` already runs `mcp` and `mcp-gateway`, and
`ferriskey-init` provisions the realm for them (see `deploy/README.md`). The
gateway answers on `http://localhost:8080` (`MCP_GATEWAY_PORT`,
`MCP_GATEWAY_BIND`).

1. Put these in `deploy/.env`. `FERRISTRACE_MCP_PUBLIC_URL` is the address
   your MCP clients use; the canonical resource of an organisation is
   `<public url>/orgs/<organisation id>/mcp`, and a FerrisKey token for it is
   what the gateway accepts. `FERRISTRACE_MCP_ORGS` lists the organisation ids
   (comma separated) whose resource the realm should allow, because FerrisKey
   only puts a resource in a token's audience when the realm was told about
   it, one exact address at a time.

   ```sh
   FERRISTRACE_MCP_PUBLIC_URL=http://localhost:8080
   FERRISTRACE_MCP_ORGS=11111111-1111-4111-8111-111111111111
   ```

2. `docker compose up -d`, or after adding an organisation to the list,
   `docker compose run --rm ferriskey-init` (it adds and never removes).
3. In FerrisTrace, Settings > Organisation > MCP: turn MCP on for the organisation. That
   registers the gateway's client as an agent of the organisation with the
   scopes you choose and shows the address to give a client.
4. In FerrisKey, put people in organisation groups (Dev, Support, ...): the
   group names reach your policies. The browser and the gateway reach FerrisKey
   under the name `ferriskey` (see "Before the first sign-in" in `deploy/README.md`).

What `ferriskey-init` sets up in the realm, all idempotent: authorization
turned on; the clients `ferristrace-mcp-gateway` (token exchange, delegation
only, toward the `ferristrace-agents` audience) and `ferristrace-mcp-pep` (a
service account whose role holds `evaluate_authorization`); the client scopes
`telemetry:read`, `incidents:comment`, `incidents:resolve`,
`oncall:acknowledge` and `groups` (open to dynamically registered clients),
plus an audience mapper that puts `ferristrace-agents` in the gateway's own
token only; dynamic client registration and metadata-document registration
turned on; the allowed resources and their owner; on a FerrisKey that carries
the vocabulary (below), the `FerrisTrace` schema; and the policy
`members-read-tools`. Its secrets land in `deploy/ferriskey/mcp-gateway.env`.

## Connect a client

Claude Code:

```sh
claude mcp add --transport http ferristrace http://localhost:8080/orgs/<organisation id>/mcp
```

then run `/mcp` inside Claude Code and choose to authenticate: the client finds
the authorization server in the gateway's resource metadata, registers itself
with FerrisKey and sends you to sign in. Claude Desktop: add the same address
as a custom connector (Settings, Connectors), or, for a build that only reads
`claude_desktop_config.json`, bridge it with `npx mcp-remote <address>`. Any
client that speaks streamable HTTP and OAuth 2.1 with resource indicators works
the same way.

A call that is refused comes back as a JSON-RPC error with a reason (table in
"The policy gateway" below). `tools/list` shows only the tools the person
would be allowed to call at that moment.

## What a policy can test

The gateway asks FerrisKey (AuthZEN 1.0, action `call_tool`) once per tool call
and once, as a batch, per tool list. The request it builds is the vocabulary.
Every attribute that applies is present; one that does not apply (no service
named, no incident) is left out, since Cedar has no null, so a policy tests it
with `has`.

| What | Attribute | Type | Meaning |
| --- | --- | --- | --- |
| Subject `User` (a call made for a person) | id | String | The token's `sub` |
| | `email` | String | From the token |
| | `groups` | Set of String | From the token's `groups` claim (the `groups` scope) |
| | `roles` | Set of String | The token's `roles`, or `realm_access.roles` |
| | `on_call` | Bool | On call now in this organisation, any schedule |
| | `on_call_schedules` | Set of String | The schedules the person is on call for |
| Subject `Client` (an autonomous agent) | id | String | The client id |
| Action | `call_tool` | | The nature of the tool is on the resource |
| Resource `Tool` | id | String | `ferristrace/<tool>` |
| | `tool` | String | Tool name |
| | `organisation` | String | Organisation id |
| | `access` | String | `read` or `write`, from the tool's annotations |
| | `destructive` | Bool | From the tool's annotations |
| Context | `agent` | String | The client id that called |
| | `delegated` | Bool | Made for a person |
| | `act` | Record | The token's `act` claim, when present |
| | `target.service` | String | The service the call names, or the incident's only service |
| | `target.services` | Set of String | Every service of an incident (`service` is only set when there is one) |
| | `target.namespace`, `cluster`, `pod` | String | As named by the call |
| | `target.environment` | String | The service's environment, only when it was seen in exactly one |
| | `target.environments` | Set of String | Every environment the service was seen in |
| | `target.project` | String | The service's project |
| | `target.severity` | String | `sev1` to `sev4`, the incident's |
| | `target.incident_status` | String | The incident's status, or the status asked for |
| | `target.alert_state`, `target.log_severity` | String | As asked for |
| | `arguments` | Record | The normalised arguments the tool will run (text trimmed, choices lower-cased, ids in canonical form) |
| | `time.weekday` | Long | 1 (Monday) to 7, in the organisation's time zone |
| | `time.hour`, `time.minute` | Long | 0 to 23, 0 to 59, in the same zone (DST included) |
| | `time.timezone`, `time.iso_utc` | String | The zone, and the instant in UTC |

The time zone is the organisation's (Settings > Organisation > General). The vocabulary
only grows: an attribute once published is not renamed or removed.
`docs/mcp/ferristrace.cedarschema` is the same table as a Cedar schema, with
exact types. It lives in its own namespace, `FerrisTrace`, and extends
FerrisKey's built-in agent types through the reserved types `UserExtension`,
`ToolExtension` and `ContextExtension`.

### Which FerrisKey carries the vocabulary

The whole vocabulary reaches Cedar on a FerrisKey that contains
ferriskey/ferriskey#1912: every AuthZEN property becomes a Cedar attribute, the
whole context is forwarded, and a realm schema may extend the built-in types
with the `...Extension` types. Releases up to and including 0.10.1 (the newest
when this was written) do not, and `ferriskey-init` detects which one it is
talking to, by looking for `ContextExtension` in the api description
(`/api-docs/openapi.json`):

| | A release containing #1912 | 0.10.1 and earlier |
| --- | --- | --- |
| Passed to Cedar | everything the gateway sends | `principal` in `FerrisKey::Group::"<group>"` and `FerrisKey::Role::"<role>"`, `principal.email`, `resource.tool`, `resource.server`, `context.agent`, `delegated`, `act`, `target.*`, `time.*` |
| Dropped | nothing a realm schema declares; undeclared attributes are ignored | `principal.on_call`, `on_call_schedules`, `resource.access`, `destructive`, `organisation`, `context.arguments` |
| Schema stored by init | `ferristrace.cedarschema` (only if the realm has none, or already has this one) | none: a stored schema is checked against the built-in agent context, which has no `target`, no `time.minute` and requires `time.iso`, so the gateway's request would get a 400 and everything would be refused |
| Seeded policy | `seed-members-read.cedar`: any signed-in person, tools whose `resource.access` is `read` | `seed-members-read-by-name.cedar`: the same, by tool name |
| Validation | a policy that tests an attribute the vocabulary lacks is refused when saved (422, with the position) | none, since no schema is stored |

On a FerrisKey without #1912, a rule that depends on a dropped attribute never
matches. The examples are written so that is safe: `principal.on_call` is never
true, so "only the person on call resolves" permits nobody and the night
`forbid` applies to everyone. The suite fails if any build permits what the
guide refuses, and runs against both kinds (`FERRISKEY_URL=... REQUIRE_FERRISKEY_URL=1
cargo test -p ferristrace-mcp-gateway --test real_pdp -- --nocapture`) and
asserts what each decides. Upgrade FerrisKey before you rely on a `forbid` that
tests a forwarded attribute.

A policy is also checked offline, without FerrisKey, with
`cargo test -p ferristrace-mcp-gateway --test policies`: it merges the
extension into a copy of FerrisKey's built-in schema
(`apps/mcp-gateway/tests/ferriskey-builtin.cedarschema`), validates every
policy of the guide and evaluates the cases of `policy_cases.json` with
`cedar-policy` 4.14, the version FerrisKey runs. A typo like `principal.oncall`
is reported by both, but a typo behind a `has` guard is accepted by both and
simply never matches.

With a schema stored, a declared attribute of the wrong type is a 400 to the
gateway, which refuses the call: a client that sends `limit` as a string is
refused rather than evaluated.

## Worked policies

Each is one file in `docs/mcp/policies/`, validated against the schema and
evaluated for the cases in `apps/mcp-gateway/tests/policy_cases.json`, and
decided by FerrisKey's own PDP in the suite above. Cedar denies by default, so
every example is a `permit` for what to allow or a `forbid` for what to make
final. They are written for FerrisKey's built-in agent types, extended as above.
Load them in the console (Authorization, Policies) or with
`POST /realms/<realm>/authz/policies`, one policy each. The seeded
`members-read-tools` permits every read tool (`resource.access` is `read`) to any signed-in person; an
example that must be the only permit, such as the first, needs it disabled.

### 1. Dev reads checkout's logs and traces on weekdays, 9 to 18, local time

```cedar
permit (
  principal in FerrisKey::Group::"Dev",
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  ["search_logs", "search_traces"].contains(resource.tool) &&
  context.target has service &&
  context.target.service == "checkout" &&
  context.time.weekday >= 1 &&
  context.time.weekday <= 5 &&
  context.time.hour >= 9 &&
  context.time.hour < 18
};
```

A Dev member gets checkout's logs on a Tuesday at 10:00 in the organisation's
time zone and is refused at 20:00, on a Saturday, at 18:00 sharp and for any
other service. The window is local: the gateway computes `time.weekday` and
`time.hour` from the organisation's zone, DST included.

### 2. Only the person on call resolves or acknowledges

```cedar
permit (
  principal is FerrisKey::User,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  ["resolve_incident", "acknowledge_page"].contains(resource.tool) &&
  principal has on_call &&
  principal.on_call
};
```

Anyone off call is refused. On a FerrisKey without #1912 `on_call` is not passed
on (above), so this permits nobody.

### 3. Nobody resolves a sev1; comments stay possible

```cedar
forbid (
  principal,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.tool == "resolve_incident"
}
unless {
  context.target has severity &&
  context.target.severity != "sev1"
};
```

```cedar
permit (
  principal is FerrisKey::User,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  resource.tool == "comment_incident"
};
```

The `forbid` is final: even the person on call is refused. It is written as
"forbid resolving unless the severity is known and is not sev1", so an unknown
severity forbids. `target.severity` is the incident's, read by the gateway before
it asks (a lookup that fails refuses the call with `pip_unavailable`). The second
policy lets any signed-in person comment.

### 4. Support reads staging, and nothing of production

```cedar
permit (
  principal in FerrisKey::Group::"Support",
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  resource.server == "ferristrace" &&
  resource has access &&
  resource.access == "read" &&
  context.target has environment &&
  context.target.environment == "staging"
};
```

Any read tool, only for a known environment. A service without spans has no environment, and one
seen in both staging and production has no single `environment`: both are
refused, which is what you want. Do not write this as "forbid production": it
would let through whatever has no environment.

### 5. Interns read nothing of namespace payments

```cedar
forbid (
  principal in FerrisKey::Group::"Interns",
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  context.target has namespace &&
  context.target.namespace == "payments"
};
```

Pair it with whatever permits interns to read at all (the seeded policy does).
It matches the namespace a call names (`search_logs`, `cluster_status`); a call
that names only a service whose pods live in `payments` is not caught, so
combine it with a rule on `target.service` or `target.project` if that matters.

### 6. No write between 22:00 and 07:00 unless on call

```cedar
forbid (
  principal,
  action == FerrisKey::Action::"call_tool",
  resource is FerrisKey::Tool
)
when {
  (context.time.hour >= 22 || context.time.hour < 7) &&
  !(resource has access && resource.access == "read")
}
unless {
  principal has on_call &&
  principal.on_call
};
```

Everything that is not known to be a read is a write, so a new tool is covered
without touching the rule, and so is a FerrisKey that does not pass
`resource.access` on (it then also refuses reads at night, which is the safe
side). The person on call is let through by the `unless`.

## Writing policies

- Write permits on known values. A `forbid` on `target.environment` misses a
  service with no environment; a `permit` on it cannot.
- `environment` and `service` are only set when there is exactly one; to forbid
  by environment, test the set: `context.target.environments.contains("production")`
  behind a `has environments`.
- `tools/list` is evaluated with an empty target. A permit that needs
  `target.service` hides its tool from the list, and a model does not call a
  tool it was not shown. To show it, allow the call without a target with
  `!(context.target has service) || context.target.service == "checkout"`, and
  accept what that allows: a call that names no service passes that clause too.
  The first example is written the strict way and is hidden from the list.
- Write a forbid so that a missing attribute forbids, and a permit so that a
  missing attribute does not permit. Before, the night rule forbade only what it
  knew to be a write, so on a FerrisKey that does not pass `resource.access` it
  forbade nothing:

  ```cedar
  forbid (principal, action == FerrisKey::Action::"call_tool", resource is FerrisKey::Tool)
  when { resource has access && resource.access == "write" && (context.time.hour >= 22 || context.time.hour < 7) }
  unless { principal has on_call && principal.on_call };
  ```

  Now it forbids what it does not know to be a read (policy 6). Put the facts
  that may be missing in the `unless`, or negate them. The exception is a fact
  whose absence only means "the call does not name it", like example 5's
  `target.namespace`.
- Guard every optional attribute with `has`. On FerrisKey a policy that errors
  while it is evaluated makes the whole decision a deny, even if another policy
  permits, so an unguarded `context.target.service` refuses calls that name no
  service.
- A rule that tests an attribute the vocabulary does not have never matches. With
  the schema stored (see "Which FerrisKey carries the vocabulary") FerrisKey
  reports it when you save the policy (422 with the position), but not behind a
  `has` guard.
- Test a rule with `cargo test -p ferristrace-mcp-gateway --test policies`
  after adding a case to `policy_cases.json`, then against FerrisKey with
  `FERRISKEY_URL=... REQUIRE_FERRISKEY_URL=1 cargo test -p ferristrace-mcp-gateway --test real_pdp -- --nocapture`.
  The suite reads `FERRISKEY_ADMIN_USERNAME` and `FERRISKEY_ADMIN_PASSWORD`
  (default `admin`), creates the organisation `ftmcp-e2e`, four users and its
  own policies, disables yours while it runs and enables them again at the end:
  point it at a scratch stack.

## Reading the decision log

Every decision is one structured event, `kind="mcp_decision"`, once per tool of
a `tools/list` and once per `tools/call`. The fields are in the table under
"The decision log" below. In `docker compose logs mcp-gateway` the events are
the lines of target `mcp_decision`. A permit has `decision=permit` and the
names of the policies FerrisKey reports in `policies`; a refusal has
`decision=deny` and `reason`: `no_permitting_policy` means FerrisKey answered
and nothing permitted the call (read `target` and `arguments` to see what it
was asked), while `pdp_timeout`, `pdp_unavailable`, `pdp_malformed` and
`pip_unavailable` mean it could not be asked or a fact could not be read.

With an ingest key (Settings > Project > Api keys) set as
`FERRISTRACE_MCP_GATEWAY_INGEST_KEY`, and
`FERRISTRACE_MCP_GATEWAY_INGEST_ENDPOINT=http://server:4318/v1/logs`, the same
records are also exported to FerrisTrace as logs of the service
`ferristrace-mcp-gateway`: severity INFO for a permit, WARN for a refusal, body
`mcp decision: <decision> <tool>`, and the fields as attributes. In Logs,
choose that service, then WARN to see the refusals, or search `mcp decision:
deny resolve_incident` for one tool. A delegated call also lands in the
organisation's audit log with the person it was made for.

## The MCP server

```sh
FERRISTRACE_MCP_API_URL=http://localhost:3333 cargo run -p ferristrace-mcp
```

or build the image:

```sh
docker build -f Dockerfile.mcp -t ferristrace-mcp .
docker run --rm -p 8081:8081 -e FERRISTRACE_MCP_API_URL=http://ferristrace:3333 ferristrace-mcp
```

| Variable                   | Default                 | Meaning                                  |
| -------------------------- | ----------------------- | ---------------------------------------- |
| `FERRISTRACE_MCP_API_URL`  | `http://localhost:3333` | Base URL of the FerrisTrace HTTP API.    |
| `FERRISTRACE_MCP_LISTEN`   | `0.0.0.0:8081`          | Address the MCP server listens on.       |
| `RUST_LOG`                 | `info`                  | Log filter.                              |

### Endpoints

- `POST /orgs/{org}/mcp`: MCP over streamable HTTP, stateless. The organisation
  id in the path scopes every API call the tools make.
- `GET /healthz`: answers 200, no token needed.

A request to the MCP endpoint without `Authorization: Bearer <token>` is
answered 401 before any tool runs. The server is meant to sit behind the
gateway (part 7C), which hands it a token already exchanged for FerrisTrace's
agent audience.

### Tools

| Tool               | Arguments                                                              | FerrisTrace route                                                  |
| ------------------ | ---------------------------------------------------------------------- | ------------------------------------------------------------------ |
| `list_services`    | `window`                                                               | `GET /api/orgs/{org}/services`                                     |
| `service_health`   | `service`, `window`                                                    | `GET .../services`, `GET .../services/{service}/operations`        |
| `search_traces`    | `service`, `errors_only`, `min_duration_ms`, `window`, `limit`         | `GET .../traces`                                                   |
| `get_trace`        | `trace_id`                                                             | `GET .../traces/{trace_id}`                                        |
| `search_logs`      | `service`, `severity`, `text`, `namespace`, `pod`, `window`, `limit`   | `GET .../logs`                                                     |
| `query_metric`     | `name`, `aggregation`, `group_by`, `service`, `window`                 | `GET .../metrics/{name}/series`                                    |
| `cluster_status`   | `cluster`, `namespace`                                                 | `GET .../cluster`                                                  |
| `list_alerts`      | `state`                                                                | `GET .../alerts`                                                   |
| `alert_history`    | `alert_id`, `window`                                                   | `GET .../alerts/{id}/history`                                      |
| `list_incidents`   | `status`                                                               | `GET .../incidents`                                                |
| `get_incident`     | `incident_id`                                                          | `GET .../incidents/{id}`                                           |
| `who_is_on_call`   | `schedule`                                                             | `GET .../oncall/now` |
| `comment_incident` | `incident_id`, `text`                                                  | `POST .../incidents/{id}/comments`                                 |
| `resolve_incident` | `incident_id`                                                          | `PATCH .../incidents/{id}` with `{"status":"resolved"}`            |
| `acknowledge_page` | `page_id`                                                              | `POST .../pages/{id}/ack`                                          |

Reads are annotated `readOnlyHint`; `comment_incident` and `acknowledge_page`
are writes; `resolve_incident` is a destructive write.

A `window` is `15m`, `1h`, `24h` or `7d` (default `1h`).

### Answers

Answers are compact text: a header line, then a table or a bullet list. They
are capped at 200 rows and 64 KiB, and a cut answer ends with a
`[truncated: showing N of M rows; ...]` line. An error from the API (a 403
because the token lacks a scope, a 404, a 5xx) or an unreachable API is
returned as a tool error carrying the API's message, never as a transport
error.

## The policy gateway

`ferristrace-mcp-gateway` (`apps/mcp-gateway`) is the only public MCP door.
It verifies the caller's token, asks FerrisKey (AuthZEN) whether the call is
permitted, exchanges the token for one addressed to FerrisTrace and forwards
to `ferristrace-mcp`. See ADR 0029.

```sh
docker build -f Dockerfile.mcp-gateway -t ferristrace-mcp-gateway .
```

Settings are read from the environment, prefix `FERRISTRACE_MCP_GATEWAY_`,
nested settings separated by `__` (for example
`FERRISTRACE_MCP_GATEWAY_PDP__CLIENT_SECRET`). The gateway refuses to start
when a required one is missing or not a URL. Secrets are never logged.

| Variable                                          | Default                  | Meaning                                                                                                   |
| ------------------------------------------------- | ------------------------ | --------------------------------------------------------------------------------------------------------- |
| `FERRISTRACE_MCP_GATEWAY_LISTEN`                  | `0.0.0.0:8080`           | Address the gateway listens on.                                                                           |
| `FERRISTRACE_MCP_GATEWAY_PUBLIC_URL`              | required                 | Public base URL. The canonical resource is `<public_url>/orgs/{org}/mcp`, the `aud` a token must carry.   |
| `FERRISTRACE_MCP_GATEWAY_UPSTREAM`                | `http://localhost:8081`  | Base URL of `ferristrace-mcp`.                                                                            |
| `FERRISTRACE_MCP_GATEWAY_ISSUER`                  | required                 | FerrisKey realm issuer, e.g. `https://id.example.com/realms/ferristrace`.                                 |
| `FERRISTRACE_MCP_GATEWAY_API_URL`                 | `http://localhost:3333`  | FerrisTrace HTTP API, read for the PIP lookups.                                                           |
| `FERRISTRACE_MCP_GATEWAY_AGENT_AUDIENCE`          | required                 | FerrisTrace's agent audience (`FERRISTRACE_AGENTS__AUDIENCE`): the exchange and PIP tokens are for it.    |
| `FERRISTRACE_MCP_GATEWAY_SCOPES_SUPPORTED`        | the four agent scopes    | Scopes listed in the resource metadata and the challenge.                                                 |
| `FERRISTRACE_MCP_GATEWAY_UPSTREAM_TIMEOUT_MS`     | `30000`                  | Total time allowed to `ferristrace-mcp`.                                                                  |
| `FERRISTRACE_MCP_GATEWAY_BODY_TIMEOUT_MS`         | `10000`                  | Time allowed to read a request body.                                                                      |
| `FERRISTRACE_MCP_GATEWAY_PDP__URL`                | required                 | FerrisKey base URL; AuthZEN is `<url>/realms/<realm>/authzen/access/v1/evaluation[s]`.                    |
| `FERRISTRACE_MCP_GATEWAY_PDP__REALM`              | required                 | Realm holding the Cedar policies.                                                                         |
| `FERRISTRACE_MCP_GATEWAY_PDP__CLIENT_ID`          | required                 | PEP service account (holds `evaluate_authorization`).                                                     |
| `FERRISTRACE_MCP_GATEWAY_PDP__CLIENT_SECRET`      | required                 | Its secret.                                                                                               |
| `FERRISTRACE_MCP_GATEWAY_PDP__TOKEN_URL`          | `<issuer>/protocol/openid-connect/token` | Token endpoint for the PEP service account.                                               |
| `FERRISTRACE_MCP_GATEWAY_PDP__TIMEOUT_MS`         | `1500`                   | Budget for one PDP evaluation, and for the PIP lookups of one call. Over it, the call is refused.         |
| `FERRISTRACE_MCP_GATEWAY_EXCHANGE__CLIENT_ID`     | required                 | The gateway's own confidential client: actor of the exchange, and the agent that reads FerrisTrace.       |
| `FERRISTRACE_MCP_GATEWAY_EXCHANGE__CLIENT_SECRET` | required                 | Its secret.                                                                                               |
| `FERRISTRACE_MCP_GATEWAY_EXCHANGE__TOKEN_URL`     | `<issuer>/protocol/openid-connect/token` | RFC 8693 token endpoint.                                                                  |
| `FERRISTRACE_MCP_GATEWAY_EXCHANGE__TIMEOUT_MS`    | `3000`                   | Time allowed to one token request.                                                                        |
| `FERRISTRACE_MCP_GATEWAY_INGEST_KEY`              | unset                    | FerrisTrace ingest key. With `INGEST_ENDPOINT`, decisions are also exported as OTLP logs.                 |
| `FERRISTRACE_MCP_GATEWAY_INGEST_ENDPOINT`         | unset                    | OTLP/HTTP logs URL, e.g. `http://ferristrace:4318/v1/logs`.                                               |
| `RUST_LOG`                                        | `info`                   | Log filter.                                                                                               |

### Endpoints

- `GET /.well-known/oauth-protected-resource/orgs/{org}/mcp`: RFC 9728
  metadata (`resource`, `authorization_servers` = the issuer, `scopes_supported`).
- `POST /orgs/{org}/mcp`: MCP. A missing, expired, mis-signed or wrongly
  addressed token is `401` with
  `WWW-Authenticate: Bearer resource_metadata="..."`, and nothing is called.
  `GET` and `DELETE` are `405`: the server is stateless.
- `GET /healthz`.

### What is refused, and why

A refused call is a JSON-RPC error with code `-32001`, HTTP 200, and
`error.data.reason`. It never carries a policy's text.

| `reason`               | Meaning                                                                                  |
| ---------------------- | ---------------------------------------------------------------------------------------- |
| `no_permitting_policy` | The PDP answered, and no policy permits the call.                                        |
| `pdp_timeout`          | The PDP did not answer within the budget.                                                |
| `pdp_unavailable`      | The PDP, or its token endpoint, is down or refused the gateway.                          |
| `pdp_malformed`        | The PDP's answer could not be read.                                                      |
| `pip_unavailable`      | A fact the call needs (time zone, on-call state, incident, service) could not be read.   |
| `unknown_tool`         | `ferristrace-mcp` has no such tool.                                                      |
| `invalid_target`       | A name in the call cannot be looked up safely, an id is missing or not a UUID, or a choice is not one the tool accepts. |
| `unknown_target`       | The incident or service the call names is not known to FerrisTrace.                      |
| `invalid_request`      | Not a valid MCP request (parse error, bad params, body too large or too slow).           |
| `upstream_unavailable` | `ferristrace-mcp` did not answer usably (HTTP 502).                                      |
| `exchange_failed`      | The token for FerrisTrace could not be obtained (HTTP 502).                              |
| `method_not_allowed`   | MCP method other than initialize, server/discover, ping, notifications, tools/list/call. |
| `header_mismatch`      | `Mcp-Method` or `Mcp-Name` disagrees with the body (HTTP 400).                           |

### The decision log

Every decision is one `tracing` event with `kind="mcp_decision"` (target
`mcp_decision`), once per tool for a `tools/list` and once per `tools/call`:

| Field         | Content                                                                          |
| ------------- | -------------------------------------------------------------------------------- |
| `request_id`  | Generated for every request; sent back as `X-Request-Id` and to `ferristrace-mcp`. |
| `client_request_id` | The caller's own `X-Request-Id`, when it sent a printable one.            |
| `method`      | `tools/call` or `tools/list`, or the refused method.                             |
| `org`         | Organisation id.                                                                 |
| `tool`        | Tool name (empty for a refusal that is not about one tool).                      |
| `subject_type`, `subject` | `User` and the token's `sub`, or `Client` and the client id.         |
| `groups`      | The caller's groups, as a JSON list.                                             |
| `agent`       | The client id that called.                                                       |
| `delegated`   | Whether the call is made for a person.                                           |
| `act`         | The token's `act` claim as JSON, empty when absent.                              |
| `target`      | The target the policy saw, as JSON.                                              |
| `arguments`   | The normalised primitive arguments the policy saw, as JSON; strings cut at 256 characters, a `comment_incident` `text` replaced by `text_length`; the whole record is capped at 4 KiB. |
| `decision`    | `permit` or `deny`.                                                              |
| `policies`    | The policies FerrisKey reported as having decided, as a JSON list.               |
| `reason`      | One of the reasons above when denied, empty otherwise.                           |

With `INGEST_KEY` and `INGEST_ENDPOINT` set, the same fields are sent to
FerrisTrace as OTLP logs (`service.name` `ferristrace-mcp-gateway`, severity
`INFO` for a permit and `WARN` for a deny), so they are searchable in Logs.
The export is batched and never blocks a decision: when the queue is full or
FerrisTrace does not accept a batch, the records are dropped and counted
(`dropped_total` in the gateway's own log).

The policy is given the arguments as the tool will run them: the gateway trims
text, lower-cases choices (log severity, alert state, incident status,
aggregation), writes ids in canonical form, and forwards a body rebuilt from
those same arguments. Both programs share that logic in `crates/mcp-args`.
