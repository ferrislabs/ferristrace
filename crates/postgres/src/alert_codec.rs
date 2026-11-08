use std::collections::BTreeMap;

use ferristrace_domain::{
    Aggregation, AlertSeverity, ChannelId, Comparison, Condition, CulpritPod, Culprits, Definition,
    DefinitionParts, Labels, LlmGroup, LlmMeasure, LlmSignal, LogSignal, Mentions, MetricSignal,
    NoDataPolicy, Placement, PlacementName, ProjectId, ScheduleId, Severity, Signal, TraceMeasure,
    TraceSignal, Workload,
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

fn placement_of(placement: &Placement) -> Value {
    let mut held = Map::new();
    let named = [
        (
            "cluster",
            placement.cluster.as_ref().map(ToString::to_string),
        ),
        (
            "namespace",
            placement.namespace.as_ref().map(ToString::to_string),
        ),
        (
            "workload",
            placement.workload.as_ref().map(ToString::to_string),
        ),
        ("pod", placement.pod.as_ref().map(ToString::to_string)),
        ("node", placement.node.as_ref().map(ToString::to_string)),
    ];
    for (key, value) in named {
        if let Some(value) = value {
            held.insert(key.to_owned(), Value::String(value));
        }
    }
    Value::Object(held)
}

fn decode_placement(value: &Value) -> Option<Placement> {
    let Some(held) = value.get("placement").filter(|held| !held.is_null()) else {
        return Some(Placement::default());
    };
    let name = |key: &str| -> Option<Option<PlacementName>> {
        match held.get(key) {
            None | Some(Value::Null) => Some(None),
            Some(text) => PlacementName::try_from(text.as_str()?).ok().map(Some),
        }
    };
    let workload = match held.get("workload") {
        None | Some(Value::Null) => None,
        Some(text) => Some(Workload::try_from(text.as_str()?).ok()?),
    };
    Some(Placement {
        cluster: name("cluster")?,
        namespace: name("namespace")?,
        workload,
        pod: name("pod")?,
        node: name("node")?,
    })
}

fn signal_of(signal: &Signal) -> Value {
    match signal {
        Signal::Traces(traces) => json!({
            "kind": "traces",
            "service": text(traces.service.as_deref()),
            "operation": text(traces.operation.as_deref()),
            "environment": text(traces.environment.as_deref()),
            "measure": traces.measure.as_str(),
            "placement": placement_of(&traces.placement),
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
            "placement": placement_of(&logs.placement),
        }),
        Signal::Metrics(metric) => json!({
            "kind": "metrics",
            "name": &*metric.name,
            "aggregation": metric.aggregation.name(),
            "group_by": texts(metric.group_by.iter().map(|key| &**key)),
            "filter": object(metric.filter.iter().map(|(key, value)| (&**key, &**value))),
        }),
        Signal::Llm(llm) => json!({
            "kind": "llm",
            "measure": llm.measure.as_str(),
            "group_by": llm.group_by.as_str(),
            "filter": {
                "feature": text(llm.feature.as_deref()),
                "customer": text(llm.customer.as_deref()),
                "model": text(llm.model.as_deref()),
                "project": llm.project.map_or(Value::Null, |project| Value::String(project.to_string())),
            },
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
        "mentions": {
            "roles": definition.mentions().roles(),
            "users": definition.mentions().users(),
        },
        "page": definition
            .page()
            .map_or(Value::Null, |schedule| json!({"schedule": schedule.to_string()})),
    })
    .to_string()
}

struct Unreadable;

pub(crate) fn mentions_of(held: Option<&Value>) -> Option<Mentions> {
    let Some(held) = held.filter(|held| !held.is_null()) else {
        return Some(Mentions::default());
    };
    let ids = |key: &str| -> Option<Vec<String>> {
        match held.get(key) {
            None | Some(Value::Null) => Some(Vec::new()),
            Some(list) => list
                .as_array()?
                .iter()
                .map(|one| one.as_str().map(str::to_owned))
                .collect(),
        }
    };
    Mentions::new(&ids("roles")?, &ids("users")?).ok()
}

fn decode_page(value: &Value) -> Result<Option<ScheduleId>, Unreadable> {
    match value.get("page") {
        None | Some(Value::Null) => Ok(None),
        Some(page) => page
            .get("schedule")
            .and_then(Value::as_str)
            .and_then(|held| Uuid::try_parse(held).ok())
            .map(|id| Some(ScheduleId::new(id)))
            .ok_or(Unreadable),
    }
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
            placement: decode_placement(value)?,
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
            placement: decode_placement(value)?,
        })),
        "metrics" => Some(Signal::Metrics(MetricSignal {
            name: string(value, "name")?,
            aggregation: Aggregation::try_from(value.get("aggregation")?.as_str()?).ok()?,
            group_by: strings(value, "group_by")?,
            filter: decode_labels(value.get("filter")?)?.pairs().clone(),
        })),
        "llm" => decode_llm(value),
        _ => None,
    }
}

fn decode_llm(value: &Value) -> Option<Signal> {
    let filter = value.get("filter").filter(|held| !held.is_null());
    let named = |key: &str| -> Option<Option<Box<str>>> {
        match filter.and_then(|held| held.get(key)) {
            None | Some(Value::Null) => Some(None),
            Some(held) => held.as_str().map(|text| Some(text.into())),
        }
    };
    let project = match filter.and_then(|held| held.get("project")) {
        None | Some(Value::Null) => None,
        Some(held) => Some(ProjectId::new(Uuid::try_parse(held.as_str()?).ok()?)),
    };
    Some(Signal::Llm(LlmSignal {
        measure: LlmMeasure::parse(value.get("measure")?.as_str()?)?,
        group_by: LlmGroup::parse(value.get("group_by")?.as_str()?)?,
        feature: named("feature")?,
        customer: named("customer")?,
        model: named("model")?,
        project,
    }))
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
        page: decode_page(&value).ok()?,
        mentions: mentions_of(value.get("mentions"))?,
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

pub(crate) fn culprits_value(culprits: &Culprits) -> Value {
    json!({
        "pods": Value::Array(
            culprits
                .pods()
                .iter()
                .map(|pod| json!({
                    "name": &*pod.name,
                    "node": text(pod.node.as_deref()),
                    "count": pod.count,
                }))
                .collect()
        ),
        "others": culprits.others(),
        "nodes": texts(culprits.nodes().iter().map(|node| &**node)),
        "versions": texts(culprits.versions().iter().map(|version| &**version)),
        "labels": object(culprits.labels().pairs().iter().map(|(key, value)| (&**key, &**value))),
    })
}

pub(crate) fn encode_culprits(culprits: &Culprits) -> String {
    culprits_value(culprits).to_string()
}

pub(crate) fn culprits_from(value: &Value) -> Option<Culprits> {
    let pods = value
        .get("pods")?
        .as_array()?
        .iter()
        .map(|pod| {
            Some(CulpritPod {
                name: string(pod, "name")?,
                node: string(pod, "node"),
                count: pod.get("count")?.as_u64()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let others = value.get("others")?.as_u64()?;
    let named = u64::try_from(pods.len()).ok()?;
    Some(Culprits::new(
        pods,
        named.saturating_add(others),
        strings(value, "nodes")?,
        strings(value, "versions")?,
        &decode_labels(value.get("labels")?)?,
    ))
}

pub(crate) fn culprits_of(text: &str) -> Option<Culprits> {
    culprits_from(&serde_json::from_str(text).ok()?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferristrace_domain::{
        Aggregation, AlertSeverity, ChannelId, Comparison, Condition, Definition, DefinitionParts,
        Labels, LlmGroup, LlmMeasure, LlmSignal, LogSignal, Mentions, MetricSignal, NoDataPolicy,
        ProjectId, ScheduleId, Severity, Signal, TraceMeasure, TraceSignal,
    };
    use uuid::Uuid;

    use ferristrace_domain::{Placement, PlacementName, Workload};

    use super::{decode, encode, encode_labels, labels_of};

    fn placed() -> Placement {
        Placement {
            cluster: Some(PlacementName::try_from("prod").expect("a cluster")),
            namespace: Some(PlacementName::try_from("shop").expect("a namespace")),
            workload: Some(Workload::try_from("deployment:checkout").expect("a workload")),
            pod: Some(PlacementName::try_from("checkout-1").expect("a pod")),
            node: None,
        }
    }

    #[test]
    fn a_signal_kept_before_placements_existed_reads_every_placement() {
        for signal in [
            r#"{"kind":"traces","service":"checkout","operation":null,"environment":null,"measure":"count"}"#,
            r#"{"kind":"logs","services":[],"severities":["error"],"q":null,"attrs":[],"measure":"count"}"#,
        ] {
            let stored = format!(
                r#"{{"signal":{signal},"condition":{{"op":">","threshold":1.0}},"window_s":300,"pending_s":0,"severity":"warning","no_data":"ok","channels":[]}}"#
            );

            let read = decode(&stored).expect("an old definition reads");

            match read.signal() {
                Signal::Traces(traces) => assert!(traces.placement.is_empty()),
                Signal::Logs(logs) => assert!(logs.placement.is_empty()),
                Signal::Metrics(_) | Signal::Llm(_) => {
                    panic!("a metric or llm signal from a trace or log one")
                }
            }
        }
    }

    #[test]
    fn a_placement_kept_in_a_form_no_longer_valid_leaves_the_definition_unreadable() {
        let stored = r#"{"signal":{"kind":"traces","measure":"count","placement":{"pod":"a b"}},"condition":{"op":">","threshold":1.0},"window_s":300,"pending_s":0,"severity":"warning","no_data":"ok","channels":[]}"#;

        assert_eq!(decode(stored), None);
    }

    fn kept(signal: Signal) -> Definition {
        Definition::new(DefinitionParts {
            signal,
            condition: Condition::new(Comparison::AtMost, 2.5).expect("a threshold"),
            window_secs: 600,
            pending_secs: 60,
            severity: AlertSeverity::Info,
            no_data: NoDataPolicy::NoData,
            channels: vec![ChannelId::new(Uuid::from_u128(3))],
            page: Some(ScheduleId::new(Uuid::from_u128(4))),
            mentions: Mentions::new(
                &["123456789012345678".to_owned()],
                &["223456789012345678".to_owned()],
            )
            .expect("ids"),
        })
        .expect("a definition")
    }

    #[test]
    fn a_definition_kept_before_paging_existed_pages_nobody() {
        let mut stored: serde_json::Value =
            serde_json::from_str(&encode(&kept(Signal::Traces(TraceSignal {
                service: None,
                operation: None,
                environment: None,
                measure: TraceMeasure::Count,
                placement: Placement::default(),
            }))))
            .expect("json");
        stored.as_object_mut().expect("an object").remove("page");

        let read = decode(&stored.to_string()).expect("it reads");

        assert_eq!(read.page(), None);
        assert!(encode(&read).contains("\"page\":null"));
    }

    #[test]
    fn every_signal_reads_back_from_its_json() {
        for signal in [
            Signal::Traces(TraceSignal {
                service: Some("checkout".into()),
                operation: Some("POST /pay".into()),
                environment: None,
                measure: TraceMeasure::P99,
                placement: placed(),
            }),
            Signal::Logs(LogSignal {
                services: vec!["api".into()],
                severities: vec![Severity::Error, Severity::Fatal],
                text: Some("timeout".into()),
                attributes: vec![("region".into(), "eu=west".into())],
                placement: Placement {
                    node: Some(PlacementName::try_from("node-a").expect("a node")),
                    ..Placement::default()
                },
            }),
            Signal::Metrics(MetricSignal {
                name: "queue.depth".into(),
                aggregation: Aggregation::P95,
                group_by: vec!["queue".into()],
                filter: BTreeMap::from([("env".into(), "prod".into())]),
            }),
            Signal::Llm(LlmSignal {
                measure: LlmMeasure::P95,
                group_by: LlmGroup::Customer,
                feature: Some("ticket-summary".into()),
                customer: None,
                model: Some("gpt-4o".into()),
                project: Some(ProjectId::new(Uuid::from_u128(8))),
            }),
            Signal::Llm(LlmSignal {
                measure: LlmMeasure::Cost,
                group_by: LlmGroup::None,
                feature: None,
                customer: None,
                model: None,
                project: None,
            }),
        ] {
            let definition = kept(signal);

            assert_eq!(decode(&encode(&definition)), Some(definition));
        }
    }

    #[test]
    fn an_llm_signal_with_a_bad_measure_group_or_filter_leaves_the_definition_unreadable() {
        for signal in [
            r#"{"kind":"llm","measure":"spend","group_by":"none","filter":{}}"#,
            r#"{"kind":"llm","measure":"cost","group_by":"user","filter":{}}"#,
            r#"{"kind":"llm","measure":"cost","group_by":"none","filter":{"project":"nope"}}"#,
            r#"{"kind":"llm","measure":"cost","group_by":"none","filter":{"feature":3}}"#,
        ] {
            let stored = format!(
                r#"{{"signal":{signal},"condition":{{"op":">","threshold":1.0}},"window_s":300,"pending_s":0,"severity":"warning","no_data":"ok","channels":[]}}"#
            );

            assert_eq!(decode(&stored), None, "{signal}");
        }
    }

    #[test]
    fn an_llm_signal_without_a_filter_reads_as_unfiltered() {
        let stored = r#"{"signal":{"kind":"llm","measure":"calls","group_by":"model"},"condition":{"op":">","threshold":1.0},"window_s":300,"pending_s":0,"severity":"warning","no_data":"ok","channels":[]}"#;

        let read = decode(stored).expect("it reads");

        assert!(matches!(
            read.signal(),
            Signal::Llm(llm) if llm.feature.is_none() && llm.project.is_none()
        ));
    }

    #[test]
    fn labels_read_back_from_their_json() {
        let labels = Labels::new(BTreeMap::from([("queue".into(), "mail\"box".into())]));

        assert_eq!(labels_of(&encode_labels(&labels)), Some(labels));
        assert_eq!(labels_of("{}"), Some(Labels::none()));
    }
}
