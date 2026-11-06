use ferristrace_domain::{
    AttributeKey, BadQuery, Clause, Expr, Field, Number, Op, Query, Scalar, Stream, Value,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ExprBody {
    All { all: Vec<ExprBody> },
    Any { any: Vec<ExprBody> },
    Not { not: Box<ExprBody> },
    Is(ClauseBody),
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClauseBody {
    #[serde(default)]
    field: Option<FieldBody>,
    #[serde(default)]
    attr: Option<String>,
    op: OpBody,
    #[serde(default)]
    value: Option<ValueBody>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FieldBody {
    Service,
    Name,
    Status,
    Kind,
    Duration,
    HttpStatusCode,
    TraceId,
    SpanId,
    Severity,
    Body,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OpBody {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    NotContains,
    In,
    NotIn,
    Exists,
    NotExists,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ValueBody {
    Flag(bool),
    Count(f64),
    Word(String),
    Many(Vec<ScalarBody>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ScalarBody {
    Flag(bool),
    Count(f64),
    Word(String),
}

impl ScalarBody {
    fn scalar(self) -> Result<Scalar, BadQuery> {
        Ok(match self {
            Self::Flag(flag) => Scalar::Bool(flag),
            Self::Count(count) => Scalar::Number(Number::new(count)?),
            Self::Word(word) => Scalar::Text(word.into()),
        })
    }
}

impl ValueBody {
    fn value(self) -> Result<Value, BadQuery> {
        Ok(match self {
            Self::Flag(flag) => Value::One(Scalar::Bool(flag)),
            Self::Count(count) => Value::One(Scalar::Number(Number::new(count)?)),
            Self::Word(word) => Value::One(Scalar::Text(word.into())),
            Self::Many(many) => Value::Many(
                many.into_iter()
                    .map(ScalarBody::scalar)
                    .collect::<Result<_, _>>()?,
            ),
        })
    }
}

impl From<OpBody> for Op {
    fn from(op: OpBody) -> Self {
        match op {
            OpBody::Eq => Self::Eq,
            OpBody::Ne => Self::Ne,
            OpBody::Gt => Self::Gt,
            OpBody::Gte => Self::Gte,
            OpBody::Lt => Self::Lt,
            OpBody::Lte => Self::Lte,
            OpBody::Contains => Self::Contains,
            OpBody::NotContains => Self::NotContains,
            OpBody::In => Self::In,
            OpBody::NotIn => Self::NotIn,
            OpBody::Exists => Self::Exists,
            OpBody::NotExists => Self::NotExists,
        }
    }
}

impl From<FieldBody> for Field {
    fn from(field: FieldBody) -> Self {
        match field {
            FieldBody::Service => Self::Service,
            FieldBody::Name => Self::Name,
            FieldBody::Status => Self::Status,
            FieldBody::Kind => Self::Kind,
            FieldBody::Duration => Self::Duration,
            FieldBody::HttpStatusCode => Self::HttpStatusCode,
            FieldBody::TraceId => Self::TraceId,
            FieldBody::SpanId => Self::SpanId,
            FieldBody::Severity => Self::Severity,
            FieldBody::Body => Self::Body,
        }
    }
}

impl ClauseBody {
    fn clause(self) -> Result<Clause, BadQuery> {
        let field = match (self.field, self.attr) {
            (Some(field), None) => field.into(),
            (None, Some(key)) => Field::Attribute(AttributeKey::new(&key)?),
            _ => return Err(BadQuery::Shape),
        };
        let value = match self.value {
            Some(value) => value.value()?,
            None => Value::Nothing,
        };
        Clause::new(field, self.op.into(), value)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldRefBody {
    #[serde(default)]
    field: Option<FieldBody>,
    #[serde(default)]
    attr: Option<String>,
}

impl FieldRefBody {
    pub fn field(self, stream: Stream) -> Result<Field, BadQuery> {
        let field = match (self.field, self.attr) {
            (Some(field), None) => Field::from(field),
            (None, Some(key)) => Field::Attribute(AttributeKey::new(&key)?),
            _ => return Err(BadQuery::Shape),
        };
        if field.belongs_to(stream) {
            Ok(field)
        } else {
            Err(BadQuery::Field)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum FieldRefOut {
    Built { field: &'static str },
    Attribute { attr: String },
}

impl From<&Field> for FieldRefOut {
    fn from(field: &Field) -> Self {
        let built = |name| Self::Built { field: name };
        match field {
            Field::Service => built("service"),
            Field::Name => built("name"),
            Field::Status => built("status"),
            Field::Kind => built("kind"),
            Field::Duration => built("duration"),
            Field::HttpStatusCode => built("http_status_code"),
            Field::TraceId => built("trace_id"),
            Field::SpanId => built("span_id"),
            Field::Severity => built("severity"),
            Field::Body => built("body"),
            Field::Attribute(key) => Self::Attribute {
                attr: key.as_str().to_owned(),
            },
        }
    }
}

impl ExprBody {
    fn expr(self) -> Result<Expr, BadQuery> {
        Ok(match self {
            Self::All { all } => Expr::All(Self::parts(all)?),
            Self::Any { any } => Expr::Any(Self::parts(any)?),
            Self::Not { not } => Expr::Not(Box::new(not.expr()?)),
            Self::Is(clause) => Expr::Is(clause.clause()?),
        })
    }

    fn parts(parts: Vec<Self>) -> Result<Vec<Expr>, BadQuery> {
        parts.into_iter().map(Self::expr).collect()
    }

    pub fn query(self, stream: Stream) -> Result<Query, BadQuery> {
        let query = Query::new(self.expr()?)?;
        query.fits(stream)?;
        Ok(query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(json: &str) -> Result<Query, BadQuery> {
        serde_json::from_str::<ExprBody>(json)
            .expect("the shape is valid json for the body")
            .query(Stream::Spans)
    }

    #[test]
    fn a_clause_on_a_built_in_and_one_on_an_attribute() {
        let query = parsed(
            r#"{"all":[{"field":"service","op":"eq","value":"checkout"},
                       {"attr":"http.status_code","op":"gte","value":500}]}"#,
        )
        .expect("a valid query");

        assert!(!query.is_anything());
    }

    #[test]
    fn groups_nest_and_not_wraps_one_expression() {
        let query = parsed(
            r#"{"not":{"any":[{"attr":"a","op":"exists"},{"attr":"b","op":"in","value":["x",2,true]}]}}"#,
        );

        assert!(query.is_ok());
    }

    #[test]
    fn a_clause_names_a_field_or_an_attribute_never_both_or_neither() {
        assert_eq!(
            parsed(r#"{"field":"service","attr":"a","op":"eq","value":"x"}"#).unwrap_err(),
            BadQuery::Shape
        );
        assert_eq!(
            parsed(r#"{"op":"eq","value":"x"}"#).unwrap_err(),
            BadQuery::Shape
        );
    }

    #[test]
    fn the_domain_refuses_what_the_shape_allows() {
        assert_eq!(
            parsed(r#"{"field":"service","op":"gt","value":1}"#).unwrap_err(),
            BadQuery::Shape
        );
        assert_eq!(parsed(r#"{"all":[]}"#).unwrap_err(), BadQuery::Empty);
    }
}
