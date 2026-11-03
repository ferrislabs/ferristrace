# MCP server

`ferristrace-mcp` lets an MCP client (Claude Code, Claude Desktop, any MCP
host) read FerrisTrace and act on incidents. It is a small stateless HTTP
service in `apps/mcp`. It holds no credential and no database: every tool is
one or two calls to the FerrisTrace HTTP API, made with the bearer token the
request arrived with, so FerrisTrace still decides what that token may see.

This page is a stub. The full guide (connecting a client and the Cedar
policies) comes with part 7D of `docs/specs/lot-7-mcp.md`. The gateway's
configuration and its decision log are described at the end of this page.

## Run it

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

## Endpoints

- `POST /orgs/{org}/mcp`: MCP over streamable HTTP, stateless. The organisation
  id in the path scopes every API call the tools make.
- `GET /healthz`: answers 200, no token needed.

A request to the MCP endpoint without `Authorization: Bearer <token>` is
answered 401 before any tool runs. The server is meant to sit behind the
gateway (part 7C), which hands it a token already exchanged for FerrisTrace's
agent audience.

## Tools

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

## Answers

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
| `invalid_target`       | A name in the call cannot be looked up safely.                                           |
| `method_not_allowed`   | MCP method other than initialize, server/discover, ping, notifications, tools/list/call. |
| `header_mismatch`      | `Mcp-Method` or `Mcp-Name` disagrees with the body (HTTP 400).                           |

### The decision log

Every decision is one `tracing` event with `kind="mcp_decision"` (target
`mcp_decision`), once per tool for a `tools/list` and once per `tools/call`:

| Field         | Content                                                                          |
| ------------- | -------------------------------------------------------------------------------- |
| `request_id`  | `X-Request-Id` of the request, or a generated one; also sent back and upstream.  |
| `method`      | `tools/call` or `tools/list`, or the refused method.                             |
| `org`         | Organisation id.                                                                 |
| `tool`        | Tool name (empty for a refusal that is not about one tool).                      |
| `subject_type`, `subject` | `User` and the token's `sub`, or `Client` and the client id.         |
| `groups`      | The caller's groups, as a JSON list.                                             |
| `agent`       | The client id that called.                                                       |
| `delegated`   | Whether the call is made for a person.                                           |
| `act`         | The token's `act` claim as JSON, empty when absent.                              |
| `target`      | The target the policy saw, as JSON.                                              |
| `arguments`   | The primitive arguments the policy saw, as JSON.                                 |
| `decision`    | `permit` or `deny`.                                                              |
| `policies`    | The policies FerrisKey reported as having decided, as a JSON list.               |
| `reason`      | One of the reasons above when denied, empty otherwise.                           |

With `INGEST_KEY` and `INGEST_ENDPOINT` set, the same fields are sent to
FerrisTrace as OTLP logs (`service.name` `ferristrace-mcp-gateway`, severity
`INFO` for a permit and `WARN` for a deny), so they are searchable in Logs.
The export is batched and never blocks a decision: when the queue is full or
FerrisTrace does not accept a batch, the records are dropped and counted
(`dropped_total` in the gateway's own log).
