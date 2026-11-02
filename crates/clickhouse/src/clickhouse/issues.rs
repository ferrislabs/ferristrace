use clickhouse::{Client, Row};
use ferristrace_domain::{
    Counted, Fingerprint, IssueFilter, IssueReader, Occurrence, ProjectId, StoreError, TenantId,
    Window,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::clickhouse::read::{a_span, a_trace, held, nanos, refused, unsigned, uuid_of};

const COUNTED: &str = "SELECT \
     toString(project_id) AS project_id, \
     lower(hex(fingerprint)) AS fingerprint, \
     any(attributes['exception.type']) AS exception_type, \
     argMax(attributes['exception.message'], time) AS message, \
     any(service_name) AS service, \
     count() AS seen, \
     toUnixTimestamp64Nano(min(time)) AS first_seen, \
     toUnixTimestamp64Nano(max(time)) AS last_seen \
   FROM span_events \
   WHERE tenant_id = ? \
     AND name = 'exception' \
     AND time >= fromUnixTimestamp64Nano(?) \
     AND time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?) \
     AND (? = '' OR service_name = ?) \
     AND (? = '' OR fingerprint = toFixedString(unhex(?), 16)) \
   GROUP BY project_id, fingerprint \
   HAVING (? = '' \
       OR positionCaseInsensitive(exception_type, ?) > 0 \
       OR positionCaseInsensitive(message, ?) > 0) \
   ORDER BY last_seen DESC \
   LIMIT ?";

const OCCURRENCES: &str = "SELECT \
     service_name AS service, \
     attributes['exception.type'] AS exception_type, \
     attributes['exception.message'] AS message, \
     attributes['exception.stacktrace'] AS stacktrace, \
     lower(hex(trace_id)) AS trace_id, \
     lower(hex(span_id)) AS span_id, \
     toUnixTimestamp64Nano(time) AS time \
   FROM span_events \
   WHERE tenant_id = ? \
     AND project_id = ? \
     AND name = 'exception' \
     AND fingerprint = toFixedString(unhex(?), 16) \
     AND time >= fromUnixTimestamp64Nano(?) \
     AND time < fromUnixTimestamp64Nano(?) \
   ORDER BY time DESC \
   LIMIT ?";

#[derive(Debug, Clone)]
pub struct ClickhouseIssues {
    client: Client,
}

impl ClickhouseIssues {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub(crate) fn client(&self) -> &Client {
        &self.client
    }
}

#[derive(Debug, Row, Deserialize)]
struct CountedRow {
    project_id: String,
    fingerprint: String,
    exception_type: String,
    message: String,
    service: String,
    seen: u64,
    first_seen: i64,
    last_seen: i64,
}

#[derive(Debug, Row, Deserialize)]
struct OccurrenceRow {
    service: String,
    exception_type: String,
    message: String,
    stacktrace: String,
    trace_id: String,
    span_id: String,
    time: i64,
}

impl IssueReader for ClickhouseIssues {
    async fn issues(
        &self,
        tenant: TenantId,
        filter: &IssueFilter,
    ) -> Result<Vec<Counted>, StoreError> {
        let service = filter.service.as_deref().unwrap_or_default();
        let text = filter.text.as_deref().unwrap_or_default();
        let print = filter
            .print
            .map(|print| print.to_string())
            .unwrap_or_default();
        let rows: Vec<CountedRow> = self
            .client
            .query(COUNTED)
            .bind(tenant.as_uuid())
            .bind(nanos(filter.window.from_unix_nanos()))
            .bind(nanos(filter.window.to_unix_nanos()))
            .bind(u8::from(filter.project.is_some()))
            .bind(uuid_of(filter.project))
            .bind(service)
            .bind(service)
            .bind(print.as_str())
            .bind(print.as_str())
            .bind(text)
            .bind(text)
            .bind(text)
            .bind(filter.limit())
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        rows.into_iter().map(read_counted).collect()
    }

    async fn occurrences(
        &self,
        tenant: TenantId,
        project: ProjectId,
        print: Fingerprint,
        window: Window,
        limit: u32,
    ) -> Result<Vec<Occurrence>, StoreError> {
        let rows: Vec<OccurrenceRow> = self
            .client
            .query(OCCURRENCES)
            .bind(tenant.as_uuid())
            .bind(project.as_uuid())
            .bind(print.to_string())
            .bind(nanos(window.from_unix_nanos()))
            .bind(nanos(window.to_unix_nanos()))
            .bind(limit)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        rows.into_iter().map(read_occurrence).collect()
    }
}

fn read_counted(row: CountedRow) -> Result<Counted, StoreError> {
    Ok(Counted {
        project: ProjectId::new(Uuid::parse_str(&row.project_id).map_err(|error| held(&error))?),
        print: Fingerprint::try_from(row.fingerprint.as_str()).map_err(|error| held(&error))?,
        exception_type: row.exception_type.into(),
        message: row.message.into(),
        service: row.service.into(),
        count: row.seen,
        first_seen_unix_nanos: unsigned(row.first_seen),
        last_seen_unix_nanos: unsigned(row.last_seen),
    })
}

fn read_occurrence(row: OccurrenceRow) -> Result<Occurrence, StoreError> {
    Ok(Occurrence {
        service: row.service.into(),
        exception_type: row.exception_type.into(),
        message: row.message.into(),
        stacktrace: Some(row.stacktrace)
            .filter(|held| !held.is_empty())
            .map(Into::into),
        trace_id: a_trace(&row.trace_id)?,
        span_id: a_span(&row.span_id)?,
        time_unix_nanos: unsigned(row.time),
    })
}

#[cfg(test)]
mod tests {
    use ferristrace_domain::EXCEPTION_EVENT;

    use super::{COUNTED, OCCURRENCES};
    use crate::clickhouse::rows::{EXCEPTION_MESSAGE, EXCEPTION_STACKTRACE, EXCEPTION_TYPE};

    #[test]
    fn neither_query_reads_across_tenants() {
        for query in [COUNTED, OCCURRENCES] {
            assert!(
                query.contains("WHERE tenant_id = ?"),
                "a query without a tenant would count somebody else's exceptions"
            );
        }
    }

    #[test]
    fn both_queries_read_the_attribute_keys_the_sink_writes() {
        assert!(COUNTED.contains(&format!("attributes['{EXCEPTION_TYPE}']")));
        assert!(COUNTED.contains(&format!("attributes['{EXCEPTION_MESSAGE}']")));
        assert!(OCCURRENCES.contains(&format!("attributes['{EXCEPTION_STACKTRACE}']")));
    }

    #[test]
    fn neither_query_counts_an_event_that_is_not_an_exception() {
        for query in [COUNTED, OCCURRENCES] {
            assert!(
                query.contains(&format!("name = '{EXCEPTION_EVENT}'")),
                "a query without the event name would group log events into issues"
            );
        }
    }

    #[test]
    fn the_list_groups_by_the_pair_that_decides_an_issue() {
        assert!(COUNTED.contains("GROUP BY project_id, fingerprint"));
        assert!(
            COUNTED.contains("ORDER BY last_seen DESC"),
            "the list is last seen first"
        );
    }
}
