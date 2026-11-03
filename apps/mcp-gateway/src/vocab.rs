use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::clock::LocalTime;
use crate::pip::{Incident, OnCall, Placement};
use crate::reason::Reason;
use crate::verifier::Principal;

pub const SERVER: &str = "ferristrace";
pub const ACTION: &str = "call_tool";

const NAMED: [(&str, &[(&str, &str)]); 7] = [
    ("service_health", &[("service", "service")]),
    ("search_traces", &[("service", "service")]),
    (
        "search_logs",
        &[
            ("service", "service"),
            ("severity", "log_severity"),
            ("namespace", "namespace"),
            ("pod", "pod"),
        ],
    ),
    ("query_metric", &[("service", "service")]),
    (
        "cluster_status",
        &[("cluster", "cluster"), ("namespace", "namespace")],
    ),
    ("list_alerts", &[("state", "alert_state")]),
    ("list_incidents", &[("status", "incident_status")]),
];

const INCIDENT_TOOLS: [&str; 3] = ["get_incident", "comment_incident", "resolve_incident"];
const MAX_NAME_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nature {
    pub read_only: bool,
    pub destructive: bool,
}

impl Nature {
    pub fn from_annotations(annotations: Option<&Value>) -> Self {
        let hint = |key: &str| {
            annotations
                .and_then(|annotations| annotations.get(key))
                .and_then(Value::as_bool)
        };
        let read_only = hint("readOnlyHint").unwrap_or(false);
        let destructive = !read_only && hint("destructiveHint").unwrap_or(true);
        Self {
            read_only,
            destructive,
        }
    }

    pub fn access(self) -> &'static str {
        if self.read_only { "read" } else { "write" }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub target: Map<String, Value>,
    pub incident: Option<Uuid>,
    pub service: Option<String>,
}

fn named(arguments: &Map<String, Value>, key: &str) -> Option<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn lookup_safe(name: &str) -> bool {
    name.chars().count() <= MAX_NAME_CHARS
        && name != "."
        && name != ".."
        && !name.chars().any(char::is_control)
}

pub fn plan(tool: &str, arguments: &Map<String, Value>) -> Result<Plan, Reason> {
    let mut target = Map::new();
    if let Some((_, pairs)) = NAMED.iter().find(|(name, _)| *name == tool) {
        for (argument, key) in *pairs {
            if let Some(value) = named(arguments, argument) {
                target.insert((*key).to_owned(), Value::String(value));
            }
        }
    }
    let service = named(arguments, "service").filter(|_| target.contains_key("service"));
    if service
        .as_deref()
        .is_some_and(|service| !lookup_safe(service))
    {
        return Err(Reason::InvalidTarget);
    }
    let incident = INCIDENT_TOOLS
        .contains(&tool)
        .then(|| named(arguments, "incident_id"))
        .flatten()
        .and_then(|id| Uuid::parse_str(&id).ok());
    Ok(Plan {
        target,
        incident,
        service,
    })
}

pub fn apply_incident(target: &mut Map<String, Value>, incident: &Incident) {
    target.insert("severity".to_owned(), json!(incident.severity));
    target.insert("incident_status".to_owned(), json!(incident.status));
    if !incident.services.is_empty() {
        target.insert("services".to_owned(), json!(incident.services));
    }
    if let [only] = incident.services.as_slice() {
        target
            .entry("service")
            .or_insert_with(|| Value::String(only.clone()));
    }
}

pub fn apply_placement(target: &mut Map<String, Value>, placement: &Placement) {
    if let Some(project) = &placement.project {
        target.insert("project".to_owned(), json!(project));
    }
    if !placement.environments.is_empty() {
        target.insert("environments".to_owned(), json!(placement.environments));
    }
    if let [only] = placement.environments.as_slice() {
        target.insert("environment".to_owned(), json!(only));
    }
}

pub fn subject(principal: &Principal, oncall: Option<&OnCall>) -> Value {
    match (&principal.sub, principal.delegated) {
        (Some(sub), true) => {
            let mut properties = Map::new();
            if let Some(email) = &principal.email {
                properties.insert("email".to_owned(), json!(email));
            }
            properties.insert("groups".to_owned(), json!(principal.groups));
            properties.insert("roles".to_owned(), json!(principal.roles));
            if let Some(oncall) = oncall {
                properties.insert("on_call".to_owned(), json!(oncall.on_call));
                properties.insert("on_call_schedules".to_owned(), json!(oncall.schedules));
            }
            json!({ "type": "User", "id": sub, "properties": properties })
        }
        _ => json!({ "type": "Client", "id": principal.client_id }),
    }
}

pub fn subject_kind(principal: &Principal) -> (&'static str, &str) {
    match (&principal.sub, principal.delegated) {
        (Some(sub), true) => ("User", sub),
        _ => ("Client", &principal.client_id),
    }
}

pub fn resource(organisation: Uuid, tool: &str, nature: Nature) -> Value {
    json!({
        "type": "Tool",
        "id": format!("{SERVER}/{tool}"),
        "properties": {
            "tool": tool,
            "organisation": organisation.to_string(),
            "access": nature.access(),
            "destructive": nature.destructive,
        },
    })
}

pub fn primitives(arguments: &Map<String, Value>) -> Map<String, Value> {
    arguments
        .iter()
        .filter(|(_, value)| matches!(value, Value::String(_) | Value::Number(_) | Value::Bool(_)))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn context(
    principal: &Principal,
    arguments: &Map<String, Value>,
    target: &Map<String, Value>,
    time: &LocalTime,
) -> Value {
    let mut context = json!({
        "agent": principal.client_id,
        "delegated": principal.delegated,
        "target": target,
        "arguments": arguments,
        "time": {
            "weekday": time.weekday,
            "hour": time.hour,
            "minute": time.minute,
            "timezone": time.timezone,
            "iso_utc": time.iso_utc,
        },
    });
    if let Some(act) = &principal.act {
        context["act"] = act.clone();
    }
    context
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORG: Uuid = Uuid::from_u128(0x1111_1111_1111_4111_8111_1111_1111_1111);

    fn args(value: &Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    fn person() -> Principal {
        Principal {
            sub: Some("alice-sub".to_owned()),
            client_id: "claude-code".to_owned(),
            email: Some("alice@example.com".to_owned()),
            groups: vec!["Dev".to_owned()],
            roles: vec!["member".to_owned()],
            act: None,
            delegated: true,
            expires_at: 0,
        }
    }

    fn robot() -> Principal {
        Principal {
            sub: Some("service-account".to_owned()),
            delegated: false,
            email: None,
            groups: vec!["ignored".to_owned()],
            ..person()
        }
    }

    fn time() -> LocalTime {
        LocalTime {
            weekday: 2,
            hour: 10,
            minute: 5,
            timezone: "Europe/Paris".to_owned(),
            iso_utc: "2026-11-03T09:05:00Z".to_owned(),
        }
    }

    #[test]
    fn a_delegated_call_is_a_user_with_token_and_pip_properties() {
        let oncall = OnCall {
            on_call: true,
            schedules: vec!["primary".to_owned()],
        };
        assert_eq!(
            subject(&person(), Some(&oncall)),
            json!({
                "type": "User",
                "id": "alice-sub",
                "properties": {
                    "email": "alice@example.com",
                    "groups": ["Dev"],
                    "roles": ["member"],
                    "on_call": true,
                    "on_call_schedules": ["primary"],
                },
            })
        );
        assert_eq!(subject_kind(&person()), ("User", "alice-sub"));
    }

    #[test]
    fn without_pip_facts_the_on_call_properties_are_left_out() {
        let mut nameless = person();
        nameless.email = None;
        let subject = subject(&nameless, None);
        let properties = subject["properties"].as_object().unwrap();
        assert!(!properties.contains_key("email"));
        assert!(!properties.contains_key("on_call"));
        assert!(!properties.contains_key("on_call_schedules"));
        assert_eq!(properties["groups"], json!(["Dev"]));
        assert_eq!(properties["roles"], json!(["member"]));
    }

    #[test]
    fn an_autonomous_agent_is_a_client_with_no_properties() {
        assert_eq!(
            subject(&robot(), None),
            json!({"type": "Client", "id": "claude-code"})
        );
        assert_eq!(subject_kind(&robot()), ("Client", "claude-code"));
    }

    #[test]
    fn a_token_without_a_subject_is_never_a_user() {
        let mut nameless = person();
        nameless.sub = None;
        assert_eq!(subject(&nameless, None)["type"], "Client");
    }

    #[test]
    fn a_tool_resource_names_its_organisation_and_nature() {
        let write = Nature {
            read_only: false,
            destructive: true,
        };
        assert_eq!(
            resource(ORG, "resolve_incident", write),
            json!({
                "type": "Tool",
                "id": "ferristrace/resolve_incident",
                "properties": {
                    "tool": "resolve_incident",
                    "organisation": "11111111-1111-4111-8111-111111111111",
                    "access": "write",
                    "destructive": true,
                },
            })
        );
    }

    #[test]
    fn annotations_decide_read_write_and_destructive() {
        let read = Nature::from_annotations(Some(
            &json!({"readOnlyHint": true, "destructiveHint": true}),
        ));
        assert_eq!((read.access(), read.destructive), ("read", false));
        let comment = Nature::from_annotations(Some(
            &json!({"readOnlyHint": false, "destructiveHint": false}),
        ));
        assert_eq!((comment.access(), comment.destructive), ("write", false));
        let resolve = Nature::from_annotations(Some(
            &json!({"readOnlyHint": false, "destructiveHint": true}),
        ));
        assert_eq!((resolve.access(), resolve.destructive), ("write", true));
    }

    #[test]
    fn missing_annotations_are_a_destructive_write() {
        for annotations in [None, Some(json!({})), Some(json!({"readOnlyHint": "yes"}))] {
            let nature = Nature::from_annotations(annotations.as_ref());
            assert_eq!((nature.access(), nature.destructive), ("write", true));
        }
    }

    #[test]
    fn a_search_names_what_it_is_about() {
        let plan = plan(
            "search_logs",
            &args(&json!({"service": "checkout", "severity": "error", "namespace": "payments", "pod": "web-1", "text": "boom", "limit": 5})),
        )
        .unwrap();
        assert_eq!(
            Value::Object(plan.target),
            json!({"service": "checkout", "log_severity": "error", "namespace": "payments", "pod": "web-1"})
        );
        assert_eq!(plan.service.as_deref(), Some("checkout"));
        assert_eq!(plan.incident, None);
    }

    #[test]
    fn each_tool_maps_its_own_arguments() {
        let target = |tool: &str, arguments: Value| {
            Value::Object(plan(tool, &args(&arguments)).unwrap().target)
        };
        assert_eq!(
            target(
                "cluster_status",
                json!({"cluster": "prod-eu", "namespace": "payments"})
            ),
            json!({"cluster": "prod-eu", "namespace": "payments"})
        );
        assert_eq!(
            target("list_alerts", json!({"state": "firing"})),
            json!({"alert_state": "firing"})
        );
        assert_eq!(
            target("list_incidents", json!({"status": "open"})),
            json!({"incident_status": "open"})
        );
        assert_eq!(
            target("service_health", json!({"service": "api"})),
            json!({"service": "api"})
        );
        assert_eq!(
            target("search_traces", json!({"service": "api"})),
            json!({"service": "api"})
        );
        assert_eq!(
            target("query_metric", json!({"service": "api", "name": "x"})),
            json!({"service": "api"})
        );
        assert_eq!(target("list_services", json!({"window": "1h"})), json!({}));
        assert_eq!(
            target("who_is_on_call", json!({"schedule": "primary"})),
            json!({})
        );
        assert_eq!(target("get_trace", json!({"trace_id": "abc"})), json!({}));
    }

    #[test]
    fn a_target_the_call_does_not_name_is_left_out() {
        let plan = plan(
            "search_logs",
            &args(&json!({"text": "boom", "service": "", "pod": 4, "namespace": null})),
        )
        .unwrap();
        assert!(plan.target.is_empty());
        assert_eq!(plan.service, None);
    }

    #[test]
    fn an_argument_of_another_tool_is_not_a_target() {
        let plan = plan("get_trace", &args(&json!({"service": "checkout"}))).unwrap();
        assert!(plan.target.is_empty());
        assert_eq!(plan.service, None);
    }

    #[test]
    fn an_incident_tool_plans_an_incident_lookup_by_a_valid_id() {
        let id = "22222222-2222-4222-8222-222222222222";
        for tool in ["get_incident", "comment_incident", "resolve_incident"] {
            let plan = plan(tool, &args(&json!({"incident_id": id, "text": "x"}))).unwrap();
            assert_eq!(plan.incident, Some(Uuid::parse_str(id).unwrap()));
        }
        assert_eq!(
            plan("resolve_incident", &args(&json!({"incident_id": "../x"})))
                .unwrap()
                .incident,
            None
        );
        assert_eq!(
            plan("get_trace", &args(&json!({"incident_id": id})))
                .unwrap()
                .incident,
            None
        );
    }

    #[test]
    fn a_service_that_cannot_be_looked_up_safely_is_refused() {
        for bad in [".", "..", "a\nb", &"x".repeat(300)] {
            assert_eq!(
                plan("service_health", &args(&json!({"service": bad}))),
                Err(Reason::InvalidTarget),
                "{bad:?}"
            );
        }
        assert!(plan("service_health", &args(&json!({"service": "a/b c%"}))).is_ok());
    }

    #[test]
    fn an_incident_fills_severity_status_and_a_single_service() {
        let mut target = Map::new();
        apply_incident(
            &mut target,
            &Incident {
                severity: "sev1".to_owned(),
                status: "investigating".to_owned(),
                services: vec!["checkout".to_owned()],
            },
        );
        assert_eq!(
            Value::Object(target),
            json!({"severity": "sev1", "incident_status": "investigating", "services": ["checkout"], "service": "checkout"})
        );
    }

    #[test]
    fn an_incident_with_several_services_sets_no_single_service() {
        let mut target = Map::new();
        apply_incident(
            &mut target,
            &Incident {
                severity: "sev3".to_owned(),
                status: "open".to_owned(),
                services: vec!["a".to_owned(), "b".to_owned()],
            },
        );
        assert!(!target.contains_key("service"));
        assert_eq!(target["services"], json!(["a", "b"]));
        let mut none = Map::new();
        apply_incident(
            &mut none,
            &Incident {
                severity: "sev4".to_owned(),
                status: "open".to_owned(),
                services: vec![],
            },
        );
        assert!(!none.contains_key("services"));
        assert!(!none.contains_key("service"));
    }

    #[test]
    fn placement_fills_project_and_environment() {
        let mut target = Map::new();
        apply_placement(
            &mut target,
            &Placement {
                project: Some("shop".to_owned()),
                environments: vec!["staging".to_owned()],
            },
        );
        assert_eq!(
            Value::Object(target),
            json!({"project": "shop", "environments": ["staging"], "environment": "staging"})
        );
        let mut both = Map::new();
        apply_placement(
            &mut both,
            &Placement {
                project: None,
                environments: vec!["staging".to_owned(), "production".to_owned()],
            },
        );
        assert!(!both.contains_key("environment"));
        assert!(!both.contains_key("project"));
        assert_eq!(both["environments"], json!(["staging", "production"]));
        let mut empty = Map::new();
        apply_placement(
            &mut empty,
            &Placement {
                project: None,
                environments: vec![],
            },
        );
        assert!(empty.is_empty());
    }

    #[test]
    fn only_primitive_arguments_are_sent_as_they_came() {
        let arguments =
            args(&json!({"s": "x", "n": 3, "b": true, "o": {"k": 1}, "a": [1], "z": null}));
        assert_eq!(
            Value::Object(primitives(&arguments)),
            json!({"s": "x", "n": 3, "b": true})
        );
    }

    #[test]
    fn the_context_has_the_agent_target_arguments_and_local_time() {
        let mut target = Map::new();
        target.insert("service".to_owned(), json!("checkout"));
        let context = context(
            &person(),
            &args(&json!({"service": "checkout"})),
            &target,
            &time(),
        );
        assert_eq!(
            context,
            json!({
                "agent": "claude-code",
                "delegated": true,
                "target": {"service": "checkout"},
                "arguments": {"service": "checkout"},
                "time": {
                    "weekday": 2,
                    "hour": 10,
                    "minute": 5,
                    "timezone": "Europe/Paris",
                    "iso_utc": "2026-11-03T09:05:00Z",
                },
            })
        );
    }

    #[test]
    fn act_is_in_the_context_only_when_the_token_has_one() {
        let mut acted = person();
        acted.act = Some(json!({"client_id": "gateway"}));
        let with = context(&acted, &Map::new(), &Map::new(), &time());
        assert_eq!(with["act"], json!({"client_id": "gateway"}));
        let without = context(&person(), &Map::new(), &Map::new(), &time());
        assert!(without.get("act").is_none());
        assert_eq!(without["target"], json!({}));
        assert_eq!(without["arguments"], json!({}));
    }
}
