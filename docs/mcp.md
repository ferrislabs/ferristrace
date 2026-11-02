# MCP server

`ferristrace-mcp` lets an MCP client (Claude Code, Claude Desktop, any MCP
host) read FerrisTrace and act on incidents. It is a small stateless HTTP
service in `apps/mcp`. It holds no credential and no database: every tool is
one or two calls to the FerrisTrace HTTP API, made with the bearer token the
request arrived with, so FerrisTrace still decides what that token may see.

This page is a stub. The full guide (connecting a client, the policy gateway
and the Cedar policies) comes with part 7D of `docs/specs/lot-7-mcp.md`.

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
| `who_is_on_call`   | `schedule`                                                             | `GET .../pages`, `GET .../schedules/{id}`, `GET .../schedules/{id}/now` |
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
