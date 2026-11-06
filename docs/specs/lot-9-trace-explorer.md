# Lot 9: the trace explorer

Goal
  Reading traces stops being "pick a service and a span name". A person
  filters on any attribute of a span, saves the result as a view the
  whole organisation can open, switches between ways of looking at it
  (traces, spans, time series, table) and can type the query instead of
  clicking it.

Context
  `GET /api/orgs/{org}/traces` accepts a fixed list: service, name,
  errors, minimum duration, placement (crates/api/src/explore_routes.rs
  `Asked`, crates/domain/src/explore/value_objects.rs `TraceFilter`).
  Spans keep every attribute, resource ones included, in
  `spans.attributes Map(String, String)` plus three typed columns
  (crates/clickhouse/migrations/clickhouse/0001_spans.up.sql), so a
  filter on any attribute needs no migration of the span store. The
  placement filter already selects traces by "any span matches"
  (`trace_id IN (...)`, crates/clickhouse/src/clickhouse/read.rs).
  Nothing stores a view; the `dashboards` table is for dashboards.

Decisions (2026-11-06, owner)
  D1  Order of delivery: the query and its filters, saved views, the
      extra tabs, the typed query bar, then the UX extras.
  D2  Views can be shared with the organisation.
  D3  The query has AND, OR, NOT and parentheses from the start.

Design
  The query is a value, not a string. The server only ever receives and
  stores the tree; the typed syntax is parsed in the web.

  Domain (crates/domain/src/explore)
    `Query { filter: Option<Expr> }`
    `Expr` is `All(Vec<Expr>)`, `Any(Vec<Expr>)`, `Not(Box<Expr>)` or a
    `Condition { field, op, value }`.
    `Field` is a built-in (`service`, `name`, `status`, `kind`,
    `duration`, `http_status_code`, `trace_id`, `span_id`) or an
    `Attribute(key)`. A key is 1..=128 chars, no control characters.
    `Op` is `Eq`, `Ne`, `Gt`, `Gte`, `Lt`, `Lte`, `Contains`,
    `NotContains`, `In`, `NotIn`, `Exists`, `NotExists`.
    `Value` is text, number, bool or a list of the first two.
    Parsing, don't validate: an unknown op for a field, a number op on a
    text-only built-in, a list on a scalar op, more than 8 levels of
    nesting or more than 32 conditions cannot be constructed.
    `TraceFilter` gains `query: Query`; the old fields stay and are
    ANDed with it, so alerts, the MCP tools and the export keep working.

  Store (crates/clickhouse)
    The tree compiles to one parameterised `WHERE` fragment over `spans`.
    Attribute keys and values are bound, never interpolated.
    Number operators on an attribute use `toFloat64OrNull`; a span with a
    non-numeric value does not match.
    On the traces tab a trace matches when any of its spans in the
    window matches, as the placement does; its summary still covers the
    whole trace. On the spans tab a span matches on its own.

  API
    `POST /api/orgs/{org}/traces/search`
      body `{from, to, project?, order, limit, errors?, query?}` (the
      GET parameters plus a `query` tree); answers `{traces: [...]}`
      like the GET. The GET stays. A clause is
      `{"field": "service"|"name"|"status"|"kind"|"duration"|
      "http_status_code"|"trace_id"|"span_id", ...}` or
      `{"attr": "<key>", ...}`, then `"op"` and `"value"` (text,
      number, bool or a list; absent for exists). Groups are
      `{"all": [...]}`, `{"any": [...]}` and `{"not": {...}}`.
      `duration` is in nanoseconds.
    `GET  /api/orgs/{org}/attributes?from&to&project&prefix`
      answers `{attributes: [{key, spans, numeric}]}`, the 200 most
      used span attribute keys in the window. The built-in fields are
      a fixed list in the web.
    `GET  /api/orgs/{org}/attributes/{key}/values?from&to&project&prefix`
      answers `{values: [{value, spans}]}`, the 20 most used.
    Later lots add `POST .../explore/spans`, `.../series`, `.../table`.
    Views (lot 9b), person sessions only (an API key or an agent gets
    401):
    `GET /api/orgs/{org}/views?signal=traces` lists the organisation's
    views and the caller's private ones;
    `POST /api/orgs/{org}/views` with `{signal?, name, visibility,
    definition}` answers 201 with the view;
    `PUT /api/orgs/{org}/views/{id}` with `{name, definition,
    visibility?}`; `DELETE` answers 204.
    A name already taken is 409, a bad name, visibility or definition
    422, a view the caller may not see 404, one it may not change 403.
    `definition` is a JSON object the server stores as it is, at most
    16 KiB: the web owns its shape.

  Views (lot 9b)
    Postgres table `explorer_views(id, org_id, signal, name, visibility
    'private'|'org', definition jsonb, created_by, created_at,
    updated_at)`, unique on (org, signal, name, visibility) for org
    views and (org, signal, name, created_by) for private ones.
    `definition` is `{tab, query, order, columns, window}`; it is parsed
    into a domain type on the way in and out.
    Any member creates a private view or an org view. An org view is
    edited and deleted by its creator, an owner or an administrator.
    Deleting an org's member keeps their org views (created_by set null).
    The built-in views (Recent, Errors, Slow) are code, not rows: they
    cannot be edited, only duplicated.
    API keys do not read or write views; a person does.

  Web (web/src)
    Lot 9a: a filter bar of chips (attribute, operator, value) with
    suggestions from the two attribute endpoints, replacing the select
    boxes of the Explore view. The query rides in the url so a link
    shares it. Recent, Errors and Slow stay as the tabs of #250.
    Lot 9b: a Views menu replaces those tabs; they become built-in views.
    Lot 9c: a facets panel at the left of the list, and the views as a
    panel beside it, as SigNoz does. A facet is a section (Service,
    Status, Kind, Name, Duration, then the well-known attributes present
    in the window) that unfolds to the most used values with their span
    counts, to tick. A tick adds the chip `field is value`, two ticks make
    it `is one of`, and removing the chip unticks. The counts follow the
    query without the clauses on the facet's own field, so each box
    tells what ticking it would give. Duration is a min and a max in ms.
    `POST /api/orgs/{org}/traces/facets` takes `{from, to, project?,
    query?, facets: [{field}|{attr}], duration?}` (at most 12 facets) and
    answers `{facets: [{field|attr, values: [{value, spans}]}],
    duration: {min_us, max_us}|null}`. The values are the 10 most used.
    Lot 9d: tabs Traces, Spans, Time series, Table.
    Lot 9e: a typed bar (`service.name = "checkout" AND
    http.status_code >= 500`) that parses to the same tree, both ways.
    The URL then carries the typed form.
    Lot 9f: duration histogram with brush selection, click a value to
    filter or exclude, attribute facets with counts, column picker,
    side preview of a trace.

Acceptance (lot 9a)
  A1  A query `http.status_code >= 500` over a window holding one 200 and
      one 503 span returns the trace of the 503 only.
  A2  `service = checkout AND (rpc.method = Pay OR rpc.method = Refund)`
      returns the traces that have one span where the whole expression
      holds. A trace where `checkout` is one span and `rpc.method = Pay`
      is another span does not match: a query is evaluated on one span.
  A3  `Not(Exists(db.system))` returns spans without the attribute.
  A4  An attribute key with a quote, a backslash or `'; DROP` is bound as
      data and returns nothing, with no error.
  A5  A tree nested 9 levels deep, or with 33 conditions, is refused with
      400 before any query reaches ClickHouse.
  A6  A number operator on a text-only built-in is refused with 400.
  A7  Another organisation's spans never match, whatever the tree.
  A8  The attributes endpoint lists keys of the window only and respects
      `project`.
  A9  The GET route and the alert and MCP callers behave as before.
  A10 In the web, adding, editing and removing a chip changes the list
      and the url; reloading the url restores the chips.

Out of scope for 9a
  Regex, aggregate functions, a query across signals, saved views,
  traces tab pagination beyond `limit`.

Open points
  Q1  Does "all conditions hold on the same span" (A2) match what people
      expect from "trace contains an error and service checkout"? A
      second mode, "anywhere in the trace", would be a toggle per group.
  Q2  Resource attributes and span attributes share one map. Is a
      namespace in the key (`resource.k8s.pod.name`) wanted, or is the
      flat key enough?
