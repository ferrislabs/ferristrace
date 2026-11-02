use serde_json::Value;

use crate::fault::Fault;
use crate::render::Report;
use crate::window::Window;

const NANOS_PER_MILLI: u64 = 1_000_000;
const TOP_OPERATIONS: usize = 10;

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn count(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

pub fn moment(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok())
        .map_or_else(
            || ms.to_string(),
            |at| at.strftime("%Y-%m-%dT%H:%M:%SZ").to_string(),
        )
}

fn moment_ns(ns: u64) -> String {
    moment(ns / NANOS_PER_MILLI)
}

pub fn micros(us: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = us as f64;
    if us < 1_000 {
        format!("{us}µs")
    } else if us < 1_000_000 {
        format!("{:.1}ms", value / 1_000.0)
    } else {
        format!("{:.2}s", value / 1_000_000.0)
    }
}

pub fn number(value: f64) -> String {
    if !value.is_finite() {
        return "n/a".to_owned();
    }
    let fixed = format!("{value:.4}");
    fixed.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn percent(ratio: f64) -> String {
    format!("{:.2}%", ratio * 100.0)
}

fn maybe_number(value: &Value) -> String {
    value.as_f64().map_or_else(|| "-".to_owned(), number)
}

fn plural(count: usize, one: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {one}s")
    }
}

fn person(value: &Value) -> String {
    if value.is_null() {
        return "nobody".to_owned();
    }
    ["name", "email", "id"]
        .iter()
        .map(|key| text(value, key))
        .find(|held| !held.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

pub fn services(body: &Value, window: Window) -> Report {
    let rows = items(body, "services")
        .iter()
        .map(service_row)
        .collect::<Vec<_>>();
    Report::new(format!(
        "{} seen in the last {}",
        plural(rows.len(), "service"),
        window.name()
    ))
    .table(
        None,
        &[
            "service",
            "requests",
            "errors",
            "error rate",
            "req/s",
            "p50",
            "p95",
            "p99",
            "last seen",
        ],
        rows,
    )
}

fn service_row(service: &Value) -> Vec<String> {
    vec![
        text(service, "name").to_owned(),
        count(service, "requests").to_string(),
        count(service, "errors").to_string(),
        percent(service["error_rate"].as_f64().unwrap_or_default()),
        number(service["per_second"].as_f64().unwrap_or_default()),
        micros(count(service, "p50_us")),
        micros(count(service, "p95_us")),
        micros(count(service, "p99_us")),
        moment(count(service, "last_seen_ms")),
    ]
}

pub fn service_health(
    listing: &Value,
    name: &str,
    operations: &Value,
    window: Window,
) -> Result<Report, Fault> {
    let service = items(listing, "services")
        .iter()
        .find(|service| text(service, "name") == name)
        .ok_or_else(|| {
            Fault::argument(format!(
                "no service named {name:?} reported spans in the last {}; call list_services to see the names",
                window.name()
            ))
        })?;
    let mut ranked = items(operations, "operations").iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        count(b, "errors")
            .cmp(&count(a, "errors"))
            .then(count(b, "requests").cmp(&count(a, "requests")))
    });
    let rows = ranked
        .into_iter()
        .take(TOP_OPERATIONS)
        .map(|operation| {
            vec![
                text(operation, "name").to_owned(),
                count(operation, "requests").to_string(),
                count(operation, "errors").to_string(),
                micros(count(operation, "p95_ns") / 1_000),
                micros(count(operation, "p99_ns") / 1_000),
            ]
        })
        .collect();
    Ok(
        Report::new(format!("{name}: health over the last {}", window.name()))
            .bullets(vec![
                format!(
                    "requests: {} ({} per second)",
                    count(service, "requests"),
                    number(service["per_second"].as_f64().unwrap_or_default())
                ),
                format!(
                    "errors: {} ({})",
                    count(service, "errors"),
                    percent(service["error_rate"].as_f64().unwrap_or_default())
                ),
                format!(
                    "latency: p50 {}, p95 {}, p99 {}",
                    micros(count(service, "p50_us")),
                    micros(count(service, "p95_us")),
                    micros(count(service, "p99_us"))
                ),
                format!("last span: {}", moment(count(service, "last_seen_ms"))),
            ])
            .table(
                Some("Operations with the most errors"),
                &["operation", "requests", "errors", "p95", "p99"],
                rows,
            ),
    )
}

pub fn traces(body: &Value, window: Window) -> Report {
    let rows = items(body, "traces")
        .iter()
        .map(|trace| {
            vec![
                text(trace, "trace_id").to_owned(),
                moment(count(trace, "started_at_ms")),
                micros(count(trace, "duration_us")),
                text(trace, "root_service").to_owned(),
                text(trace, "root_name").to_owned(),
                count(trace, "spans").to_string(),
                count(trace, "errors").to_string(),
            ]
        })
        .collect::<Vec<_>>();
    Report::new(format!(
        "{} in the last {}",
        plural(rows.len(), "trace"),
        window.name()
    ))
    .table(
        None,
        &[
            "trace_id",
            "started",
            "duration",
            "service",
            "root span",
            "spans",
            "errors",
        ],
        rows,
    )
}

fn failure_detail(span: &Value) -> String {
    let pick = |attributes: &Value| {
        [
            "exception.message",
            "error.message",
            "http.response.status_code",
        ]
        .iter()
        .find_map(|key| attributes.get(key).and_then(Value::as_str))
        .map(str::to_owned)
    };
    pick(&span["attributes"])
        .or_else(|| {
            items(span, "events")
                .iter()
                .find_map(|event| pick(&event["attributes"]))
        })
        .unwrap_or_default()
}

pub fn trace(body: &Value) -> Report {
    let mut spans = items(body, "spans").iter().collect::<Vec<_>>();
    spans.sort_by_key(|span| count(span, "start_offset_us"));
    let failed = spans
        .iter()
        .filter(|span| text(span, "status") == "error")
        .count();
    let rows = spans
        .iter()
        .map(|span| {
            vec![
                text(span, "service").to_owned(),
                text(span, "name").to_owned(),
                text(span, "status").to_owned(),
                format!("+{}", micros(count(span, "start_offset_us"))),
                micros(count(span, "duration_us")),
                text(span, "span_id").to_owned(),
                text(span, "parent_span_id").to_owned(),
                failure_detail(span),
            ]
        })
        .collect();
    Report::new(format!(
        "trace {}: {}, {} total, {} with errors, started {}",
        text(body, "trace_id"),
        plural(spans.len(), "span"),
        micros(count(body, "duration_us")),
        failed,
        moment(count(body, "started_at_ms"))
    ))
    .table(
        None,
        &[
            "service", "span", "status", "start", "duration", "span_id", "parent", "detail",
        ],
        rows,
    )
}

pub fn logs(body: &Value, window: Window) -> Report {
    let rows = items(body, "records")
        .iter()
        .map(|record| {
            vec![
                moment_ns(count(record, "time_ns")),
                text(record, "severity").to_owned(),
                text(record, "service").to_owned(),
                text(record, "body").to_owned(),
                text(record, "trace_id").to_owned(),
            ]
        })
        .collect::<Vec<_>>();
    let more = if body.get("next").is_some_and(|next| !next.is_null()) {
        "; older lines exist, raise limit or narrow the filters"
    } else {
        ""
    };
    Report::new(format!(
        "{} in the last {}, newest first{more}",
        plural(rows.len(), "log line"),
        window.name()
    ))
    .table(
        None,
        &["time", "severity", "service", "message", "trace_id"],
        rows,
    )
}

pub struct MetricAsk<'a> {
    pub name: &'a str,
    pub aggregation: Option<&'a str>,
    pub service: Option<&'a str>,
    pub window: Window,
}

fn label_of<'a>(line: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| line["labels"].get(key).and_then(Value::as_str))
}

pub fn metric(body: &Value, ask: &MetricAsk<'_>) -> Report {
    let lines = items(body, "series")
        .iter()
        .filter(|line| {
            ask.service
                .is_none_or(|service| label_of(line, &["service.name", "service"]) == Some(service))
        })
        .collect::<Vec<_>>();
    let rows = lines
        .iter()
        .map(|line| {
            let values = line["points"]
                .as_array()
                .map(|points| {
                    points
                        .iter()
                        .filter_map(|point| point.get(1).and_then(Value::as_f64))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let labels = line["labels"].as_object().map_or_else(String::new, |map| {
                map.iter()
                    .map(|(key, value)| format!("{key}={}", value.as_str().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join(",")
            });
            let (last, low, high, mean) = summary(&values);
            vec![
                if labels.is_empty() {
                    "(all)".to_owned()
                } else {
                    labels
                },
                values.len().to_string(),
                last,
                low,
                high,
                mean,
            ]
        })
        .collect::<Vec<_>>();
    let aggregation = ask
        .aggregation
        .map_or_else(String::new, |agg| format!(" ({agg})"));
    let filtered = ask
        .service
        .map_or_else(String::new, |service| format!(" for service {service}"));
    Report::new(format!(
        "{}{aggregation}: {}{filtered} over the last {}",
        ask.name,
        plural(rows.len(), "series"),
        ask.window.name()
    ))
    .table(
        None,
        &["series", "points", "last", "min", "max", "avg"],
        rows,
    )
}

fn summary(values: &[f64]) -> (String, String, String, String) {
    let Some(last) = values.last() else {
        let none = "-".to_owned();
        return (none.clone(), none.clone(), none.clone(), none);
    };
    let low = values.iter().copied().fold(f64::INFINITY, f64::min);
    let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    #[allow(clippy::cast_precision_loss)]
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    (number(*last), number(low), number(high), number(mean))
}

pub fn cluster(body: &Value) -> Report {
    let pods = items(body, "pods");
    let unhealthy = pods
        .iter()
        .filter(|pod| pod["unhealthy"].as_bool().unwrap_or(false))
        .map(|pod| {
            vec![
                text(pod, "namespace").to_owned(),
                text(pod, "name").to_owned(),
                pod["phase"].as_str().unwrap_or("-").to_owned(),
                pod["ready"]
                    .as_bool()
                    .map_or("-", |ready| if ready { "yes" } else { "no" })
                    .to_owned(),
                count(pod, "restarts").to_string(),
                pod["reason"].as_str().unwrap_or("-").to_owned(),
                text(pod, "node").to_owned(),
            ]
        })
        .collect::<Vec<_>>();
    let degraded = items(body, "workloads")
        .iter()
        .filter(|workload| {
            matches!(
                (workload["desired"].as_u64(), workload["ready"].as_u64()),
                (Some(desired), Some(ready)) if ready < desired
            )
        })
        .map(|workload| {
            vec![
                text(workload, "namespace").to_owned(),
                format!("{}/{}", text(workload, "kind"), text(workload, "name")),
                format!(
                    "{}/{}",
                    workload["ready"].as_u64().unwrap_or_default(),
                    workload["desired"].as_u64().unwrap_or_default()
                ),
            ]
        })
        .collect::<Vec<_>>();
    let nodes = items(body, "nodes");
    let not_ready = nodes
        .iter()
        .filter(|node| node["ready"].as_bool() == Some(false))
        .count();
    let seen = body["seen_ms"]
        .as_u64()
        .map_or_else(|| "never".to_owned(), moment);
    Report::new(format!(
        "cluster status, last report {seen}: {}, {}, {}, {} unhealthy",
        plural(nodes.len(), "node"),
        plural(items(body, "workloads").len(), "workload"),
        plural(pods.len(), "pod"),
        unhealthy.len()
    ))
    .bullets(vec![
        format!("clusters: {}", join(items(body, "clusters"))),
        format!("namespaces: {}", join(items(body, "namespaces"))),
        format!("nodes not ready: {not_ready}"),
    ])
    .table(
        Some("Unhealthy pods"),
        &[
            "namespace",
            "pod",
            "phase",
            "ready",
            "restarts",
            "reason",
            "node",
        ],
        unhealthy,
    )
    .table(
        Some("Workloads below their desired replicas"),
        &["namespace", "workload", "ready"],
        degraded,
    )
}

fn join(values: &[Value]) -> String {
    let names = values
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if names.is_empty() {
        "none".to_owned()
    } else {
        names
    }
}

pub fn alerts(body: &Value, state: Option<&str>) -> Report {
    let rows = items(body, "alerts")
        .iter()
        .filter(|rule| state.is_none_or(|wanted| rule["state"]["status"].as_str() == Some(wanted)))
        .map(|rule| {
            vec![
                text(rule, "id").to_owned(),
                text(rule, "name").to_owned(),
                rule["state"]["status"].as_str().unwrap_or("-").to_owned(),
                text(rule, "severity").to_owned(),
                maybe_number(&rule["state"]["value"]),
                moment(count(&rule["state"], "since_ms")),
                if rule["enabled"].as_bool().unwrap_or(true) {
                    "yes".to_owned()
                } else {
                    "no".to_owned()
                },
            ]
        })
        .collect::<Vec<_>>();
    let filter = state.map_or_else(String::new, |wanted| format!(" in state {wanted}"));
    Report::new(format!("{}{filter}", plural(rows.len(), "alert rule"))).table(
        None,
        &[
            "id", "name", "state", "severity", "value", "since", "enabled",
        ],
        rows,
    )
}

pub fn alert_history(body: &Value, window: Window) -> Report {
    let mut events = items(body, "events").iter().collect::<Vec<_>>();
    events.sort_by_key(|event| std::cmp::Reverse(count(event, "at_ms")));
    let rows = events
        .iter()
        .map(|event| {
            let labels = event["labels"].as_object().map_or_else(String::new, |map| {
                map.iter()
                    .map(|(key, value)| format!("{key}={}", value.as_str().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join(",")
            });
            vec![
                moment(count(event, "at_ms")),
                format!("{} -> {}", text(event, "from"), text(event, "to")),
                maybe_number(&event["value"]),
                labels,
            ]
        })
        .collect::<Vec<_>>();
    Report::new(format!(
        "{} in the last {}, newest first",
        plural(rows.len(), "transition"),
        window.name()
    ))
    .table(None, &["time", "change", "value", "labels"], rows)
}

fn incident_services(incident: &Value) -> String {
    join(items(incident, "services"))
}

pub fn incidents(body: &Value, scope: &str) -> Report {
    let rows = items(body, "incidents")
        .iter()
        .map(|incident| {
            vec![
                text(incident, "id").to_owned(),
                text(incident, "severity").to_owned(),
                text(incident, "status").to_owned(),
                text(incident, "title").to_owned(),
                incident_services(incident),
                moment(count(incident, "opened_ms")),
                person(&incident["owner"]),
            ]
        })
        .collect::<Vec<_>>();
    Report::new(format!("{} ({scope})", plural(rows.len(), "incident"))).table(
        None,
        &[
            "id", "severity", "status", "title", "services", "opened", "owner",
        ],
        rows,
    )
}

fn timeline_detail(kind: &str, detail: &Value) -> String {
    let change = || format!("{} -> {}", text(detail, "from"), text(detail, "to"));
    match kind {
        "comment" => text(detail, "body").to_owned(),
        "status" | "severity" | "title" => change(),
        "public" => format!("{} -> {}", detail["from"], detail["to"]),
        "owner" => format!("{} -> {}", person(&detail["from"]), person(&detail["to"])),
        "opened" => format!("{} {}", text(detail, "severity"), text(detail, "title")),
        "alert" => format!("{}: {}", text(&detail["alert"], "name"), change()),
        "attachment" => format!("{} {}", text(detail, "kind"), text(detail, "label")),
        "paged" => format!(
            "step {} -> {}",
            count(detail, "step"),
            person(&detail["target"])
        ),
        "acknowledged" => format!("page {}", text(detail, "page")),
        "deploy" => format!(
            "{} {} ({})",
            text(detail, "service"),
            text(detail, "version"),
            text(detail, "environment")
        ),
        _ => detail.to_string(),
    }
}

pub fn incident(body: &Value) -> Report {
    let rows = items(body, "timeline")
        .iter()
        .map(|entry| {
            let author = &entry["author"];
            let mut who = text(author, "label").to_owned();
            if let Some(behalf) = author.get("on_behalf_of").and_then(Value::as_str) {
                who.push_str(" for ");
                who.push_str(behalf);
            }
            vec![
                moment(count(entry, "at_ms")),
                text(entry, "kind").to_owned(),
                who,
                timeline_detail(text(entry, "kind"), &entry["detail"]),
            ]
        })
        .collect::<Vec<_>>();
    let attachments = items(body, "attachments")
        .iter()
        .map(|attachment| {
            format!(
                "{}: {} ({})",
                text(attachment, "kind"),
                text(attachment, "label"),
                text(attachment, "reference")
            )
        })
        .collect::<Vec<_>>();
    let mut facts = vec![
        format!("severity: {}", text(body, "severity")),
        format!("status: {}", text(body, "status")),
        format!("owner: {}", person(&body["owner"])),
        format!("opened: {}", moment(count(body, "opened_ms"))),
        format!("services: {}", incident_services(body)),
    ];
    if let Some(resolved) = body["resolved_ms"].as_u64() {
        facts.push(format!("resolved: {}", moment(resolved)));
    }
    if let Some(alert) = body["alert"].as_object() {
        facts.push(format!(
            "alert: {}",
            alert
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
        ));
    }
    let mut report = Report::new(format!(
        "incident {}: {}",
        text(body, "id"),
        text(body, "title")
    ))
    .bullets(facts);
    if !attachments.is_empty() {
        report = report.bullets(attachments);
    }
    report.table(Some("Timeline"), &["time", "kind", "by", "detail"], rows)
}

pub struct Rostered {
    pub schedule: String,
    pub person: Value,
    pub until_ms: Option<u64>,
}

pub fn on_call(rosters: &[Rostered]) -> Report {
    let rows = rosters
        .iter()
        .map(|roster| {
            vec![
                roster.schedule.clone(),
                person(&roster.person),
                roster.until_ms.map_or_else(|| "-".to_owned(), moment),
            ]
        })
        .collect::<Vec<_>>();
    Report::new(format!("on call now: {}", plural(rows.len(), "schedule"))).table(
        None,
        &["schedule", "on call", "until"],
        rows,
    )
}

pub fn commented(incident_id: &str, entry: &Value) -> Report {
    Report::new(format!(
        "comment added to incident {incident_id} at {}",
        moment(count(entry, "at_ms"))
    ))
}

pub fn resolved(incident: &Value) -> Report {
    Report::new(format!(
        "incident {} is {}: {}",
        text(incident, "id"),
        text(incident, "status"),
        text(incident, "title")
    ))
}

pub fn acknowledged(page: &Value) -> Report {
    Report::new(format!(
        "page {} is {}: {}",
        text(page, "id"),
        text(page, "state"),
        text(page, "headline")
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn durations_pick_a_readable_unit() {
        assert_eq!(micros(850), "850µs");
        assert_eq!(micros(12_300), "12.3ms");
        assert_eq!(micros(1_500_000), "1.50s");
    }

    #[test]
    fn numbers_drop_trailing_zeros() {
        assert_eq!(number(3.0), "3");
        assert_eq!(number(0.5), "0.5");
        assert_eq!(number(1.234_567), "1.2346");
        assert_eq!(number(f64::NAN), "n/a");
    }

    #[test]
    fn moments_are_utc_seconds() {
        assert_eq!(moment(1_700_000_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(moment(u64::MAX), u64::MAX.to_string());
    }

    #[test]
    fn services_list_one_row_each() {
        let body = json!({"services": [{
            "name": "checkout", "requests": 120, "errors": 6, "per_second": 0.033,
            "error_rate": 0.05, "p50_us": 800, "p95_us": 12_000, "p99_us": 1_500_000,
            "last_seen_ms": 1_700_000_000_000_u64
        }]});
        let text = services(&body, Window::Hour).render();
        assert!(
            text.starts_with("1 service seen in the last 1h\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "checkout | 120 | 6 | 5.00% | 0.033 | 800µs | 12.0ms | 1.50s | 2023-11-14T22:13:20Z"
            ),
            "{text}"
        );
    }

    #[test]
    fn service_health_names_a_missing_service() {
        let fault = service_health(
            &json!({"services": []}),
            "ghost",
            &json!({"operations": []}),
            Window::Day,
        )
        .unwrap_err();
        assert!(fault.to_string().contains("\"ghost\""));
        assert!(fault.to_string().contains("24h"));
    }

    #[test]
    fn service_health_ranks_operations_by_errors() {
        let listing = json!({"services": [{"name": "api", "requests": 10, "errors": 2,
            "per_second": 1.0, "error_rate": 0.2, "p50_us": 1, "p95_us": 2, "p99_us": 3,
            "last_seen_ms": 0}]});
        let operations = json!({"operations": [
            {"name": "GET /a", "requests": 5, "errors": 0, "p95_ns": 0, "p99_ns": 0},
            {"name": "POST /b", "requests": 5, "errors": 2, "p95_ns": 0, "p99_ns": 0}
        ]});
        let text = service_health(&listing, "api", &operations, Window::Hour)
            .unwrap()
            .render();
        assert!(text.contains("- errors: 2 (20.00%)"), "{text}");
        assert!(text.find("POST /b").unwrap() < text.find("GET /a").unwrap());
    }

    #[test]
    fn a_trace_shows_error_spans_with_their_message() {
        let body = json!({"trace_id": "ab", "started_at_ms": 0, "duration_us": 5000, "spans": [
            {"span_id": "2", "parent_span_id": "1", "service": "db", "name": "query",
             "status": "error", "start_offset_us": 100, "duration_us": 900,
             "attributes": {}, "events": [{"name": "exception", "offset_us": 1,
             "attributes": {"exception.message": "deadlock"}}]},
            {"span_id": "1", "parent_span_id": null, "service": "api", "name": "GET /",
             "status": "ok", "start_offset_us": 0, "duration_us": 5000,
             "attributes": {}, "events": []}
        ]});
        let text = trace(&body).render();
        assert!(
            text.starts_with("trace ab: 2 spans, 5.0ms total, 1 with errors"),
            "{text}"
        );
        assert!(text.contains("deadlock"));
        assert!(text.find("GET /").unwrap() < text.find("query").unwrap());
    }

    #[test]
    fn logs_say_when_older_lines_exist() {
        let body = json!({"records": [{"time_ns": 1_700_000_000_000_000_000_u64,
            "severity": "error", "service": "api", "body": "boom\nsecond line",
            "trace_id": "ab"}], "next": "1.2"});
        let text = logs(&body, Window::Quarter).render();
        assert!(text.contains("older lines exist"));
        assert!(text.contains("2023-11-14T22:13:20Z | error | api | boom second line | ab"));
    }

    #[test]
    fn a_metric_is_summarised_per_series_and_filtered_by_service() {
        let body = json!({"series": [
            {"labels": {"service.name": "api"}, "points": [[1, 1.0], [2, null], [3, 3.0]]},
            {"labels": {"service.name": "web"}, "points": [[1, 9.0]]}
        ]});
        let ask = MetricAsk {
            name: "http.requests",
            aggregation: Some("sum"),
            service: Some("api"),
            window: Window::Hour,
        };
        let text = metric(&body, &ask).render();
        assert!(
            text.starts_with("http.requests (sum): 1 series for service api over the last 1h"),
            "{text}"
        );
        assert!(
            text.contains("service.name=api | 2 | 3 | 1 | 3 | 2"),
            "{text}"
        );
        assert!(!text.contains("web"));
    }

    #[test]
    fn the_cluster_lists_only_what_is_wrong() {
        let body = json!({"clusters": ["prod"], "namespaces": ["shop"],
            "nodes": [{"name": "n1", "ready": false}],
            "workloads": [
                {"kind": "deployment", "name": "cart", "namespace": "shop", "desired": 3, "ready": 1},
                {"kind": "deployment", "name": "fine", "namespace": "shop", "desired": 2, "ready": 2}
            ],
            "pods": [
                {"name": "cart-1", "namespace": "shop", "phase": "Running", "ready": false,
                 "restarts": 4, "unhealthy": true, "reason": "crash_loop", "node": "n1"},
                {"name": "fine-1", "namespace": "shop", "unhealthy": false, "restarts": 0}
            ], "seen_ms": 0});
        let text = cluster(&body).render();
        assert!(text.contains("2 pods, 1 unhealthy"), "{text}");
        assert!(text.contains("nodes not ready: 1"));
        assert!(text.contains("shop | cart-1 | Running | no | 4 | crash_loop | n1"));
        assert!(text.contains("shop | deployment/cart | 1/3"));
        assert!(!text.contains("fine"));
    }

    fn rule(name: &str, status: &str) -> Value {
        json!({"id": name, "name": name, "severity": "critical", "enabled": true,
            "state": {"status": status, "value": 1.5, "since_ms": 0}})
    }

    #[test]
    fn alerts_can_be_filtered_by_state() {
        let body = json!({"alerts": [rule("a", "firing"), rule("b", "ok")]});
        let text = alerts(&body, Some("firing")).render();
        assert!(text.starts_with("1 alert rule in state firing\n"), "{text}");
        assert!(text.contains("a | a | firing | critical | 1.5"));
        assert!(!text.contains("b | b"));
        assert!(alerts(&body, None).render().contains("2 alert rules"));
    }

    #[test]
    fn alert_history_is_newest_first() {
        let body = json!({"events": [
            {"at_ms": 1000, "from": "ok", "to": "firing", "value": 9.0, "labels": {"svc": "a"}},
            {"at_ms": 2000, "from": "firing", "to": "ok", "value": null, "labels": {}}
        ]});
        let text = alert_history(&body, Window::Day).render();
        assert!(text.find("firing -> ok").unwrap() < text.find("ok -> firing").unwrap());
        assert!(text.contains("svc=a"));
    }

    #[test]
    fn an_incident_shows_who_acted_for_whom() {
        let body = json!({"id": "i1", "title": "Checkout down", "severity": "sev1",
        "status": "investigating", "owner": null, "opened_ms": 0, "resolved_ms": null,
        "services": ["checkout"], "alert": null, "attachments": [],
        "timeline": [
            {"at_ms": 0, "kind": "comment", "author": {"kind": "agent",
             "label": "MCP gateway", "on_behalf_of": "ada@example.com"},
             "detail": {"body": "looking", "public": false}},
            {"at_ms": 1, "kind": "status", "author": {"kind": "system", "label": "system"},
             "detail": {"from": "investigating", "to": "resolved"}}
        ]});
        let text = incident(&body).render();
        assert!(text.starts_with("incident i1: Checkout down\n"), "{text}");
        assert!(text.contains("- owner: nobody"));
        assert!(text.contains("comment | MCP gateway for ada@example.com | looking"));
        assert!(text.contains("status | system | investigating -> resolved"));
    }

    #[test]
    fn on_call_shows_the_person_and_the_handoff() {
        let rosters = vec![
            Rostered {
                schedule: "primary".to_owned(),
                person: json!({"id": "u", "email": "ada@example.com", "name": "Ada"}),
                until_ms: Some(1_700_000_000_000),
            },
            Rostered {
                schedule: "backup".to_owned(),
                person: Value::Null,
                until_ms: None,
            },
        ];
        let text = on_call(&rosters).render();
        assert!(
            text.contains("primary | Ada | 2023-11-14T22:13:20Z"),
            "{text}"
        );
        assert!(text.contains("backup | nobody | -"));
    }
}
