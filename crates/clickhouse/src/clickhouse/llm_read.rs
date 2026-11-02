use clickhouse::Row;
use clickhouse::query::Query;
use ferristrace_domain::{
    Call, CallCursor, CallPage, CallQuery, LlmReader, MODEL_LIMIT, PricedCall, ProjectId,
    SeenModel, StoreError, TenantId, Window,
};
use serde::Deserialize;
use uuid::Uuid;

use crate::clickhouse::issues::ClickhouseIssues;
use crate::clickhouse::read::{a_span, a_trace, nanos, refused, unsigned};

const TIEBREAK: &str = "cityHash64(trace_id, span_id)";

const CALLS: &str = "SELECT \
     lower(hex(trace_id)) AS trace_hex, \
     lower(hex(span_id)) AS span_hex, \
     service_name AS service, \
     toUnixTimestamp64Nano(time) AS time_ns, \
     duration_ns AS duration_ns, \
     status_error AS status_error, \
     provider AS provider, \
     operation AS operation, \
     request_model AS request_model, \
     response_model AS response_model, \
     input_tokens AS input_tokens, \
     output_tokens AS output_tokens, \
     cached_input_tokens AS cached_input_tokens, \
     feature AS feature, \
     customer AS customer, \
     user AS user, \
     prompt_version AS prompt_version, \
     environment AS environment, \
     {tiebreak} AS tiebreak \
   FROM llm_calls FINAL \
   WHERE {filters} \
   ORDER BY time DESC, tiebreak DESC \
   LIMIT ?";

const MODELS: &str = "SELECT \
     provider AS provider, \
     request_model AS request_model, \
     response_model AS response_model, \
     count() AS calls, \
     toUnixTimestamp64Nano(max(time)) AS last_nanos \
   FROM llm_calls FINAL \
   WHERE tenant_id = ? \
     AND time >= fromUnixTimestamp64Nano(?) \
     AND time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?) \
   GROUP BY provider, request_model, response_model \
   ORDER BY calls DESC, request_model, response_model \
   LIMIT {limit}";

#[derive(Debug, Row, Deserialize)]
struct CallRow {
    trace_hex: String,
    span_hex: String,
    service: String,
    time_ns: i64,
    duration_ns: u64,
    status_error: u8,
    provider: String,
    operation: String,
    request_model: String,
    response_model: String,
    input_tokens: u64,
    output_tokens: u64,
    cached_input_tokens: u64,
    feature: String,
    customer: String,
    user: String,
    prompt_version: String,
    environment: String,
    tiebreak: u64,
}

#[derive(Debug, Row, Deserialize)]
struct ModelRow {
    provider: String,
    request_model: String,
    response_model: String,
    calls: u64,
    last_nanos: i64,
}

enum Bound<'a> {
    Tenant(Uuid),
    Nanos(i64),
    Count(u64),
    Text(&'a str),
    Hex(String),
}

struct Filters<'a> {
    clauses: Vec<&'static str>,
    bound: Vec<Bound<'a>>,
}

impl<'a> Filters<'a> {
    fn of(org: TenantId, query: &'a CallQuery) -> Self {
        let mut filters = Self {
            clauses: Vec::new(),
            bound: Vec::new(),
        };
        filters.push("tenant_id = ?", [Bound::Tenant(org.as_uuid())]);
        filters.push(
            "time >= fromUnixTimestamp64Nano(?) AND time < fromUnixTimestamp64Nano(?)",
            [
                Bound::Nanos(nanos(query.window.from_unix_nanos())),
                Bound::Nanos(nanos(query.window.to_unix_nanos())),
            ],
        );
        if let Some(project) = query.project {
            filters.push("project_id = ?", [Bound::Tenant(project.as_uuid())]);
        }
        if let Some(feature) = query.feature.as_deref() {
            filters.push("feature = ?", [Bound::Text(feature)]);
        }
        if let Some(model) = query.model.as_deref() {
            filters.push(
                "(request_model = ? OR response_model = ?)",
                [Bound::Text(model), Bound::Text(model)],
            );
        }
        if let Some(customer) = query.customer.as_deref() {
            filters.push("customer = ?", [Bound::Text(customer)]);
        }
        if let Some(user) = query.user.as_deref() {
            filters.push("user = ?", [Bound::Text(user)]);
        }
        if let Some(version) = query.prompt_version.as_deref() {
            filters.push("prompt_version = ?", [Bound::Text(version)]);
        }
        if let Some(provider) = query.provider.as_deref() {
            filters.push("provider = ?", [Bound::Text(provider)]);
        }
        if let Some(trace) = query.trace {
            filters.push(
                "trace_id = unhex(?)",
                [Bound::Hex(hex::encode(trace.as_bytes()))],
            );
        }
        if let Some(before) = query.before {
            filters.push(
                "(time, cityHash64(trace_id, span_id)) < (fromUnixTimestamp64Nano(?), ?)",
                [
                    Bound::Nanos(nanos(before.time_unix_nanos)),
                    Bound::Count(before.tiebreak),
                ],
            );
        }
        filters
    }

    fn push<const N: usize>(&mut self, clause: &'static str, bound: [Bound<'a>; N]) {
        self.clauses.push(clause);
        self.bound.extend(bound);
    }

    fn sql(&self) -> String {
        CALLS
            .replace("{tiebreak}", TIEBREAK)
            .replace("{filters}", &self.clauses.join(" AND "))
    }

    fn bind(self, mut query: Query) -> Query {
        for bound in self.bound {
            query = match bound {
                Bound::Tenant(id) => query.bind(id),
                Bound::Nanos(at) => query.bind(at),
                Bound::Count(count) => query.bind(count),
                Bound::Text(text) => query.bind(text),
                Bound::Hex(spelling) => query.bind(spelling),
            };
        }
        query
    }
}

fn read_call(row: CallRow) -> Result<Call, StoreError> {
    Ok(Call {
        time_unix_nanos: unsigned(row.time_ns),
        trace: a_trace(&row.trace_hex)?,
        span: a_span(&row.span_hex)?,
        service: row.service.into(),
        provider: row.provider.into(),
        operation: row.operation.into(),
        request_model: row.request_model.into(),
        response_model: row.response_model.into(),
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        cached_input_tokens: row.cached_input_tokens,
        duration_nanos: row.duration_ns,
        error: row.status_error != 0,
        feature: row.feature.into(),
        customer: row.customer.into(),
        user: row.user.into(),
        prompt_version: row.prompt_version.into(),
        environment: row.environment.into(),
    })
}

impl LlmReader for ClickhouseIssues {
    async fn calls(&self, org: TenantId, query: &CallQuery) -> Result<CallPage, StoreError> {
        let filters = Filters::of(org, query);
        let sql = filters.sql();
        let limit = query.limit();
        let rows: Vec<CallRow> = filters
            .bind(self.client().query(&sql))
            .bind(u64::from(limit) + 1)
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;

        let more = rows.len() > limit as usize;
        let mut calls = Vec::with_capacity(rows.len().min(limit as usize));
        let mut next = None;
        for row in rows.into_iter().take(limit as usize) {
            next = Some(CallCursor {
                time_unix_nanos: unsigned(row.time_ns),
                tiebreak: row.tiebreak,
            });
            calls.push(PricedCall {
                call: read_call(row)?,
                cost: None,
            });
        }
        Ok(CallPage {
            calls,
            next: next.filter(|_| more),
        })
    }

    async fn models(
        &self,
        org: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Result<Vec<SeenModel>, StoreError> {
        let rows: Vec<ModelRow> = self
            .client()
            .query(&MODELS.replace("{limit}", &MODEL_LIMIT.to_string()))
            .bind(org.as_uuid())
            .bind(nanos(window.from_unix_nanos()))
            .bind(nanos(window.to_unix_nanos()))
            .bind(u8::from(project.is_some()))
            .bind(project.map(ProjectId::as_uuid).unwrap_or_default())
            .fetch_all()
            .await
            .map_err(|error| refused(&error))?;
        Ok(rows
            .into_iter()
            .map(|row| SeenModel {
                provider: row.provider.into(),
                request_model: row.request_model.into(),
                response_model: row.response_model.into(),
                calls: row.calls,
                last_unix_nanos: unsigned(row.last_nanos),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::{CALLS, MODELS};

    #[test]
    fn every_read_filters_on_the_tenant_and_narrows_the_project() {
        assert!(MODELS.contains("WHERE tenant_id = ?"));
        assert!(MODELS.contains("(? = 0 OR project_id = ?)"));
        assert!(CALLS.contains("{filters}"));
    }

    #[test]
    fn reads_deduplicate_replayed_spans() {
        assert!(CALLS.contains("FROM llm_calls FINAL"));
        assert!(MODELS.contains("FROM llm_calls FINAL"));
    }
}
