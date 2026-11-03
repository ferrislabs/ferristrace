use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use cedar_policy::{
    Authorizer, Context, Decision, Entities, EntityUid, PolicySet, Request, Schema, SchemaFragment,
    ValidationMode, Validator,
};
use serde_json::{Map, Value, json};

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mcp")
}

fn schema_text() -> String {
    fs::read_to_string(docs().join("ferristrace.cedarschema")).unwrap()
}

fn fragment(text: &str) -> SchemaFragment {
    SchemaFragment::from_cedarschema_str(text).unwrap().0
}

fn is_record(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("Record")
}

fn copy_required(target: &mut Value, source: &Value) {
    if let Some(object) = target.as_object_mut() {
        match source.get("required") {
            Some(required) => {
                object.insert("required".to_owned(), required.clone());
            }
            None => {
                object.remove("required");
            }
        }
    }
}

fn merge(base: &mut Map<String, Value>, extension: &Map<String, Value>) {
    for (key, added) in extension {
        let Some(existing) = base.get_mut(key) else {
            base.insert(key.clone(), added.clone());
            continue;
        };
        if is_record(existing) && is_record(added) {
            let inner = added["attributes"].as_object().cloned().unwrap_or_default();
            if let Some(Value::Object(attributes)) = existing.get_mut("attributes") {
                merge(attributes, &inner);
            }
        }
        copy_required(existing, added);
    }
}

fn extension(realm: &Value, name: &str) -> Map<String, Value> {
    realm["FerrisTrace"]["commonTypes"][name]["attributes"]
        .as_object()
        .cloned()
        .unwrap_or_default()
}

fn schema() -> Schema {
    let builtin = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ferriskey-builtin.cedarschema"),
    )
    .unwrap();
    let realm = fragment(&schema_text());
    let declared = realm.clone().to_json_value().unwrap();
    let mut merged = fragment(&builtin).to_json_value().unwrap();
    let root = merged["FerrisKey"].as_object_mut().unwrap();
    for (name, entity) in [("UserExtension", "User"), ("ToolExtension", "Tool")] {
        let Some(Value::Object(base)) = root
            .get_mut("entityTypes")
            .and_then(|types| types.pointer_mut(&format!("/{entity}/shape/attributes")))
        else {
            panic!("{entity} has no attributes");
        };
        merge(base, &extension(&declared, name));
    }
    let Some(Value::Object(base)) = root
        .get_mut("commonTypes")
        .and_then(|types| types.pointer_mut("/AgentContext/attributes"))
    else {
        panic!("AgentContext has no attributes");
    };
    merge(base, &extension(&declared, "ContextExtension"));
    Schema::from_schema_fragments([SchemaFragment::from_json_value(merged).unwrap(), realm])
        .unwrap()
}

fn policy_text(file: &str) -> String {
    fs::read_to_string(docs().join("policies").join(file)).unwrap()
}

fn policy_files() -> Vec<String> {
    let mut files: Vec<String> = fs::read_dir(docs().join("policies"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "cedar")
        })
        .collect();
    files.sort();
    files
}

fn set_of(files: &[String]) -> PolicySet {
    let mut set = PolicySet::new();
    for file in files {
        let parsed = cedar_policy::Policy::parse(
            Some(cedar_policy::PolicyId::new(file.as_str())),
            policy_text(file),
        )
        .unwrap_or_else(|error| panic!("{file}: {error}"));
        set.add(parsed).unwrap();
    }
    set
}

fn validation_errors(source: &str) -> Vec<String> {
    let set = PolicySet::from_str(source).unwrap();
    let result = Validator::new(schema()).validate(&set, ValidationMode::Strict);
    result
        .validation_errors()
        .map(ToString::to_string)
        .collect()
}

const READ_TOOLS: [&str; 12] = [
    "list_services",
    "service_health",
    "search_traces",
    "get_trace",
    "search_logs",
    "query_metric",
    "cluster_status",
    "list_alerts",
    "alert_history",
    "list_incidents",
    "get_incident",
    "who_is_on_call",
];

fn decide(case: &Value, users: &Value) -> Decision {
    let user = case["user"].as_str().unwrap();
    let groups: Vec<&str> = users[user]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| group.as_str().unwrap())
        .collect();
    let tool = case["tool"].as_str().unwrap();
    let read = READ_TOOLS.contains(&tool);
    let user_uid = json!({"type": "FerrisKey::User", "id": format!("{user}-sub")});
    let tool_uid = json!({"type": "FerrisKey::Tool", "id": format!("ferristrace/{tool}")});
    let mut entities = vec![
        json!({
            "uid": user_uid,
            "attrs": {
                "username": user,
                "email_verified": true,
                "enabled": true,
                "roles": [],
                "on_call": case["on_call"],
                "on_call_schedules": [],
            },
            "parents": groups
                .iter()
                .map(|group| json!({"type": "FerrisKey::Group", "id": group}))
                .collect::<Vec<_>>(),
        }),
        json!({
            "uid": tool_uid,
            "attrs": {
                "server": "ferristrace",
                "tool": tool,
                "organisation": "11111111-1111-4111-8111-111111111111",
                "access": if read { "read" } else { "write" },
                "destructive": tool == "resolve_incident",
            },
            "parents": [{"type": "FerrisKey::McpServer", "id": "ferristrace"}],
        }),
    ];
    entities.extend(groups.iter().map(|group| {
        json!({
            "uid": {"type": "FerrisKey::Group", "id": group},
            "attrs": {"name": group},
            "parents": [],
        })
    }));
    let schema = schema();
    let entities = Entities::from_json_value(Value::Array(entities), Some(&schema)).unwrap();
    let time = &case["time"];
    let context = json!({
        "agent": "claude-code",
        "delegated": true,
        "target": case["target"],
        "arguments": case["arguments"],
        "time": {
            "weekday": time["weekday"],
            "hour": time["hour"],
            "minute": time["minute"],
            "timezone": "Europe/Paris",
            "iso_utc": case["at"],
        },
        "act": {"sub": "gateway-sub", "client_id": "ferristrace-mcp-gateway"},
    });
    let action = EntityUid::from_str("FerrisKey::Action::\"call_tool\"").unwrap();
    let request = Request::new(
        EntityUid::from_str(&format!("FerrisKey::User::\"{user}-sub\"")).unwrap(),
        action.clone(),
        EntityUid::from_str(&format!("FerrisKey::Tool::\"ferristrace/{tool}\"")).unwrap(),
        Context::from_json_value(context, Some((&schema, &action))).unwrap(),
        Some(&schema),
    )
    .unwrap();
    let files: Vec<String> = case["policies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file.as_str().unwrap().to_owned())
        .collect();
    let response = Authorizer::new().is_authorized(&request, &set_of(&files), &entities);
    assert_eq!(
        response.diagnostics().errors().count(),
        0,
        "{}: a policy errored",
        case["name"]
    );
    response.decision()
}

#[test]
fn the_vocabulary_schema_extends_the_ferriskey_one() {
    let text = schema_text();
    for needed in [
        "namespace FerrisTrace",
        "type UserExtension",
        "type ToolExtension",
        "type ContextExtension",
        "on_call?: Bool",
        "on_call_schedules?: Set<String>",
        "access?: String",
        "destructive?: Bool",
        "target: {",
        "arguments: {",
        "minute: Long",
    ] {
        assert!(text.contains(needed), "{needed}");
    }
    let _ = schema();
}

#[test]
fn the_vocabulary_names_every_target_key_of_the_spec() {
    let text = schema_text();
    for key in [
        "service",
        "services",
        "namespace",
        "cluster",
        "pod",
        "environment",
        "environments",
        "project",
        "severity",
        "incident_status",
        "alert_state",
        "log_severity",
    ] {
        assert!(text.contains(&format!("{key}?:")), "{key}");
    }
}

#[test]
fn every_policy_of_the_guide_validates_against_the_schema() {
    let files = policy_files();
    assert_eq!(files.len(), 9, "{files:?}");
    for file in files {
        let errors = validation_errors(&policy_text(&file));
        assert!(errors.is_empty(), "{file}: {errors:?}");
    }
}

#[test]
fn a_policy_testing_an_attribute_the_vocabulary_lacks_is_reported() {
    for body in [
        "resource.tool == \"x\" && principal.oncall",
        "context.target.servce == \"checkout\"",
        "context.time.hours > 3",
        "resource.access_level == \"read\"",
    ] {
        let source = format!(
            "permit (principal is FerrisKey::User, action == FerrisKey::Action::\"call_tool\", resource is FerrisKey::Tool) when {{ {body} }};"
        );
        let errors = validation_errors(&source);
        assert!(!errors.is_empty(), "{body} was accepted");
    }
}

#[test]
fn a_policy_that_guards_an_optional_attribute_validates_and_one_that_does_not_is_reported() {
    let guarded = "permit (principal is FerrisKey::User, action == FerrisKey::Action::\"call_tool\", resource is FerrisKey::Tool) when { context.target has service && context.target.service == \"checkout\" };";
    assert!(validation_errors(guarded).is_empty());
    let bare = "permit (principal is FerrisKey::User, action == FerrisKey::Action::\"call_tool\", resource is FerrisKey::Tool) when { context.target.service == \"checkout\" };";
    assert!(!validation_errors(bare).is_empty());
}

#[test]
fn the_worked_examples_permit_and_refuse_exactly_the_calls_they_describe() {
    let cases: Value =
        serde_json::from_str(include_str!("policy_cases.json")).expect("policy_cases.json");
    let users = &cases["users"];
    let mut covered = BTreeSet::new();
    for case in cases["cases"].as_array().unwrap() {
        let expected = match case["expect"].as_str().unwrap() {
            "permit" => Decision::Allow,
            _ => Decision::Deny,
        };
        assert_eq!(decide(case, users), expected, "{}", case["name"]);
        for file in case["policies"].as_array().unwrap() {
            covered.insert(file.as_str().unwrap().to_owned());
        }
    }
    let every: BTreeSet<String> = policy_files()
        .into_iter()
        .filter(|file| file != "seed-members-read-by-name.cedar")
        .collect();
    assert_eq!(covered, every);
}

#[test]
fn the_guide_shows_every_policy_exactly_as_it_is_tested() {
    let guide = fs::read_to_string(docs().join("../mcp.md")).unwrap();
    for file in policy_files() {
        if file.starts_with("seed-") {
            continue;
        }
        let source = policy_text(&file);
        assert!(
            guide.contains(source.trim_end()),
            "docs/mcp.md does not show {file} as it is"
        );
    }
}

#[test]
fn a_build_that_drops_attributes_never_permits_what_the_guide_refuses() {
    let cases: Value = serde_json::from_str(include_str!("policy_cases.json")).unwrap();
    for case in cases["cases"].as_array().unwrap() {
        if case["today"] != case["expect"] {
            assert_eq!(case["today"], "deny", "{} fails open", case["name"]);
        }
    }
}
