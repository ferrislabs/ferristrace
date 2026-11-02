use std::collections::HashMap;

use clickhouse::Row;
use ferristrace_domain::{
    ExportedSpan, ProjectId, Span, SpanCursor, SpanEvent, SpanPage, SpanPages, StoreError, TenantId,
};
use serde::Deserialize;
use uuid::Uuid;

use super::read::{
    ClickhouseReader, a_span, a_trace, map_of, nanos, read_kind, read_status, refused, unsigned,
};

const TRACES_PER_EVENT_QUERY: usize = 500;

const PAGE: &str = "SELECT \
     project_id AS project, \
     lower(hex(trace_id)) AS trace_hex, \
     lower(hex(span_id)) AS span_hex, \
     lower(hex(parent_span_id)) AS parent_hex, \
     service_name AS service, \
     name AS span_name, \
     toString(kind) AS kind_text, \
     toString(status) AS status_text, \
     toUnixTimestamp64Nano(start_time) AS start_ns, \
     toUnixTimestamp64Nano(end_time) AS end_ns, \
     attributes AS span_attributes \
   FROM spans FINAL \
   WHERE tenant_id = ? \
     AND (? = 0 OR (service_name, toStartOfHour(start_time), trace_id, span_id) \
          > (?, toStartOfHour(fromUnixTimestamp64Nano(?, 'UTC')), unhex(?), unhex(?))) \
   ORDER BY service_name, toStartOfHour(start_time), trace_id, span_id \
   LIMIT ?";

const EVENTS: &str = "SELECT \
     lower(hex(trace_id)) AS trace_hex, \
     lower(hex(span_id)) AS span_hex, \
     name AS event_name, \
     toUnixTimestamp64Nano(time) AS time_ns, \
     attributes AS event_attributes \
   FROM span_events FINAL \
   WHERE tenant_id = ? AND trace_id IN ({traces}) \
   ORDER BY time, name";

#[derive(Debug, Row, Deserialize)]
struct PageRow {
    #[serde(with = "clickhouse::serde::uuid")]
    project: Uuid,
    trace_hex: String,
    span_hex: String,
    parent_hex: Option<String>,
    service: String,
    span_name: String,
    kind_text: String,
    status_text: String,
    start_ns: i64,
    end_ns: i64,
    span_attributes: Vec<(String, String)>,
}

#[derive(Debug, Row, Deserialize)]
struct EventRow {
    trace_hex: String,
    span_hex: String,
    event_name: String,
    time_ns: i64,
    event_attributes: Vec<(String, String)>,
}

impl SpanPages for ClickhouseReader {
    async fn page(
        &self,
        org: TenantId,
        after: Option<&SpanCursor>,
        limit: u32,
    ) -> Result<SpanPage, StoreError> {
        let (service, start, trace, span) = after.map_or_else(
            || (String::new(), 0, String::new(), String::new()),
            |cursor| {
                (
                    cursor.service_name().to_owned(),
                    nanos(cursor.start_unix_nanos()),
                    hex::encode(cursor.trace_id().as_bytes()),
                    hex::encode(cursor.span_id().as_bytes()),
                )
            },
        );
        let rows: Vec<PageRow> = self
            .client()
            .query(PAGE)
            .bind(org.as_uuid())
            .bind(u8::from(after.is_some()))
            .bind(service)
            .bind(start)
            .bind(trace)
            .bind(span)
            .bind(limit.saturating_add(1))
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let mut events = self.events_of(org, &rows).await?;
        let probed = rows
            .into_iter()
            .map(|row| {
                let held = events
                    .remove(&(row.trace_hex.clone(), row.span_hex.clone()))
                    .unwrap_or_default();
                read_row(row, held)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SpanPage::cut(probed, limit))
    }
}

impl ClickhouseReader {
    async fn events_of(
        &self,
        org: TenantId,
        rows: &[PageRow],
    ) -> Result<HashMap<(String, String), Vec<SpanEvent>>, StoreError> {
        let mut traces: Vec<&str> = rows.iter().map(|row| row.trace_hex.as_str()).collect();
        traces.sort_unstable();
        traces.dedup();
        let wanted: std::collections::HashSet<(&str, &str)> = rows
            .iter()
            .map(|row| (row.trace_hex.as_str(), row.span_hex.as_str()))
            .collect();

        let mut held: HashMap<(String, String), Vec<SpanEvent>> = HashMap::new();
        for chunk in traces.chunks(TRACES_PER_EVENT_QUERY) {
            let listed = chunk
                .iter()
                .map(|trace| format!("unhex('{trace}')"))
                .collect::<Vec<_>>()
                .join(", ");
            let found: Vec<EventRow> = self
                .client()
                .query(&EVENTS.replace("{traces}", &listed))
                .bind(org.as_uuid())
                .fetch_all()
                .await
                .map_err(|error| refused(&error))?;
            for row in found {
                if !wanted.contains(&(row.trace_hex.as_str(), row.span_hex.as_str())) {
                    continue;
                }
                held.entry((row.trace_hex, row.span_hex))
                    .or_default()
                    .push(SpanEvent {
                        time_unix_nanos: unsigned(row.time_ns),
                        name: row.event_name.into(),
                        attributes: map_of(row.event_attributes),
                    });
            }
        }
        Ok(held)
    }
}

fn read_row(row: PageRow, events: Vec<SpanEvent>) -> Result<ExportedSpan, StoreError> {
    let parent = match row.parent_hex.as_deref() {
        None | Some("") => None,
        Some(spelling) => Some(a_span(spelling)?),
    };
    Ok(ExportedSpan {
        project: (!row.project.is_nil()).then(|| ProjectId::new(row.project)),
        span: Span {
            trace_id: a_trace(&row.trace_hex)?,
            span_id: a_span(&row.span_hex)?,
            parent,
            name: row.span_name.into(),
            service_name: row.service.into(),
            kind: read_kind(&row.kind_text),
            status: read_status(&row.status_text),
            start_unix_nanos: unsigned(row.start_ns),
            end_unix_nanos: unsigned(row.end_ns),
            attributes: map_of(row.span_attributes),
            events,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::{EVENTS, PAGE};

    #[test]
    fn every_query_is_bound_to_one_tenant() {
        for query in [PAGE, EVENTS] {
            assert!(
                query.contains("WHERE tenant_id = ?"),
                "a query without a tenant would export somebody else's spans"
            );
        }
    }

    #[test]
    fn the_page_resumes_from_the_key_it_is_ordered_by() {
        let key = "service_name, toStartOfHour(start_time), trace_id, span_id";
        assert!(PAGE.contains(&format!("ORDER BY {key}")));
        assert!(PAGE.contains(&format!("({key})")));
    }
}
