# Lot 11: the metrics explorer

Goal
  Metrics are read the way traces and logs are: pick a metric, filter on
  its attributes with the query language, group by a label, choose an
  aggregation, see a chart. Saved views and the zoom come from the same
  parts as the other signals.

Decisions
  D1  One query model. `Stream::Metrics` accepts `service` and attribute
      clauses only. Any other field is a 400 before any read.
  D2  The old GET series route stays (alerts, MCP and incident
      attachments use it). The query tree rides in a new POST.
  D3  Exemplars are not narrowed by a tree, since their rows carry no
      service or point attributes. A series with a filter returns no
      exemplars rather than unrelated ones.
  D4  Saved views use the same table with `signal = 'metrics'`.

Design (11a, server)
  Domain: `MetricQuery.filter: Query`, checked with `fits(Metrics)` in
  `resolved`. `MetricReader` gains `attributes(org, metric, filter)` and
  `attribute_values(org, metric, key, filter)`, empty by default.
  Store: `Compiled::of_metrics` over `metric_points` and
  `metric_histograms`; every key and value is a bound parameter.
  API:
    POST /api/orgs/{org}/metrics/{name}/series
         `{from, to, project?, query?, group_by?, agg?}` plus the
         placement fields, answers like the GET.
    GET  /api/orgs/{org}/metrics/{name}/attributes?from&to&project&prefix
    GET  /api/orgs/{org}/metrics/{name}/attributes/{key}/values?...

Design (11b, web)
  The Metrics page gains the query bar and the filter chips (signal
  metrics), the views panel, and keeps the name list, the aggregation
  and the group by. The filter is the url's `q`, the metric is `metric`;
  the old `f` parameter is read once and rewritten.

Acceptance (11a)
  A1  A tree on an attribute narrows a series; `NOT` and `!=` exclude.
  A2  A hostile value is bound, never spliced.
  A3  A span-only field on a metric is a 400.
  A4  Attribute keys and values are listed per metric and per tenant.
