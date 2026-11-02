use std::collections::BTreeMap;

use ferristrace_domain::{
    Aggregation, AlertSeverity, ChannelId, Comparison, Condition, Definition, DefinitionParts,
    Labels, LogSignal, MetricSignal, NoDataPolicy, Severity, Signal, TraceMeasure, TraceSignal,
};
use serde_json::{Map, Value, json};
use uuid::Uuid;

fn text(value: Option<&str>) -> Value {
    value.map_or(Value::Null, |held| Value::String(held.to_owned()))
}

fn texts<'a>(values: impl Iterator<Item = &'a str>) -> Value {
    Value::Array(values.map(|held| Value::String(held.to_owned())).collect())
}

fn object<'a>(pairs: impl Iterator<Item = (&'a str, &'a str)>) -> Value {
    Value::Object(
        pairs
            .map(|(key, value)| (key.to_owned(), Value::String(value.to_owned())))
            .collect::<Map<_, _>>(),
    )
}

fn signal_of(signal: &Signal) -> Value {
    match signal {
        Signal::Traces(traces) => json!({
            "kind": "traces",
            "service": text(traces.service.as_deref()),
            "operation": text(traces.operation.as_deref()),
            "environment": text(traces.environment.as_deref()),
            "measure": traces.measure.as_str(),
        }),
        Signal::Logs(logs) => json!({
            "kind": "logs",
            "services": texts(logs.services.iter().map(|service| &**service)),
            "severities": texts(logs.severities.iter().map(|severity| severity.name())),
            "q": text(logs.text.as_deref()),
            "attrs": Value::Array(
                logs.attributes
                    .iter()
                    .map(|(key, value)| Value::String(format!("{key}={value}")))
                    .collect()
            ),
            "measure": "count",
        }),
        Signal::Metrics(metric) => json!({
            "kind": "metrics",
            "name": &*metric.name,
            "aggregation": metric.aggregation.name(),
            "group_by": texts(metric.group_by.iter().map(|key| &**key)),
            "filter": object(metric.filter.iter().map(|(key, value)| (&**key, &**value))),
        }),
    }
}

pub(crate) fn encode(definition: &Definition) -> String {
    json!({
        "signal": signal_of(definition.signal()),
        "condition": {
            "op": definition.condition().op().as_str(),
            "threshold": definition.condition().threshold(),
        },
        "window_s": definition.window_secs(),
        "pending_s": definition.pending_secs(),
        "severity": definition.severity().as_str(),
        "no_data": definition.no_data().as_str(),
        "channels": Value::Array(
            definition
                .channels()
                .iter()
                .map(|channel| Value::String(channel.to_string()))
                .collect()
        ),
    })
    .to_string()
}

fn string(value: &Value, key: &str) -> Option<Box<str>> {
    value.get(key)?.as_str().map(Into::into)
}

fn strings(value: &Value, key: &str) -> Option<Vec<Box<str>>> {
    value
        .get(key)?
        .as_array()?
        .iter()
        .map(|held| held.as_str().map(Into::into))
        .collect()
}

fn decode_signal(value: &Value) -> Option<Signal> {
    match value.get("kind")?.as_str()? {
        "traces" => Some(Signal::Traces(TraceSignal {
            service: string(value, "service"),
            operation: string(value, "operation"),
            environment: string(value, "environment"),
            measure: TraceMeasure::parse(value.get("measure")?.as_str()?)?,
        })),
        "logs" => Some(Signal::Logs(LogSignal {
            services: strings(value, "services")?,
            severities: strings(value, "severities")?
                .iter()
                .map(|severity| Severity::try_from(&**severity).ok())
                .collect::<Option<Vec<_>>>()?,
            text: string(value, "q"),
            attributes: strings(value, "attrs")?
                .iter()
                .map(|pair| {
                    pair.split_once('=')
                        .map(|(key, held)| (key.into(), held.into()))
                })
                .collect::<Option<Vec<_>>>()?,
        })),
        "metrics" => Some(Signal::Metrics(MetricSignal {
            name: string(value, "name")?,
            aggregation: Aggregation::try_from(value.get("aggregation")?.as_str()?).ok()?,
            group_by: strings(value, "group_by")?,
            filter: decode_labels(value.get("filter")?)?.pairs().clone(),
        })),
        _ => None,
    }
}

pub(crate) fn decode(text: &str) -> Option<Definition> {
    let value: Value = serde_json::from_str(text).ok()?;
    let condition = value.get("condition")?;
    Definition::new(DefinitionParts {
        signal: decode_signal(value.get("signal")?)?,
        condition: Condition::new(
            Comparison::parse(condition.get("op")?.as_str()?)?,
            condition.get("threshold")?.as_f64()?,
        )
        .ok()?,
        window_secs: value.get("window_s")?.as_u64()?,
        pending_secs: value.get("pending_s")?.as_u64()?,
        severity: AlertSeverity::parse(value.get("severity")?.as_str()?)?,
        no_data: NoDataPolicy::parse(value.get("no_data")?.as_str()?)?,
        channels: strings(&value, "channels")?
            .iter()
            .map(|channel| Uuid::try_parse(channel).ok().map(ChannelId::new))
            .collect::<Option<Vec<_>>>()?,
    })
    .ok()
}

pub(crate) fn encode_labels(labels: &Labels) -> String {
    object(labels.pairs().iter().map(|(key, value)| (&**key, &**value))).to_string()
}

fn decode_labels(value: &Value) -> Option<Labels> {
    Some(Labels::new(
        value
            .as_object()?
            .iter()
            .map(|(key, held)| held.as_str().map(|held| (key.as_str().into(), held.into())))
            .collect::<Option<BTreeMap<_, _>>>()?,
    ))
}

pub(crate) fn labels_of(text: &str) -> Option<Labels> {
    decode_labels(&serde_json::from_str(text).ok()?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferristrace_domain::{
        Aggregation, AlertSeverity, ChannelId, Comparison, Condition, Definition, DefinitionParts,
        Labels, LogSignal, MetricSignal, NoDataPolicy, Severity, Signal, TraceMeasure, TraceSignal,
    };
    use uuid::Uuid;

    use super::{decode, encode, encode_labels, labels_of};

    fn kept(signal: Signal) -> Definition {
        Definition::new(DefinitionParts {
            signal,
            condition: Condition::new(Comparison::AtMost, 2.5).expect("a threshold"),
            window_secs: 600,
            pending_secs: 60,
            severity: AlertSeverity::Info,
            no_data: NoDataPolicy::NoData,
            channels: vec![ChannelId::new(Uuid::from_u128(3))],
        })
        .expect("a definition")
    }

    #[test]
    fn every_signal_reads_back_from_its_json() {
        for signal in [
            Signal::Traces(TraceSignal {
                service: Some("checkout".into()),
                operation: Some("POST /pay".into()),
                environment: None,
                measure: TraceMeasure::P99,
            }),
            Signal::Logs(LogSignal {
                services: vec!["api".into()],
                severities: vec![Severity::Error, Severity::Fatal],
                text: Some("timeout".into()),
                attributes: vec![("region".into(), "eu=west".into())],
            }),
            Signal::Metrics(MetricSignal {
                name: "queue.depth".into(),
                aggregation: Aggregation::P95,
                group_by: vec!["queue".into()],
                filter: BTreeMap::from([("env".into(), "prod".into())]),
            }),
        ] {
            let definition = kept(signal);

            assert_eq!(decode(&encode(&definition)), Some(definition));
        }
    }

    #[test]
    fn labels_read_back_from_their_json() {
        let labels = Labels::new(BTreeMap::from([("queue".into(), "mail\"box".into())]));

        assert_eq!(labels_of(&encode_labels(&labels)), Some(labels));
        assert_eq!(labels_of("{}"), Some(Labels::none()));
    }
}
