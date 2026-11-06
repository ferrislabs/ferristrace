use clickhouse::query::Query as Statement;
use ferristrace_domain::{
    Clause, Expr, Field, Number, ProjectId, Query, Scalar, TenantId, Test, Window,
};
use serde::{Serialize, Serializer};

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
}

impl Serialize for Param {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Text(text) => serializer.serialize_str(text),
            Self::Float(number) => serializer.serialize_f64(*number),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Compiled {
    expression: String,
    params: Vec<Param>,
}

impl Compiled {
    pub(crate) fn of(query: &Query) -> Self {
        let mut compiled = Self::default();
        if let Some(expr) = query.filter() {
            compiled.expression = compiled.expr(expr);
        }
        compiled
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
        let column = Column::of(condition.field());
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
            Column::Attribute(key) => format!("attributes[{}]", self.text(key)),
        }
    }

    fn number_of(&mut self, column: &Column) -> String {
        match column {
            Column::Plain(sql) => format!("toFloat64OrNull({sql})"),
            Column::Measured(sql) => (*sql).to_owned(),
            Column::HttpStatus => "http_status_code".to_owned(),
            Column::Attribute(key) => format!("toFloat64OrNull(attributes[{}])", self.text(key)),
        }
    }

    fn present_of(&mut self, column: &Column) -> String {
        match column {
            Column::Plain(sql) => format!("{sql} != ''"),
            Column::Measured(_) => "1".to_owned(),
            Column::HttpStatus => "http_status_code IS NOT NULL".to_owned(),
            Column::Attribute(key) => format!("mapContains(attributes, {})", self.text(key)),
        }
    }
}

enum Column {
    Plain(&'static str),
    Measured(&'static str),
    HttpStatus,
    Attribute(String),
}

impl Column {
    fn of(field: &Field) -> Self {
        match field {
            Field::Service => Self::Plain("service_name"),
            Field::Name => Self::Plain("name"),
            Field::Status => Self::Plain("toString(status)"),
            Field::Kind => Self::Plain("toString(kind)"),
            Field::TraceId => Self::Plain("lower(hex(trace_id))"),
            Field::SpanId => Self::Plain("lower(hex(span_id))"),
            Field::Duration => Self::Measured(DURATION),
            Field::HttpStatusCode => Self::HttpStatus,
            Field::Attribute(key) => Self::Attribute(key.as_str().to_owned()),
        }
    }
}
