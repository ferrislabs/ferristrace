use super::errors::BadQuery;

pub const MAX_DEPTH: usize = 8;
pub const MAX_CONDITIONS: usize = 32;
pub const MAX_LIST: usize = 50;
const MAX_TEXT: usize = 512;
const MAX_KEY: usize = 128;

const STATUSES: [&str; 3] = ["unset", "ok", "error"];
const KINDS: [&str; 6] = [
    "unspecified",
    "internal",
    "server",
    "client",
    "producer",
    "consumer",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeKey(Box<str>);

impl AttributeKey {
    pub fn new(key: &str) -> Result<Self, BadQuery> {
        if key.is_empty() || key.len() > MAX_KEY || key.chars().any(char::is_control) {
            return Err(BadQuery::Key);
        }
        Ok(Self(key.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Number(f64);

impl Number {
    pub fn new(value: f64) -> Result<Self, BadQuery> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(BadQuery::Number)
        }
    }

    pub fn get(self) -> f64 {
        self.0
    }
}

impl Eq for Number {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scalar {
    Text(Box<str>),
    Number(Number),
    Bool(bool),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Field {
    Service,
    Name,
    Status,
    Kind,
    Duration,
    HttpStatusCode,
    TraceId,
    SpanId,
    Attribute(AttributeKey),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    One(Scalar),
    Many(Vec<Scalar>),
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Test {
    Eq(Scalar),
    Ne(Scalar),
    Gt(Number),
    Gte(Number),
    Lt(Number),
    Lte(Number),
    Contains(Box<str>),
    NotContains(Box<str>),
    In(Vec<Scalar>),
    NotIn(Vec<Scalar>),
    Exists,
    NotExists,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clause {
    field: Field,
    test: Test,
}

impl Clause {
    pub fn new(field: Field, op: Op, value: Value) -> Result<Self, BadQuery> {
        let test = match (op, value) {
            (Op::Exists, Value::Nothing) => Test::Exists,
            (Op::NotExists, Value::Nothing) => Test::NotExists,
            (Op::Eq, Value::One(one)) => Test::Eq(one),
            (Op::Ne, Value::One(one)) => Test::Ne(one),
            (Op::Gt, Value::One(Scalar::Number(n))) => Test::Gt(n),
            (Op::Gte, Value::One(Scalar::Number(n))) => Test::Gte(n),
            (Op::Lt, Value::One(Scalar::Number(n))) => Test::Lt(n),
            (Op::Lte, Value::One(Scalar::Number(n))) => Test::Lte(n),
            (Op::Contains, Value::One(Scalar::Text(text))) => Test::Contains(text),
            (Op::NotContains, Value::One(Scalar::Text(text))) => Test::NotContains(text),
            (Op::In, Value::Many(many)) => Test::In(many),
            (Op::NotIn, Value::Many(many)) => Test::NotIn(many),
            _ => return Err(BadQuery::Shape),
        };
        let condition = Self { field, test };
        condition.check()?;
        Ok(condition)
    }

    pub fn field(&self) -> &Field {
        &self.field
    }

    pub fn test(&self) -> &Test {
        &self.test
    }

    fn check(&self) -> Result<(), BadQuery> {
        for text in self.texts() {
            if text.len() > MAX_TEXT {
                return Err(BadQuery::Text);
            }
        }
        if let Test::In(many) | Test::NotIn(many) = &self.test
            && (many.is_empty() || many.len() > MAX_LIST)
        {
            return Err(BadQuery::List);
        }
        match &self.field {
            Field::Attribute(_) => Ok(()),
            Field::Duration | Field::HttpStatusCode => self.numeric(),
            Field::Service | Field::Name => self.textual(),
            Field::Status => self.among(&STATUSES),
            Field::Kind => self.among(&KINDS),
            Field::TraceId => self.hex(32),
            Field::SpanId => self.hex(16),
        }
    }

    fn texts(&self) -> Vec<&str> {
        let scalars: &[Scalar] = match &self.test {
            Test::Eq(one) | Test::Ne(one) => std::slice::from_ref(one),
            Test::In(many) | Test::NotIn(many) => many,
            Test::Contains(text) | Test::NotContains(text) => return vec![text],
            _ => &[],
        };
        scalars
            .iter()
            .filter_map(|scalar| match scalar {
                Scalar::Text(text) => Some(&**text),
                _ => None,
            })
            .collect()
    }

    fn scalars(&self) -> &[Scalar] {
        match &self.test {
            Test::Eq(one) | Test::Ne(one) => std::slice::from_ref(one),
            Test::In(many) | Test::NotIn(many) => many,
            _ => &[],
        }
    }

    fn numeric(&self) -> Result<(), BadQuery> {
        match &self.test {
            Test::Contains(_) | Test::NotContains(_) => Err(BadQuery::Shape),
            _ if self
                .scalars()
                .iter()
                .all(|scalar| matches!(scalar, Scalar::Number(_))) =>
            {
                Ok(())
            }
            _ => Err(BadQuery::Shape),
        }
    }

    fn textual(&self) -> Result<(), BadQuery> {
        match &self.test {
            Test::Gt(_) | Test::Gte(_) | Test::Lt(_) | Test::Lte(_) => Err(BadQuery::Shape),
            _ if self
                .scalars()
                .iter()
                .all(|scalar| matches!(scalar, Scalar::Text(_))) =>
            {
                Ok(())
            }
            _ => Err(BadQuery::Shape),
        }
    }

    fn among(&self, known: &[&str]) -> Result<(), BadQuery> {
        match &self.test {
            Test::Eq(_) | Test::Ne(_) | Test::In(_) | Test::NotIn(_) => {}
            _ => return Err(BadQuery::Shape),
        }
        self.textual()?;
        if self.texts().iter().all(|text| known.contains(text)) {
            Ok(())
        } else {
            Err(BadQuery::Value)
        }
    }

    fn hex(&self, length: usize) -> Result<(), BadQuery> {
        match &self.test {
            Test::Eq(_) | Test::Ne(_) | Test::In(_) | Test::NotIn(_) => {}
            _ => return Err(BadQuery::Shape),
        }
        self.textual()?;
        let valid =
            |text: &&str| text.len() == length && text.bytes().all(|b| b.is_ascii_hexdigit());
        if self.texts().iter().all(valid) {
            Ok(())
        } else {
            Err(BadQuery::Value)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    All(Vec<Expr>),
    Any(Vec<Expr>),
    Not(Box<Expr>),
    Is(Clause),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    filter: Option<Expr>,
}

impl Query {
    pub fn anything() -> Self {
        Self::default()
    }

    pub fn new(filter: Expr) -> Result<Self, BadQuery> {
        let mut conditions = 0;
        Self::walk(&filter, 1, &mut conditions)?;
        Ok(Self {
            filter: Some(filter),
        })
    }

    pub fn filter(&self) -> Option<&Expr> {
        self.filter.as_ref()
    }

    pub fn is_anything(&self) -> bool {
        self.filter.is_none()
    }

    fn walk(expr: &Expr, depth: usize, conditions: &mut usize) -> Result<(), BadQuery> {
        if depth > MAX_DEPTH {
            return Err(BadQuery::Deep);
        }
        match expr {
            Expr::Is(_) => {
                *conditions += 1;
                if *conditions > MAX_CONDITIONS {
                    return Err(BadQuery::Large);
                }
                Ok(())
            }
            Expr::Not(inner) => Self::walk(inner, depth + 1, conditions),
            Expr::All(parts) | Expr::Any(parts) => {
                if parts.is_empty() {
                    return Err(BadQuery::Empty);
                }
                parts
                    .iter()
                    .try_for_each(|part| Self::walk(part, depth + 1, conditions))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Scalar {
        Scalar::Text(value.into())
    }

    fn number(value: f64) -> Scalar {
        Scalar::Number(Number::new(value).unwrap())
    }

    fn attribute(key: &str) -> Field {
        Field::Attribute(AttributeKey::new(key).unwrap())
    }

    fn is(field: Field, op: Op, value: Value) -> Expr {
        Expr::Is(Clause::new(field, op, value).unwrap())
    }

    fn eq(key: &str, value: &str) -> Expr {
        is(attribute(key), Op::Eq, Value::One(text(value)))
    }

    #[test]
    fn an_attribute_takes_every_operator_it_has_a_value_for() {
        assert!(Clause::new(attribute("rpc.method"), Op::Eq, Value::One(text("Pay"))).is_ok());
        assert!(Clause::new(attribute("retries"), Op::Gte, Value::One(number(3.0))).is_ok());
        assert!(Clause::new(attribute("db.system"), Op::Exists, Value::Nothing).is_ok());
        assert!(
            Clause::new(
                attribute("rpc.method"),
                Op::In,
                Value::Many(vec![text("Pay"), text("Refund")])
            )
            .is_ok()
        );
    }

    #[test]
    fn a_comparison_needs_a_number() {
        let refused = Clause::new(attribute("retries"), Op::Gt, Value::One(text("3")));

        assert_eq!(refused, Err(BadQuery::Shape));
    }

    #[test]
    fn a_number_operator_on_a_text_built_in_is_refused() {
        let refused = Clause::new(Field::Service, Op::Gt, Value::One(number(1.0)));

        assert_eq!(refused, Err(BadQuery::Shape));
    }

    #[test]
    fn a_text_value_on_a_number_built_in_is_refused() {
        let refused = Clause::new(Field::HttpStatusCode, Op::Eq, Value::One(text("500")));

        assert_eq!(refused, Err(BadQuery::Shape));
    }

    #[test]
    fn a_status_must_be_one_the_store_knows() {
        assert!(Clause::new(Field::Status, Op::Eq, Value::One(text("error"))).is_ok());
        assert_eq!(
            Clause::new(Field::Status, Op::Eq, Value::One(text("broken"))),
            Err(BadQuery::Value)
        );
    }

    #[test]
    fn a_trace_id_must_be_thirty_two_hex_digits() {
        let good = "0123456789abcdef0123456789abcdef";

        assert!(Clause::new(Field::TraceId, Op::Eq, Value::One(text(good))).is_ok());
        assert_eq!(
            Clause::new(Field::TraceId, Op::Eq, Value::One(text("abc"))),
            Err(BadQuery::Value)
        );
    }

    #[test]
    fn a_list_operator_needs_a_non_empty_bounded_list() {
        assert_eq!(
            Clause::new(attribute("a"), Op::In, Value::Many(vec![])),
            Err(BadQuery::List)
        );
        let many = (0..=MAX_LIST).map(|n| number(n as f64)).collect();
        assert_eq!(
            Clause::new(attribute("a"), Op::In, Value::Many(many)),
            Err(BadQuery::List)
        );
    }

    #[test]
    fn an_exists_test_takes_no_value() {
        assert_eq!(
            Clause::new(attribute("a"), Op::Exists, Value::One(text("x"))),
            Err(BadQuery::Shape)
        );
    }

    #[test]
    fn an_attribute_key_is_not_empty_too_long_or_a_control_character() {
        assert_eq!(AttributeKey::new(""), Err(BadQuery::Key));
        assert_eq!(AttributeKey::new(&"k".repeat(129)), Err(BadQuery::Key));
        assert_eq!(AttributeKey::new("a\nb"), Err(BadQuery::Key));
        assert!(AttributeKey::new("it's a \"key\"; DROP").is_ok());
    }

    #[test]
    fn a_number_is_finite() {
        assert_eq!(Number::new(f64::NAN), Err(BadQuery::Number));
        assert_eq!(Number::new(f64::INFINITY), Err(BadQuery::Number));
    }

    #[test]
    fn a_text_value_has_a_bound() {
        let refused = Clause::new(attribute("a"), Op::Eq, Value::One(text(&"v".repeat(513))));

        assert_eq!(refused, Err(BadQuery::Text));
    }

    #[test]
    fn a_query_nested_eight_levels_deep_is_accepted_and_nine_is_refused() {
        let nested =
            |levels: usize| (1..levels).fold(eq("a", "1"), |inner, _| Expr::Not(Box::new(inner)));

        assert!(Query::new(nested(8)).is_ok());
        assert_eq!(Query::new(nested(9)), Err(BadQuery::Deep));
    }

    #[test]
    fn a_query_holds_at_most_thirty_two_conditions() {
        let all = |count: usize| Expr::All((0..count).map(|n| eq("a", &n.to_string())).collect());

        assert!(Query::new(all(32)).is_ok());
        assert_eq!(Query::new(all(33)), Err(BadQuery::Large));
    }

    #[test]
    fn a_group_is_not_empty() {
        assert_eq!(Query::new(Expr::All(vec![])), Err(BadQuery::Empty));
        assert_eq!(
            Query::new(Expr::Not(Box::new(Expr::Any(vec![])))),
            Err(BadQuery::Empty)
        );
    }

    #[test]
    fn the_empty_query_asks_for_everything() {
        assert!(Query::anything().is_anything());
        assert!(Query::new(eq("a", "1")).unwrap().filter().is_some());
    }
}
