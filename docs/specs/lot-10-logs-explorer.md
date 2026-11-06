# Lot 10: the logs explorer

Goal
  Logs are read the way traces are: filters on any attribute, a typed
  query, views the organisation shares, a histogram with a brush, and a
  link between a log and its trace. It reuses the query model of lot 9.

Decisions
  D1  One query model for spans and logs. `Field` gains `severity` and
      `body`; `Query::fits(Stream)` refuses a field the stream does not
      have, so a log query with `duration` is a 400 before any read.
  D2  Resource attributes and log attributes are one namespace for a
      query. A log attribute wins over a resource attribute of the same
      key (`mapUpdate(resource, attributes)`). Exists matches either map.
  D3  The old GET routes stay, as the alerts and MCP callers use them.
      The query tree rides in new POST routes.
  D4  Saved views use the same table with `signal = 'logs'`.

Design (10a, server)
  Domain: `Field::Severity` (one of trace, debug, info, warn, error,
  fatal) and `Field::Body` (text, contains allowed). `LogQuery` gains
  `query: Query`, ANDed with the older fields. `LogReader` gains
  `attributes`, `attribute_values` and `facets`, empty by default.
  Store: the ClickHouse compiler takes a table (spans or logs) and binds
  every key and value, as it does for spans.
  API:
    POST /api/orgs/{org}/logs/search
         `{from, to, project?, query?, trace?, before?, limit?}`,
         answers like GET /logs.
    POST /api/orgs/{org}/logs/series
         `{from, to, project?, query?, buckets?}`, answers like
         GET /logs/histogram: counts per severity per bucket.
    POST /api/orgs/{org}/logs/facets
         `{from, to, project?, query?, facets: [{field}|{attr}]}`,
         the fields service and severity or any attribute.
    GET  /api/orgs/{org}/log-attributes?from&to&project&prefix
    GET  /api/orgs/{org}/log-attributes/{key}/values?...
  Clause fields for logs: service, severity, body, trace_id, span_id,
  or an attribute key.

Design (10b, web)
  The Logs page takes the trace explorer's parts: query bar, filter
  chips, facets panel, views panel (signal logs), histogram with brush
  (total records, the error and fatal share in red). The list keeps its
  paging and live tail. The query is the url's `q`, the zoom is `from`
  and `to`, the view is `view`, as on traces. A `SignalProvider` tells
  the shared parts which signal they serve: its attribute routes, its
  facet route, its built-in fields and its saved views (`signal=logs`).
  Built-in views: Recent, Errors, Warnings. The older `f` parameter
  (incident attachments, alert links, trace pages, the overview) is read
  once and rewritten as `q`, `from` and `to`. The search and series
  routes also take the Kubernetes placement fields.

Design (10c)
  A log with a trace id links to the trace; a trace shows its logs.

Acceptance (10a)
  A1  `severity = error` returns the error logs only.
  A2  `body CONTAINS declined OR severity = fatal` holds the union.
  A3  An attribute set on the resource matches like a log attribute.
  A4  A key with a quote is bound as data and returns nothing.
  A5  A span field in a log query is a 400; a log field in a span query
      is a 400.
  A6  Facets count without the clauses on their own field.
  A7  Another organisation's logs never match.
