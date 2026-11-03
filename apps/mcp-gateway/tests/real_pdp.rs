mod support;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ferristrace_mcp::{ApiClient, router as mcp_router};
use ferristrace_mcp_gateway::config::{Config, ExchangeConfig, PdpConfig};
use ferristrace_mcp_gateway::{Gateway, Secret, router};
use reqwest::Method;
use rmcp::model::{CallToolRequestParams, ProtocolVersion};
use rmcp::service::{RoleClient, RunningService, ServiceError};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::{ClientHandler, ClientLifecycleMode, ClientServiceExt};
use serde_json::{Value, json};
use support::*;

const E2E_CLIENT: &str = "ftmcp-e2e";
const E2E_ORG: &str = "ftmcp-e2e";
const E2E_PASSWORD: &str = "Ftmcp-E2e-Pw-2026-xyz";
const E2E_PREFIX: &str = "ftmcp-";
const E2E_SCOPES: &str =
    "openid groups telemetry:read incidents:comment incidents:resolve oncall:acknowledge";
const GATEWAY_CLIENT: &str = "ferristrace-mcp-gateway";
const PEP_CLIENT: &str = "ferristrace-mcp-pep";
const AGENT_AUDIENCE: &str = "ferristrace-agents";

#[derive(Clone, Default)]
struct Probe;

impl ClientHandler for Probe {}

struct Target {
    url: String,
    realm: String,
    admin_user: String,
    admin_password: String,
}

fn target() -> Option<Target> {
    match std::env::var("FERRISKEY_URL") {
        Ok(url) if !url.is_empty() => Some(Target {
            url: url.trim_end_matches('/').to_owned(),
            realm: std::env::var("FERRISKEY_REALM").unwrap_or_else(|_| "ferristrace".to_owned()),
            admin_user: std::env::var("FERRISKEY_ADMIN_USERNAME")
                .unwrap_or_else(|_| "admin".to_owned()),
            admin_password: std::env::var("FERRISKEY_ADMIN_PASSWORD")
                .unwrap_or_else(|_| "admin".to_owned()),
        }),
        _ => {
            assert!(
                std::env::var("REQUIRE_FERRISKEY_URL").is_err(),
                "REQUIRE_FERRISKEY_URL is set but FERRISKEY_URL is not"
            );
            None
        }
    }
}

fn items(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(all) => all.clone(),
        Value::Object(map) => map
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_default()
}

struct Admin {
    http: reqwest::Client,
    base: String,
    realm: String,
    token: String,
}

impl Admin {
    async fn login(target: &Target) -> Self {
        let http = reqwest::Client::new();
        let answer: Value = http
            .post(format!(
                "{}/realms/master/protocol/openid-connect/token",
                target.url
            ))
            .form(&[
                ("grant_type", "password"),
                ("client_id", "admin-cli"),
                ("username", target.admin_user.as_str()),
                ("password", target.admin_password.as_str()),
            ])
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        Self {
            http,
            base: target.url.clone(),
            realm: target.realm.clone(),
            token: text(&answer, "access_token").to_owned(),
        }
    }

    async fn call(&self, method: Method, url: String, body: Option<&Value>) -> (u16, Value) {
        let mut request = self.http.request(method, url).bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        let text = response.text().await.unwrap();
        (status, serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    async fn realm_call(&self, method: Method, path: &str, body: Option<&Value>) -> (u16, Value) {
        self.call(
            method,
            format!("{}/realms/{}{path}", self.base, self.realm),
            body,
        )
        .await
    }

    async fn get(&self, path: &str) -> Value {
        let (status, body) = self.realm_call(Method::GET, path, None).await;
        assert_eq!(status, 200, "GET {path}: {body}");
        body
    }

    async fn post(&self, path: &str, body: &Value) -> Value {
        let (status, answer) = self.realm_call(Method::POST, path, Some(body)).await;
        assert!(
            (200..300).contains(&status),
            "POST {path} answered {status}: {answer}"
        );
        answer.get("data").cloned().unwrap_or(answer)
    }

    async fn client_uuid(&self, client_id: &str) -> Option<String> {
        let found = self.get(&format!("/clients?client_id={client_id}")).await;
        items(&found)
            .iter()
            .find(|client| text(client, "client_id") == client_id)
            .map(|client| text(client, "id").to_owned())
    }

    async fn client_secret(&self, uuid: &str) -> String {
        let answer = self.get(&format!("/clients/{uuid}/client-secret")).await;
        text(&answer, "client_secret").to_owned()
    }

    async fn scope_id(&self, name: &str) -> String {
        let found = self.get("/client-scopes?limit=100").await;
        items(&found)
            .iter()
            .find(|scope| text(scope, "name") == name)
            .map_or_else(
                || panic!("client scope {name} is missing: run ferriskey-init"),
                |scope| text(scope, "id").to_owned(),
            )
    }
}

struct Fixture {
    e2e_secret: String,
    gateway_secret: String,
    pep_secret: String,
    users: BTreeMap<String, String>,
}

async fn check_realm(admin: &Admin) {
    let state = admin.get("/authz/state").await;
    assert_eq!(
        state["enabled"], true,
        "authorization is off in the realm: run ferriskey-init"
    );
    let realm = admin
        .call(
            Method::GET,
            format!("{}/realms/{}", admin.base, admin.realm),
            None,
        )
        .await
        .1;
    let settings = realm.get("data").unwrap_or(&realm)["settings"].clone();
    let allowed = settings["allowed_resources"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        allowed.iter().any(|uri| uri == &json!(canonical(ORG))),
        "{} is not an allowed resource: run ferriskey-init with FERRISKEY_MCP_PUBLIC_URL={PUBLIC} and FERRISKEY_MCP_ORGS={ORG}",
        canonical(ORG)
    );
    assert_eq!(settings["dcr_enabled"], true);
    assert_eq!(settings["cimd_enabled"], true);
}

async fn ensure_e2e_client(admin: &Admin) -> String {
    let uuid = if let Some(uuid) = admin.client_uuid(E2E_CLIENT).await {
        uuid
    } else {
        let created = admin
            .post(
                "/clients",
                &json!({
                    "client_id": E2E_CLIENT, "name": "FerrisTrace MCP end to end",
                    "client_type": "confidential", "protocol": "openid-connect",
                    "enabled": true, "public_client": false,
                    "direct_access_grants_enabled": true,
                }),
            )
            .await;
        text(&created, "id").to_owned()
    };
    let held = admin.get(&format!("/clients/{uuid}/client-scopes")).await;
    let held: Vec<String> = items(&held)
        .iter()
        .map(|scope| text(scope, "name").to_owned())
        .collect();
    for scope in E2E_SCOPES.split(' ').filter(|scope| *scope != "openid") {
        if !held.iter().any(|name| name == scope) {
            let id = admin.scope_id(scope).await;
            let (status, body) = admin
                .realm_call(
                    Method::PUT,
                    &format!("/clients/{uuid}/optional-client-scopes/{id}"),
                    None,
                )
                .await;
            assert_eq!(status, 200, "{body}");
        }
    }
    uuid
}

async fn ensure_org_with_groups(admin: &Admin) -> (String, BTreeMap<&'static str, String>) {
    let orgs = items(&admin.get("/organizations").await);
    let org = if let Some(org) = orgs.iter().find(|org| text(org, "alias") == E2E_ORG) {
        text(org, "id").to_owned()
    } else {
        let created = admin
            .post(
                "/organizations",
                &json!({"name": "FerrisTrace MCP end to end", "alias": E2E_ORG, "enabled": true}),
            )
            .await;
        text(&created, "id").to_owned()
    };
    let groups = items(
        &admin
            .get(&format!("/organizations/{org}/groups?limit=100"))
            .await,
    );
    let mut ids = BTreeMap::new();
    for name in ["Dev", "Support", "Interns"] {
        let id = if let Some(group) = groups.iter().find(|group| text(group, "name") == name) {
            text(group, "id").to_owned()
        } else {
            let created = admin
                .post(
                    &format!("/organizations/{org}/groups"),
                    &json!({"name": name}),
                )
                .await;
            text(&created, "id").to_owned()
        };
        ids.insert(name, id);
    }
    (org, ids)
}

async fn ensure_user(admin: &Admin, name: &str) -> String {
    let found = items(&admin.get(&format!("/users?username={name}")).await);
    let id = if let Some(user) = found.iter().find(|user| text(user, "username") == name) {
        text(user, "id").to_owned()
    } else {
        let created = admin
            .post(
                "/users",
                &json!({
                    "username": name, "email": format!("{name}@example.com"),
                    "firstname": name, "lastname": "E2E", "email_verified": true,
                }),
            )
            .await;
        text(&created, "id").to_owned()
    };
    admin
        .realm_call(
            Method::PUT,
            &format!("/users/{id}/reset-password"),
            Some(
                &json!({"credential_type": "password", "temporary": false, "value": E2E_PASSWORD}),
            ),
        )
        .await;
    id
}

async fn provision(admin: &Admin, users: &Value) -> Fixture {
    check_realm(admin).await;
    let gateway_uuid = admin
        .client_uuid(GATEWAY_CLIENT)
        .await
        .expect("the gateway client is missing: run ferriskey-init");
    let pep_uuid = admin
        .client_uuid(PEP_CLIENT)
        .await
        .expect("the policy client is missing: run ferriskey-init");
    let gateway_secret = admin.client_secret(&gateway_uuid).await;
    let pep_secret = admin.client_secret(&pep_uuid).await;
    let e2e_secret = admin.client_secret(&ensure_e2e_client(admin).await).await;
    let (org, groups) = ensure_org_with_groups(admin).await;

    let mut ids = BTreeMap::new();
    for (name, facts) in users.as_object().unwrap() {
        let id = ensure_user(admin, name).await;
        admin
            .realm_call(
                Method::POST,
                &format!("/organizations/{org}/members"),
                Some(&json!({"user_id": id})),
            )
            .await;
        for group in facts["groups"].as_array().unwrap() {
            let group = &groups[group.as_str().unwrap()];
            admin
                .realm_call(
                    Method::POST,
                    &format!("/organizations/{org}/groups/{group}/members"),
                    Some(&json!({"user_id": id})),
                )
                .await;
        }
        ids.insert(name.clone(), id);
    }
    Fixture {
        e2e_secret,
        gateway_secret,
        pep_secret,
        users: ids,
    }
}

fn docs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/mcp/policies")
}

fn stem(file: &str) -> &str {
    file.strip_suffix(".cedar").unwrap_or(file)
}

async fn sync_policies(admin: &Admin, wanted: &[String]) {
    let sources: Vec<(String, String)> = wanted
        .iter()
        .map(|file| {
            (
                format!("{E2E_PREFIX}{}", stem(file)),
                fs::read_to_string(docs().join(file)).unwrap(),
            )
        })
        .collect();
    sync_sources(admin, &sources).await;
}

async fn sync_sources(admin: &Admin, wanted: &[(String, String)]) {
    let existing = items(&admin.get("/authz/policies").await);
    for policy in &existing {
        let id = text(policy, "id");
        if text(policy, "name").starts_with(E2E_PREFIX) {
            let (status, body) = admin
                .realm_call(Method::DELETE, &format!("/authz/policies/{id}"), None)
                .await;
            assert!((200..300).contains(&status), "delete {id}: {status} {body}");
        } else if policy["enabled"] == true {
            let (status, body) = admin
                .realm_call(
                    Method::PUT,
                    &format!("/authz/policies/{id}"),
                    Some(&json!({
                        "name": policy["name"], "description": policy["description"],
                        "enabled": false, "body": policy["body"],
                    })),
                )
                .await;
            assert_eq!(status, 200, "disable {id}: {body}");
        }
    }
    for (name, source) in wanted {
        admin
            .post(
                "/authz/policies?managed_by=ftmcp-e2e",
                &json!({
                    "name": name, "enabled": true,
                    "body": {"type": "static", "source": source},
                }),
            )
            .await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;
}

async fn restore_seed(admin: &Admin) {
    let existing = items(&admin.get("/authz/policies").await);
    for policy in &existing {
        let id = text(policy, "id");
        if text(policy, "name").starts_with(E2E_PREFIX) {
            admin
                .realm_call(Method::DELETE, &format!("/authz/policies/{id}"), None)
                .await;
        } else if text(policy, "managed_by") == "ferristrace-init" {
            admin
                .realm_call(
                    Method::PUT,
                    &format!("/authz/policies/{id}"),
                    Some(&json!({
                        "name": policy["name"], "description": policy["description"],
                        "enabled": true, "body": policy["body"],
                    })),
                )
                .await;
        }
    }
}

async fn user_token(target: &Target, fixture: &Fixture, user: &str) -> String {
    let answer: Value = reqwest::Client::new()
        .post(format!(
            "{}/realms/{}/protocol/openid-connect/token",
            target.url, target.realm
        ))
        .form(&[
            ("grant_type", "password"),
            ("client_id", E2E_CLIENT),
            ("client_secret", fixture.e2e_secret.as_str()),
            ("username", user),
            ("password", E2E_PASSWORD),
            ("scope", E2E_SCOPES),
            ("resource", canonical(ORG).as_str()),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        answer["access_token"].is_string(),
        "{user} could not sign in: {answer}"
    );
    text(&answer, "access_token").to_owned()
}

fn claims_of(token: &str) -> Value {
    let payload = token.split('.').nth(1).expect("a JWT");
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).unwrap()).unwrap()
}

fn config(target: &Target, fixture: &Fixture, upstream: &str, api: &str) -> Config {
    let defaults = Config::default();
    Config {
        public_url: PUBLIC.to_owned(),
        upstream: upstream.to_owned(),
        issuer: format!("{}/realms/{}", target.url, target.realm),
        api_url: api.to_owned(),
        agent_audience: AGENT_AUDIENCE.to_owned(),
        pdp: PdpConfig {
            url: target.url.clone(),
            realm: target.realm.clone(),
            client_id: PEP_CLIENT.to_owned(),
            client_secret: Secret::new(fixture.pep_secret.clone()),
            timeout_ms: 5000,
            ..defaults.pdp.clone()
        },
        exchange: ExchangeConfig {
            client_id: GATEWAY_CLIENT.to_owned(),
            client_secret: Secret::new(fixture.gateway_secret.clone()),
            timeout_ms: 5000,
            ..defaults.exchange.clone()
        },
        ..defaults
    }
}

struct Stack {
    url: String,
    api: Fake,
    decisions: Capture,
}

async fn stack(target: &Target, fixture: &Fixture, timezone: &str, case: &Value) -> Stack {
    let (api, api_url) = start_api().await;
    api.reply(
        "GET",
        &format!("/api/orgs/{ORG}/clock"),
        200,
        &json!({"timezone": timezone}),
    );
    let on_call = case["on_call"] == true;
    api.reply(
        "GET",
        &format!("/api/orgs/{ORG}/oncall/now"),
        200,
        &json!({"on_call": on_call, "schedules": if on_call { json!(["primary"]) } else { json!([]) }}),
    );
    if !case["incident"].is_null() {
        api.reply(
            "GET",
            &format!("/api/orgs/{ORG}/incidents/{INCIDENT}"),
            200,
            &case["incident"],
        );
    }
    if let (Some(service), false) = (
        case["arguments"]["service"].as_str(),
        case["placement"].is_null(),
    ) {
        api.reply(
            "GET",
            &format!("/api/orgs/{ORG}/services/{service}/placement"),
            200,
            &json!({"project": case["placement"]["project"], "environments": case["placement"]["environments"].as_array().cloned().unwrap_or_default()}),
        );
    }
    let upstream = serve(mcp_router(ApiClient::new(&api_url).unwrap())).await;
    let decisions = Capture::default();
    let gateway = Gateway::new(
        &config(target, fixture, &upstream, &api_url),
        clock_at(text(case, "at")),
        decisions.clone(),
    )
    .unwrap();
    let url = serve(router(Arc::new(gateway))).await;
    Stack {
        url,
        api,
        decisions,
    }
}

async fn client(stack: &Stack, token: &str) -> RunningService<RoleClient, Probe> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(format!("{}/orgs/{ORG}/mcp", stack.url))
            .auth_header(token),
    );
    Probe
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .unwrap()
}

enum Outcome {
    Permit,
    Deny(String),
}

async fn call(client: &RunningService<RoleClient, Probe>, case: &Value) -> Outcome {
    let arguments = case["arguments"].as_object().cloned().unwrap_or_default();
    match client
        .call_tool(
            CallToolRequestParams::new(text(case, "tool").to_owned()).with_arguments(arguments),
        )
        .await
    {
        Ok(_) => Outcome::Permit,
        Err(ServiceError::McpError(error)) => {
            assert_eq!(error.code.0, -32001, "{}: {error:?}", case["name"]);
            Outcome::Deny(
                error
                    .data
                    .as_ref()
                    .map_or_else(String::new, |data| text(data, "reason").to_owned()),
            )
        }
        Err(other) => panic!("{}: {other:?}", case["name"]),
    }
}

fn check_credentials(stack: &Stack, fixture: &Fixture, user: &str) {
    let seen = stack.api.log.all();
    let tool_call = seen
        .iter()
        .find(|seen| seen.path.ends_with("/logs") || seen.path.contains("/traces"))
        .expect("the tool reached FerrisTrace");
    let exchanged = claims_of(
        tool_call
            .header("authorization")
            .strip_prefix("Bearer ")
            .unwrap(),
    );
    assert_eq!(exchanged["aud"], json!([AGENT_AUDIENCE]));
    assert_eq!(exchanged["azp"], GATEWAY_CLIENT);
    assert_eq!(exchanged["sub"], fixture.users[user]);
    assert_eq!(exchanged["act"]["client_id"], GATEWAY_CLIENT);
    assert!(text(&exchanged, "scope").contains("telemetry:read"));
    let lookup = seen
        .iter()
        .find(|seen| seen.path.ends_with("/clock"))
        .expect("the gateway read the clock");
    let own = claims_of(
        lookup
            .header("authorization")
            .strip_prefix("Bearer ")
            .unwrap(),
    );
    assert!(
        own["aud"]
            .as_array()
            .unwrap()
            .iter()
            .any(|aud| aud == AGENT_AUDIENCE)
    );
    assert_eq!(own["azp"], GATEWAY_CLIENT);
    assert!(own.get("act").is_none());
    assert!(text(&own, "scope").contains("telemetry:read"));
    assert_ne!(
        tool_call.header("authorization"),
        lookup.header("authorization")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_worked_policies_decide_the_gateways_own_requests_in_ferriskeys_real_pdp() {
    let Some(target) = target() else { return };
    let cases: Value = serde_json::from_str(include_str!("policy_cases.json")).unwrap();
    let admin = Admin::login(&target).await;
    let fixture = provision(&admin, &cases["users"]).await;
    let timezone = text(&cases, "timezone").to_owned();

    let mut sets: BTreeMap<Vec<String>, Vec<&Value>> = BTreeMap::new();
    for case in cases["cases"].as_array().unwrap() {
        let files = case["policies"]
            .as_array()
            .unwrap()
            .iter()
            .map(text_of)
            .collect();
        sets.entry(files).or_default().push(case);
    }

    let mut checked_credentials = false;
    let mut report = Vec::new();
    for (files, set) in &sets {
        sync_policies(&admin, files).await;
        for case in set {
            let user = text(case, "user");
            let token = user_token(&target, &fixture, user).await;
            let stack = stack(&target, &fixture, &timezone, case).await;
            let client = client(&stack, &token).await;
            let outcome = call(&client, case).await;
            let expected = text(case, "today");
            let (got, reason) = match &outcome {
                Outcome::Permit => ("permit", String::new()),
                Outcome::Deny(reason) => ("deny", reason.clone()),
            };
            assert_eq!(
                got, expected,
                "{}: the real PDP answered {got} ({reason}), the guide expects {expected}",
                case["name"]
            );
            if got == "deny" {
                assert_eq!(reason, "no_permitting_policy", "{}", case["name"]);
            }
            let records = stack.decisions.all();
            let record = records.last().expect("a decision was recorded");
            assert_eq!(record.decision, got, "{}", case["name"]);
            assert_eq!(record.tool.as_deref(), Some(text(case, "tool")));
            assert_eq!(
                Value::Object(record.target.clone()),
                case["target"],
                "{}",
                case["name"]
            );
            assert_eq!(record.subject, fixture.users[user]);
            if !checked_credentials && got == "permit" && text(case, "tool") == "search_logs" {
                check_credentials(&stack, &fixture, user);
                checked_credentials = true;
            }
            let gap = if text(case, "expect") == expected {
                String::new()
            } else {
                format!(
                    " (guide: {}, FerrisKey drops the attribute)",
                    text(case, "expect")
                )
            };
            report.push(format!("{:<58} {got}{gap}", text(case, "name")));
        }
    }
    restore_seed(&admin).await;
    assert!(
        checked_credentials,
        "no permitted search_logs case checked the tokens"
    );
    println!("{}", report.join("\n"));
}

fn text_of(value: &Value) -> String {
    value.as_str().unwrap().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_list_shows_the_seeded_read_tools_and_hides_what_needs_a_target() {
    let Some(target) = target() else { return };
    let cases: Value = serde_json::from_str(include_str!("policy_cases.json")).unwrap();
    let admin = Admin::login(&target).await;
    let fixture = provision(&admin, &cases["users"]).await;
    let timezone = text(&cases, "timezone").to_owned();
    let probe = json!({"at": "2026-11-03T09:00:00Z", "on_call": false, "incident": null, "placement": null, "arguments": {}});

    sync_policies(&admin, &["seed-members-read.cedar".to_owned()]).await;
    let token = user_token(&target, &fixture, "alice").await;
    let stack_a = stack(&target, &fixture, &timezone, &probe).await;
    let listed = client(&stack_a, &token)
        .await
        .list_all_tools()
        .await
        .unwrap();
    let mut names: Vec<String> = listed.iter().map(|tool| tool.name.to_string()).collect();
    names.sort();
    let mut expected: Vec<String> = ferristrace_mcp::TOOL_NAMES
        .iter()
        .filter(|name| {
            !matches!(
                **name,
                "comment_incident" | "resolve_incident" | "acknowledge_page"
            )
        })
        .map(|name| (*name).to_owned())
        .collect();
    expected.sort();
    assert_eq!(names, expected);
    let records = stack_a.decisions.all();
    assert_eq!(records.len(), ferristrace_mcp::TOOL_NAMES.len());
    assert_eq!(
        records
            .iter()
            .filter(|record| record.decision == "permit")
            .count(),
        12
    );

    sync_policies(&admin, &["01-dev-reads-checkout.cedar".to_owned()]).await;
    let stack_b = stack(&target, &fixture, &timezone, &probe).await;
    let listed = client(&stack_b, &token)
        .await
        .list_all_tools()
        .await
        .unwrap();
    assert!(
        listed.is_empty(),
        "a rule that needs a service shows nothing in tools/list"
    );

    sync_policies(&admin, &[]).await;
    let stack_c = stack(&target, &fixture, &timezone, &probe).await;
    let listed = client(&stack_c, &token)
        .await
        .list_all_tools()
        .await
        .unwrap();
    assert!(listed.is_empty());
    restore_seed(&admin).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_realm_with_a_schema_validates_policies_and_cannot_hold_the_gateways_context() {
    let Some(target) = target() else { return };
    let admin = Admin::login(&target).await;
    let scratch = "ftmcp-validate";
    let master = |path: &str| format!("{}/realms{path}", target.url);
    admin
        .call(Method::DELETE, master(&format!("/{scratch}")), None)
        .await;
    let (status, _) = admin
        .call(
            Method::POST,
            master(""),
            Some(&json!({"name": scratch, "display_name": scratch})),
        )
        .await;
    assert_eq!(status, 201);
    let scratch_admin = Admin {
        http: admin.http.clone(),
        base: admin.base.clone(),
        realm: scratch.to_owned(),
        token: admin.token.clone(),
    };
    let (status, body) = scratch_admin
        .realm_call(Method::PUT, "/authz/state", Some(&json!({"enabled": true})))
        .await;
    assert_eq!(status, 200, "{body}");
    let (status, body) = scratch_admin
        .realm_call(
            Method::PUT,
            "/authz/schema",
            Some(&json!({"namespace": "FerrisTrace", "source": "namespace FerrisTrace {}"})),
        )
        .await;
    assert_eq!(status, 200, "{body}");

    let prefix = "permit (principal is FerrisKey::User, action == FerrisKey::Action::\"call_tool\", resource is FerrisKey::Tool) when { ";
    let attempt = |name: &str, condition: &str| {
        let body = json!({"name": name, "enabled": true, "body": {"type": "static", "source": format!("{prefix}{condition} }};")}});
        let scratch_admin = &scratch_admin;
        async move {
            scratch_admin
                .realm_call(Method::POST, "/authz/policies", Some(&body))
                .await
        }
    };
    let (status, body) = attempt("known-attribute", "resource.tool == \"search_logs\"").await;
    assert_eq!(status, 201, "{body}");
    let (status, body) = attempt("typo-on-the-tool", "resource.toool == \"search_logs\"").await;
    assert_eq!(status, 422, "a typo must be reported by FerrisKey: {body}");
    let (status, body) = attempt("on-call", "principal.on_call").await;
    assert_eq!(
        status, 422,
        "FerrisKey 0.10.1 builtin User has no on_call: {body}"
    );
    let (status, body) = attempt("guarded-typo", "principal has oncall && principal.oncall").await;
    assert_eq!(
        status, 201,
        "a typo behind a has guard is accepted, never matching: {body}"
    );
    let (status, body) = attempt("target", "context.target has service").await;
    assert_eq!(
        status, 422,
        "FerrisKey 0.10.1 builtin AgentContext has no target: {body}"
    );

    let (status, _) = admin
        .call(Method::DELETE, master(&format!("/{scratch}")), None)
        .await;
    assert!((200..300).contains(&status), "{status}");
}

fn probe_request() -> Value {
    json!({
        "subject": {"type": "User", "id": "probe-sub", "properties": {
            "email": "probe@example.com", "groups": ["Dev"], "roles": ["member"],
            "on_call": true, "on_call_schedules": ["primary"]}},
        "action": {"name": "call_tool"},
        "resource": {"type": "Tool", "id": "ferristrace/search_logs", "properties": {
            "tool": "search_logs", "organisation": ORG, "access": "read", "destructive": false}},
        "context": {"agent": "claude-code", "delegated": true,
            "target": {"service": "checkout"}, "arguments": {"service": "checkout"},
            "time": {"weekday": 2, "hour": 10, "minute": 0, "timezone": "Europe/Paris",
                "iso_utc": "2026-11-03T09:00:00Z"}},
    })
}

fn probes() -> [(&'static str, &'static str, bool); 11] {
    [
        (
            "principal.groups",
            "principal in FerrisKey::Group::\"Dev\"",
            true,
        ),
        (
            "principal.roles",
            "principal in FerrisKey::Role::\"member\"",
            true,
        ),
        (
            "principal.email",
            "principal has email && principal.email == \"probe@example.com\"",
            true,
        ),
        ("principal.on_call", "principal has on_call", false),
        (
            "principal.on_call_schedules",
            "principal has on_call_schedules",
            false,
        ),
        (
            "resource.tool",
            "resource.tool == \"search_logs\" && resource.server == \"ferristrace\"",
            true,
        ),
        ("resource.access", "resource has access", false),
        ("resource.organisation", "resource has organisation", false),
        (
            "context.target",
            "context.target has service && context.target.service == \"checkout\"",
            true,
        ),
        (
            "context.time",
            "context.time.weekday == 2 && context.time.hour == 10 && context.time.minute == 0",
            true,
        ),
        ("context.arguments", "context has arguments", false),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn ferriskey_forwards_these_attributes_of_the_gateways_request_to_cedar_and_drops_those() {
    let Some(target) = target() else { return };
    let cases: Value = serde_json::from_str(include_str!("policy_cases.json")).unwrap();
    let admin = Admin::login(&target).await;
    let fixture = provision(&admin, &cases["users"]).await;
    let http = reqwest::Client::new();
    let pep: Value = http
        .post(format!(
            "{}/realms/{}/protocol/openid-connect/token",
            target.url, target.realm
        ))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", PEP_CLIENT),
            ("client_secret", fixture.pep_secret.as_str()),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let request = probe_request();
    let probes = probes();
    let mut report = Vec::new();
    for (what, condition, forwarded) in probes {
        let source = format!(
            "permit (principal, action == FerrisKey::Action::\"call_tool\", resource is FerrisKey::Tool) when {{ {condition} }};"
        );
        sync_sources(&admin, &[(format!("{E2E_PREFIX}probe"), source)]).await;
        let answer: Value = http
            .post(format!(
                "{}/realms/{}/authzen/access/v1/evaluation",
                target.url, target.realm
            ))
            .bearer_auth(text(&pep, "access_token"))
            .json(&request)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(answer["decision"], forwarded, "{what}: {answer}");
        report.push(format!(
            "{what:<28} {}",
            if forwarded { "forwarded" } else { "dropped" }
        ));
    }
    restore_seed(&admin).await;
    println!("{}", report.join("\n"));
}
