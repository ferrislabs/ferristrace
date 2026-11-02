use std::collections::BTreeMap;

use opentelemetry_proto::tonic::common::v1::{AnyValue, KeyValue, any_value::Value};

pub(crate) type Attributes = BTreeMap<Box<str>, Box<str>>;

pub(crate) fn collect(pairs: Vec<KeyValue>) -> Attributes {
    pairs
        .into_iter()
        .filter_map(|pair| {
            let value = render(pair.value?)?;
            Some((pair.key.into_boxed_str(), value))
        })
        .collect()
}

pub(crate) fn merged(resource: &Attributes, pairs: Vec<KeyValue>) -> Attributes {
    let mut attributes = resource.clone();
    attributes.extend(collect(pairs));
    attributes
}

pub(crate) fn lookup(pairs: &[KeyValue], key: &str) -> Option<Box<str>> {
    pairs
        .iter()
        .find(|pair| pair.key == key)
        .and_then(|pair| pair.value.clone())
        .and_then(render)
}

fn render(value: AnyValue) -> Option<Box<str>> {
    match value.value? {
        Value::StringValue(text) => Some(text.into_boxed_str()),
        Value::BoolValue(flag) => Some(flag.to_string().into_boxed_str()),
        Value::IntValue(number) => Some(number.to_string().into_boxed_str()),
        Value::DoubleValue(number) => Some(number.to_string().into_boxed_str()),
        Value::BytesValue(_)
        | Value::ArrayValue(_)
        | Value::KvlistValue(_)
        | Value::StringValueStrindex(_) => None,
    }
}
