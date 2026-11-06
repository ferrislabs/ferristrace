use clickhouse::query::Query as Statement;
use ferristrace_domain::{
    Clause, Expr, Field, Measure, Number, ProjectId, Query, Scalar, SeriesFilter, TableFilter,
    TenantId, Test, Window,
};
use serde::{Serialize, Serializer};
use uuid::Uuid;

use crate::clickhouse::read::{nanos, uuid_of};

const DURATION: &str = "(toUnixTimestamp64Nano(end_time) - toUnixTimestamp64Nano(start_time))";

const SPANS_MATCHING: &str = " AND trace_id IN (SELECT trace_id FROM spans \
   WHERE tenant_id = ? \
     AND start_time >= fromUnixTimestamp64Nano(?) AND start_time < fromUnixTimestamp64Nano(?) \
     AND (? = 0 OR project_id = ?) AND ({expression}))";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Param {
    Text(String),
    Float(f64),
    Whole(i64),
    Flag(u8),
    Id(Uuid),
}

impl Serialize for Param {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(text) => serializer.serialize_str(text),
            Self::Float(number) => serializer.serialize_f64(*number),
            Self::Whole(number) => serializer.serialize_i64(*number),
            Self::Flag(flag) => serializer.serialize_u8(*flag),
            Self::Id(id) => id.serialize(serializer),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Table {
    #[default]
    Spans,
    Logs,
}

impl Table {
    fn name(self) -> &'static str {
        match self {
            Self::Spans => "spans",
            Self::Logs => "logs",
        }
    }

    fn time(self) -> &'static str {
        match self {
            Self::Spans => "start_time",
            Self::Logs => "time",
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Compiled {
    expression: String,
    params: Vec<Param>,
    table: Table,
}

impl Compiled {
    pub(crate) fn of(query: &Query) -> Self {
        let mut compiled = Self::default();
        if let Some(expr) = query.filter() {
            compiled.expression = compiled.expr(expr);
        }
        compiled
    }

    pub(crate) fn of_logs(query: &Query) -> Self {
        let mut compiled = Self {
            table: Table::Logs,
            ..Self::default()
        };
        if let Some(expr) = query.filter() {
            compiled.expression = compiled.expr(expr);
        }
        compiled
    }

    pub(crate) fn expression(&self) -> &str {
        &self.expression
    }

    pub(crate) fn params(&self) -> &[Param] {
        &self.params
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.expression.is_empty()
    }

    pub(crate) fn traces_matching(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        SPANS_MATCHING.replace("{expression}", &self.expression)
    }

    pub(crate) fn bind_matching(
        &self,
        mut statement: Statement,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> Statement {
        if self.is_empty() {
            return statement;
        }
        statement = statement
            .bind(tenant.as_uuid())
            .bind(nanos(window.from_unix_nanos()))
            .bind(nanos(window.to_unix_nanos()))
            .bind(u8::from(project.is_some()))
            .bind(uuid_of(project));
        for param in &self.params {
            statement = statement.bind(param);
        }
        statement
    }

    pub(crate) fn facet(
        field: &Field,
        query: &Query,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
        limit: u32,
        ignore_own: bool,
    ) -> (String, Vec<Param>) {
        Self::facet_in(
            Table::Spans,
            field,
            query,
            tenant,
            window,
            project,
            limit,
            ignore_own,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn facet_in(
        table: Table,
        field: &Field,
        query: &Query,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
        limit: u32,
        ignore_own: bool,
    ) -> (String, Vec<Param>) {
        let mut compiled = Self {
            table,
            ..Self::default()
        };
        let column = Column::of(field, table);
        let value = compiled.text_of(&column);
        compiled.scoped(tenant, window, project);
        let present = compiled.present_of(&column);
        let narrowed = if ignore_own {
            query.without(field)
        } else {
            query.clone()
        };
        let filter = narrowed.filter().map(|expr| compiled.expr(expr));
        let mut sql = format!(
            "SELECT {value} AS value, count() AS spans FROM {name} \
             WHERE tenant_id = ? AND {time} >= fromUnixTimestamp64Nano(?) \
               AND {time} < fromUnixTimestamp64Nano(?) AND (? = 0 OR project_id = ?) \
               AND ({present})",
            name = table.name(),
            time = table.time(),
        );
        if let Some(filter) = filter {
            sql.push_str(" AND (");
            sql.push_str(&filter);
            sql.push(')');
        }
        sql.push_str(" GROUP BY value ORDER BY spans DESC, value LIMIT ?");
        compiled.params.push(Param::Whole(i64::from(limit)));
        (sql, compiled.params)
    }

    pub(crate) fn durations(
        query: &Query,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
    ) -> (String, Vec<Param>) {
        let mut compiled = Self::default();
        compiled.scoped(tenant, window, project);
        let narrowed = query.without(&Field::Duration);
        let filter = narrowed.filter().map(|expr| compiled.expr(expr));
        let mut sql = format!(
            "SELECT toUInt64(ifNull(min({DURATION}), 0)) AS shortest, \
                    toUInt64(ifNull(max({DURATION}), 0)) AS longest, count() AS spans \
             FROM spans \
             WHERE tenant_id = ? AND start_time >= fromUnixTimestamp64Nano(?) \
               AND start_time < fromUnixTimestamp64Nano(?) AND (? = 0 OR project_id = ?)"
        );
        if let Some(filter) = filter {
            sql.push_str(" AND (");
            sql.push_str(&filter);
            sql.push(')');
        }
        (sql, compiled.params)
    }

    pub(crate) fn series(
        filter: &SeriesFilter,
        groups: &[String],
        tenant: TenantId,
    ) -> (String, Vec<Param>) {
        let (group, measure, query) = (filter.group(), filter.measure, &filter.query);
        let (window, project, step_nanos) = (filter.window, filter.project, filter.step_nanos());
        let mut compiled = Self::default();
        let column = group.map(|field| Column::of(field, Table::Spans));
        let selected = column
            .as_ref()
            .map_or_else(|| "''".to_owned(), |column| compiled.text_of(column));
        compiled
            .params
            .push(Param::Whole(nanos(window.from_unix_nanos())));
        compiled
            .params
            .push(Param::Whole(i64::try_from(step_nanos).unwrap_or(i64::MAX)));
        compiled.scoped(tenant, window, project);
        let present = column.as_ref().map(|column| compiled.present_of(column));
        let mut sql = format!(
            "SELECT {selected} AS grp, \
                    toInt64(intDiv(toUnixTimestamp64Nano(start_time) - ?, ?)) AS slot, \
                    toFloat64({}) AS value \
             FROM spans FINAL \
             WHERE tenant_id = ? AND start_time >= fromUnixTimestamp64Nano(?) \
               AND start_time < fromUnixTimestamp64Nano(?) AND (? = 0 OR project_id = ?)",
            measured(measure)
        );
        if let Some(present) = present {
            sql.push_str(" AND (");
            sql.push_str(&present);
            sql.push(')');
        }
        if column.is_some() {
            let marks = vec!["?"; groups.len()].join(", ");
            sql.push_str(" AND grp IN (");
            sql.push_str(&marks);
            sql.push(')');
            compiled
                .params
                .extend(groups.iter().map(|group| Param::Text(group.clone())));
        }
        if let Some(filter) = query.filter().map(|expr| compiled.expr(expr)) {
            sql.push_str(" AND (");
            sql.push_str(&filter);
            sql.push(')');
        }
        sql.push_str(" GROUP BY grp, slot ORDER BY slot");
        (sql, compiled.params)
    }

    pub(crate) fn table(filter: &TableFilter, tenant: TenantId) -> (String, Vec<Param>) {
        let mut compiled = Self::default();
        let column = Column::of(filter.group(), Table::Spans);
        let selected = compiled.text_of(&column);
        compiled.scoped(tenant, filter.window, filter.project);
        let present = compiled.present_of(&column);
        let mut sql = format!(
            "SELECT {selected} AS grp, count() AS spans, countIf(status = 'error') AS errors, \
                    avg({DURATION}) AS average, \
                    quantiles(0.5, 0.95, 0.99)({DURATION}) AS latency \
             FROM spans FINAL \
             WHERE tenant_id = ? AND start_time >= fromUnixTimestamp64Nano(?) \
               AND start_time < fromUnixTimestamp64Nano(?) AND (? = 0 OR project_id = ?) \
               AND ({present})"
        );
        if let Some(query) = filter.query.filter().map(|expr| compiled.expr(expr)) {
            sql.push_str(" AND (");
            sql.push_str(&query);
            sql.push(')');
        }
        sql.push_str(" GROUP BY grp ORDER BY ");
        sql.push_str(match filter.order {
            Measure::Count => "spans",
            Measure::Errors => "errors",
            Measure::ErrorRate => "errors / spans",
            Measure::Average => "average",
            Measure::P50 => "latency[1]",
            Measure::P95 => "latency[2]",
            Measure::P99 => "latency[3]",
        });
        sql.push_str(" DESC, grp LIMIT ?");
        compiled.params.push(Param::Whole(i64::from(filter.rows())));
        (sql, compiled.params)
    }

    pub(crate) fn spans(
        query: &Query,
        tenant: TenantId,
        window: Window,
        project: Option<ProjectId>,
        by_duration: bool,
        limit: u32,
    ) -> (String, Vec<Param>) {
        let mut compiled = Self::default();
        compiled.scoped(tenant, window, project);
        let filter = query.filter().map(|expr| compiled.expr(expr));
        let mut sql = format!(
            "SELECT lower(hex(trace_id)) AS trace_hex, lower(hex(span_id)) AS span_hex, \
                    service_name AS service, name AS name, toString(kind) AS kind, \
                    toString(status) AS status, ifNull(http_status_code, 0) AS status_code, \
                    toUnixTimestamp64Nano(start_time) AS started_at, \
                    toUInt64({DURATION}) AS duration \
             FROM spans FINAL \
             WHERE tenant_id = ? AND start_time >= fromUnixTimestamp64Nano(?) \
               AND start_time < fromUnixTimestamp64Nano(?) AND (? = 0 OR project_id = ?)"
        );
        if let Some(filter) = filter {
            sql.push_str(" AND (");
            sql.push_str(&filter);
            sql.push(')');
        }
        sql.push_str(if by_duration {
            " ORDER BY duration DESC, start_time DESC, span_id LIMIT ?"
        } else {
            " ORDER BY start_time DESC, span_id LIMIT ?"
        });
        compiled.params.push(Param::Whole(i64::from(limit)));
        (sql, compiled.params)
    }

    fn scoped(&mut self, tenant: TenantId, window: Window, project: Option<ProjectId>) {
        self.params.extend([
            Param::Id(tenant.as_uuid()),
            Param::Whole(nanos(window.from_unix_nanos())),
            Param::Whole(nanos(window.to_unix_nanos())),
            Param::Flag(u8::from(project.is_some())),
            Param::Id(uuid_of(project)),
        ]);
    }

    fn expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Is(condition) => self.condition(condition),
            Expr::Not(inner) => format!("NOT ({})", self.expr(inner)),
            Expr::All(parts) => self.joined(parts, " AND "),
            Expr::Any(parts) => self.joined(parts, " OR "),
        }
    }

    fn joined(&mut self, parts: &[Expr], glue: &str) -> String {
        let sql: Vec<String> = parts.iter().map(|part| self.expr(part)).collect();
        format!("({})", sql.join(glue))
    }

    fn text(&mut self, text: &str) -> &'static str {
        self.params.push(Param::Text(text.to_owned()));
        "?"
    }

    fn float(&mut self, number: Number) -> &'static str {
        self.params.push(Param::Float(number.get()));
        "?"
    }

    fn condition(&mut self, condition: &Clause) -> String {
        let column = Column::of(condition.field(), self.table);
        match condition.test() {
            Test::Eq(scalar) => self.equals(&column, scalar),
            Test::Ne(scalar) => format!("NOT ({})", self.equals(&column, scalar)),
            Test::In(many) => self.any_of(&column, many),
            Test::NotIn(many) => format!("NOT ({})", self.any_of(&column, many)),
            Test::Gt(n) => self.compared(&column, ">", *n),
            Test::Gte(n) => self.compared(&column, ">=", *n),
            Test::Lt(n) => self.compared(&column, "<", *n),
            Test::Lte(n) => self.compared(&column, "<=", *n),
            Test::Contains(needle) => {
                let held = self.text_of(&column);
                format!("positionCaseInsensitive({held}, {}) > 0", self.text(needle))
            }
            Test::NotContains(needle) => {
                let held = self.text_of(&column);
                format!("positionCaseInsensitive({held}, {}) = 0", self.text(needle))
            }
            Test::Exists => self.present_of(&column),
            Test::NotExists => format!("NOT ({})", self.present_of(&column)),
        }
    }

    fn any_of(&mut self, column: &Column, many: &[Scalar]) -> String {
        let alternatives: Vec<String> = many
            .iter()
            .map(|scalar| self.equals(column, scalar))
            .collect();
        format!("({})", alternatives.join(" OR "))
    }

    fn equals(&mut self, column: &Column, scalar: &Scalar) -> String {
        match scalar {
            Scalar::Text(text) => {
                let held = self.text_of(column);
                format!("{held} = {}", self.text(text))
            }
            Scalar::Bool(flag) => {
                let held = self.text_of(column);
                format!("{held} = {}", self.text(&flag.to_string()))
            }
            Scalar::Number(number) => self.compared(column, "=", *number),
        }
    }

    fn compared(&mut self, column: &Column, operator: &str, number: Number) -> String {
        let held = self.number_of(column);
        format!("ifNull({held} {operator} {}, 0)", self.float(number))
    }

    fn text_of(&mut self, column: &Column) -> String {
        match column {
            Column::Plain(sql) => (*sql).to_owned(),
            Column::Measured(sql) => format!("toString({sql})"),
            Column::HttpStatus => "toString(http_status_code)".to_owned(),
            Column::Attribute(key, table) => format!("{}[{}]", held(*table), self.text(key)),
        }
    }

    fn number_of(&mut self, column: &Column) -> String {
        match column {
            Column::Plain(sql) => format!("toFloat64OrNull({sql})"),
            Column::Measured(sql) => (*sql).to_owned(),
            Column::HttpStatus => "http_status_code".to_owned(),
            Column::Attribute(key, table) => {
                format!("toFloat64OrNull({}[{}])", held(*table), self.text(key))
            }
        }
    }

    fn present_of(&mut self, column: &Column) -> String {
        match column {
            Column::Plain(sql) => format!("{sql} != ''"),
            Column::Measured(_) => "1".to_owned(),
            Column::HttpStatus => "http_status_code IS NOT NULL".to_owned(),
            Column::Attribute(key, Table::Spans) => {
                format!("mapContains(attributes, {})", self.text(key))
            }
            Column::Attribute(key, Table::Logs) => {
                let first = self.text(key);
                let second = self.text(key);
                format!("(mapContains(attributes, {first}) OR mapContains(resource, {second}))")
            }
        }
    }
}

fn held(table: Table) -> &'static str {
    match table {
        Table::Spans => "attributes",
        Table::Logs => "mapUpdate(resource, attributes)",
    }
}

fn measured(measure: Measure) -> String {
    match measure {
        Measure::Count => "count()".to_owned(),
        Measure::Errors => "countIf(status = 'error')".to_owned(),
        Measure::ErrorRate => {
            "if(count() = 0, 0, countIf(status = 'error') * 100 / count())".to_owned()
        }
        Measure::Average => format!("avg({DURATION}) / 1000"),
        Measure::P50 => format!("quantile(0.5)({DURATION}) / 1000"),
        Measure::P95 => format!("quantile(0.95)({DURATION}) / 1000"),
        Measure::P99 => format!("quantile(0.99)({DURATION}) / 1000"),
    }
}

enum Column {
    Plain(&'static str),
    Measured(&'static str),
    HttpStatus,
    Attribute(String, Table),
}

impl Column {
    fn of(field: &Field, table: Table) -> Self {
        if table == Table::Logs {
            return match field {
                Field::Service => Self::Plain("service_name"),
                Field::Severity => Self::Plain("toString(severity)"),
                Field::Body => Self::Plain("body"),
                Field::TraceId => Self::Plain("lower(hex(trace_id))"),
                Field::SpanId => Self::Plain("lower(hex(span_id))"),
                Field::Attribute(key) => Self::Attribute(key.as_str().to_owned(), table),
                _ => Self::Plain("''"),
            };
        }
        match field {
            Field::Service => Self::Plain("service_name"),
            Field::Name => Self::Plain("name"),
            Field::Status => Self::Plain("toString(status)"),
            Field::Kind => Self::Plain("toString(kind)"),
            Field::TraceId => Self::Plain("lower(hex(trace_id))"),
            Field::SpanId => Self::Plain("lower(hex(span_id))"),
            Field::Duration => Self::Measured(DURATION),
            Field::HttpStatusCode => Self::HttpStatus,
            Field::Attribute(key) => Self::Attribute(key.as_str().to_owned(), table),
            Field::Severity | Field::Body => Self::Plain("''"),
        }
    }
}
