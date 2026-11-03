#![allow(clippy::too_many_lines, clippy::semicolon_if_nothing_returned)]

mod support;

use axum::http::StatusCode;
use serde_json::{Value, json};
use support::*;

fn names(answer: &Value) -> Vec<String> {
    answer["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap().to_owned())
        .collect()
}

fn reason(answer: &Value) -> &str {
    answer["error"]["data"]["reason"].as_str().unwrap_or("")
}

async fn world_with(pdp: Pdp) -> World {
    let world = World::start().await;
    world.realm.set_pdp(pdp);
    world
}

async fn warm(world: &World, token: &str) {
    world.rpc(token, "tools/list", json!({})).await;
    world.realm.log.clear();
    world.upstream.log.clear();
    world.decisions.0.lock().unwrap().clear();
}

#[tokio::test]
async fn the_protected_resource_metadata_names_the_resource_the_issuer_and_the_scopes() {
    let world = World::start().await;
    let url = format!(
        "{}/.well-known/oauth-protected-resource/orgs/{ORG}/mcp",
        world.url
    );
    let response = world.http.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let metadata: Value = response.json().await.unwrap();
    assert_eq!(metadata["resource"], canonical(ORG));
    assert_eq!(
        metadata["authorization_servers"],
        json!([world.realm.issuer])
    );
    assert_eq!(metadata["bearer_methods_supported"], json!(["header"]));
    let scopes = metadata["scopes_supported"].as_array().unwrap();
    assert!(scopes.contains(&json!("telemetry:read")));
    assert!(scopes.contains(&json!("oncall:acknowledge")));
    assert_eq!(scopes.len(), 4);

    let other = world
        .http
        .get(format!(
            "{}/.well-known/oauth-protected-resource/orgs/{OTHER_ORG}/mcp",
            world.url
        ))
        .send()
        .await
        .unwrap()
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(other["resource"], canonical(OTHER_ORG));

    let bad = world
        .http
        .get(format!(
            "{}/.well-known/oauth-protected-resource/orgs/not-a-uuid/mcp",
            world.url
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 404);
}

#[tokio::test]
async fn health_needs_no_token_and_a_bad_organisation_is_not_found() {
    let world = World::start().await;
    let health = world
        .http
        .get(format!("{}/healthz", world.url))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), 200);
    let missing = world
        .http
        .post(world.endpoint("not-a-uuid"))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), 404);
    let short = world
        .http
        .post(world.endpoint("11111111111141118111111111111111"))
        .send()
        .await
        .unwrap();
    assert_eq!(short.status(), 404);
}

#[tokio::test]
async fn every_bad_token_is_a_401_with_the_metadata_link_and_nothing_is_called() {
    let world = World::start().await;
    world.realm.set_pdp(Pdp::Rule(permit_all));
    let good = world.claims();
    let with = |key: &str, value: Value| {
        let mut claims = good.clone();
        claims[key] = value;
        sign(&claims)
    };
    let without = |key: &str| {
        let mut claims = good.clone();
        claims.as_object_mut().unwrap().remove(key);
        sign(&claims)
    };
    let cases: Vec<(&str, Option<String>)> = vec![
        ("no token", None),
        ("garbage", Some("not-a-jwt".to_owned())),
        (
            "another organisation's resource",
            Some(with("aud", json!(canonical(OTHER_ORG)))),
        ),
        (
            "the host without the path",
            Some(with("aud", json!(PUBLIC))),
        ),
        (
            "an audience list without ours",
            Some(with("aud", json!(["ferristrace", "account"]))),
        ),
        ("the agent audience", Some(with("aud", json!(AUDIENCE)))),
        ("no audience", Some(without("aud"))),
        ("expired", Some(with("exp", json!(now() - 3600)))),
        ("not yet valid", Some(with("nbf", json!(now() + 3600)))),
        (
            "another issuer",
            Some(with("iss", json!("https://evil.example/realms/ferris"))),
        ),
        ("no issuer", Some(without("iss"))),
        ("no client", Some(without("azp"))),
        ("an id token", Some(with("typ", json!("ID")))),
        ("no expiry", Some(without("exp"))),
        ("a foreign signature", Some(sign_rogue(&good))),
        ("a symmetric signature", Some(sign_hs256(&good))),
        (
            "an unknown key",
            Some(sign_with_kid(Some("other-key"), &good)),
        ),
        ("no key id", Some(sign_with_kid(None, &good))),
    ];
    let link = format!(
        "resource_metadata=\"{PUBLIC}/.well-known/oauth-protected-resource/orgs/{ORG}/mcp\""
    );
    for (name, token) in cases {
        let (status, headers, _) = world
            .post(
                token.as_deref(),
                &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{name}");
        let challenge = headers["www-authenticate"].to_str().unwrap();
        assert!(challenge.starts_with("Bearer "), "{name}: {challenge}");
        assert!(challenge.contains(&link), "{name}: {challenge}");
        assert_eq!(
            challenge.contains("invalid_token"),
            token.is_some(),
            "{name}: {challenge}"
        );
    }
    let (status, headers, _) = world
        .post_raw(None, "{}".to_owned(), &[("authorization", "Basic YTpi")])
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(
        headers["www-authenticate"]
            .to_str()
            .unwrap()
            .contains(&link)
    );

    assert_eq!(world.upstream.log.len(), 0);
    assert_eq!(world.realm.pdp_calls(), 0);
    assert_eq!(world.realm.token_calls(), 0);
    assert_eq!(world.api.log.len(), 0);
    assert!(world.decisions.all().is_empty());
}

#[tokio::test]
async fn a_token_for_the_other_organisations_path_is_refused_here() {
    let world = World::start().await;
    let token = world.user_token();
    let response = world
        .http
        .post(world.endpoint(OTHER_ORG))
        .bearer_auth(&token)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    assert!(
        response.headers()["www-authenticate"]
            .to_str()
            .unwrap()
            .contains(&format!("orgs/{OTHER_ORG}/mcp"))
    );
    assert_eq!(world.upstream.log.len(), 0);
}

#[tokio::test]
async fn an_unreachable_key_set_answers_503_and_nothing_is_called() {
    let world = World::start().await;
    *world.realm.jwks_failing.lock().unwrap() = true;
    let (status, headers, _) = world
        .post(
            Some(&world.user_token()),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        )
        .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(headers.get("www-authenticate").is_none());
    assert_eq!(world.upstream.log.len(), 0);
    assert_eq!(world.realm.pdp_calls(), 0);
}

#[tokio::test]
async fn get_and_delete_are_405_for_a_stateless_server_and_401_without_a_token() {
    let world = World::start().await;
    let token = world.user_token();
    for method in [reqwest::Method::GET, reqwest::Method::DELETE] {
        let response = world
            .http
            .request(method.clone(), world.endpoint(ORG))
            .bearer_auth(&token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 405, "{method}");
        assert_eq!(response.headers()["allow"], "POST");
        let bare = world
            .http
            .request(method, world.endpoint(ORG))
            .send()
            .await
            .unwrap();
        assert_eq!(bare.status(), 401);
    }
    assert_eq!(world.upstream.log.len(), 0);
}

#[tokio::test]
async fn initialize_discover_ping_and_notifications_pass_with_the_exchanged_token_and_no_decision()
{
    let world = World::start().await;
    let token = world.user_token();
    let init = world
        .rpc(&token, "initialize", json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "c", "version": "1"}}))
        .await;
    assert_eq!(init["result"]["serverInfo"]["name"], "fake");
    assert_eq!(
        world.rpc(&token, "ping", json!({})).await["result"],
        json!({})
    );
    assert!(
        world
            .rpc(&token, "server/discover", json!({}))
            .await
            .get("error")
            .is_some()
    );
    let (status, _, _) = world
        .post(
            Some(&token),
            &json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED);

    let seen = world.upstream.log.all();
    assert_eq!(seen.len(), 4);
    for request in &seen {
        assert_eq!(request.header("authorization"), "Bearer exchanged-1");
        assert!(!request.raw.contains(&token));
        assert_eq!(request.path, format!("/orgs/{ORG}/mcp"));
    }
    assert_eq!(world.realm.pdp_calls(), 0);
    assert!(world.decisions.all().is_empty());
    assert_eq!(world.realm.exchanges().len(), 1);
}

#[tokio::test]
async fn the_mcp_server_receives_the_exchanged_token_and_never_the_callers() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let token = world.user_token();
    let (status, _, answer) = world
        .post_raw(
            Some(&token),
            json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "search_logs", "arguments": {"service": "checkout"}}}).to_string(),
            &[
                ("cookie", "session=secret"),
                ("x-forwarded-for", "203.0.113.9"),
                ("mcp-protocol-version", "2026-07-28"),
                ("mcp-method", "tools/call"),
                ("mcp-name", "search_logs"),
            ],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["result"]["content"][0]["text"], "ran search_logs");
    assert_eq!(answer["id"], 5);

    let calls = world.upstream.log.rpc("tools/call");
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert_eq!(call.header("authorization"), "Bearer exchanged-1");
    assert_eq!(call.header("mcp-protocol-version"), "2026-07-28");
    assert_eq!(call.header("mcp-name"), "search_logs");
    assert_eq!(call.header("cookie"), "");
    assert_eq!(call.header("x-forwarded-for"), "");
    assert!(!call.header("x-request-id").is_empty());
    for request in world.upstream.log.all() {
        assert!(!request.raw.contains(&token));
        assert!(
            request
                .headers
                .values()
                .all(|value| !value.contains(&token))
        );
    }

    let exchanges = world.realm.exchanges();
    assert_eq!(exchanges.len(), 1);
    let form = &exchanges[0].form;
    assert_eq!(form["subject_token"], token);
    assert_eq!(
        form["subject_token_type"],
        "urn:ietf:params:oauth:token-type:access_token"
    );
    assert_eq!(form["audience"], AUDIENCE);
    assert_eq!(form["client_id"], "gateway");
    assert_eq!(form["actor_token"], "cc-gateway");

    world.call(&token, "search_logs", &json!({})).await;
    assert_eq!(world.realm.exchanges().len(), 1);
    assert_eq!(
        world.upstream.log.rpc("tools/call")[1].header("authorization"),
        "Bearer exchanged-1"
    );

    let mut other = world.claims();
    other["sub"] = json!("bob-sub");
    other["email"] = json!("bob@example.com");
    let bob = sign(&other);
    world.call(&bob, "search_logs", &json!({})).await;
    assert_eq!(world.realm.exchanges().len(), 2);
    assert_eq!(
        world.upstream.log.rpc("tools/call")[2].header("authorization"),
        "Bearer exchanged-2"
    );
}

#[tokio::test]
async fn a_list_shows_only_the_tools_the_policies_permit_decided_in_one_batch() {
    let world = world_with(Pdp::Rule(read_only_tools)).await;
    world.reply_api(
        "oncall/now",
        200,
        &json!({"on_call": true, "schedules": ["primary"]}),
    );
    let token = world.user_token();
    let (status, headers, answer) = world
        .post(
            Some(&token),
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["id"], 3);
    assert_eq!(
        names(&answer),
        [
            "search_logs",
            "list_services",
            "get_incident",
            "who_is_on_call"
        ]
    );
    assert_eq!(answer["result"]["cacheScope"], "private");
    assert_eq!(headers["cache-control"], "no-store");

    assert_eq!(world.realm.evaluations().len(), 1);
    assert_eq!(world.realm.evaluation().len(), 0);
    let batch = &world.realm.evaluations()[0].body;
    assert_eq!(batch["options"]["evaluations_semantic"], "execute_all");
    assert_eq!(batch["action"], json!({"name": "call_tool"}));
    assert_eq!(batch["subject"]["type"], "User");
    assert_eq!(batch["subject"]["properties"]["on_call"], true);
    assert_eq!(
        batch["subject"]["properties"]["on_call_schedules"],
        json!(["primary"])
    );
    assert_eq!(batch["context"]["target"], json!({}));
    assert_eq!(batch["context"]["arguments"], json!({}));
    assert_eq!(batch["context"]["time"]["hour"], 10);
    let ids: Vec<&str> = batch["evaluations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["resource"]["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 6);
    assert!(ids.contains(&"ferristrace/resolve_incident"));
    assert_eq!(
        batch["evaluations"][4]["resource"]["properties"]["destructive"],
        true
    );
    assert_eq!(
        batch["evaluations"][3]["resource"]["properties"]["access"],
        "write"
    );
    assert_eq!(
        batch["evaluations"][3]["resource"]["properties"]["destructive"],
        false
    );

    let records = world.decisions.all();
    assert_eq!(records.len(), 6);
    assert!(records.iter().all(|record| record.method == "tools/list"));
    let permitted: Vec<_> = records
        .iter()
        .filter(|record| record.decision == "permit")
        .collect();
    let refused: Vec<_> = records
        .iter()
        .filter(|record| record.decision == "deny")
        .collect();
    assert_eq!(permitted.len(), 4);
    assert_eq!(refused.len(), 2);
    assert!(
        permitted
            .iter()
            .all(|record| record.policies == ["members-read"] && record.reason.is_none())
    );
    assert!(
        refused
            .iter()
            .all(|record| record.policies == ["members-read-only"]
                && record.reason == Some("no_permitting_policy"))
    );
    let id = &records[0].request_id;
    assert!(records.iter().all(|record| &record.request_id == id));
    assert_eq!(headers["x-request-id"].to_str().unwrap(), id);
}

#[tokio::test]
async fn a_call_to_a_tool_the_policies_refuse_gets_a_reason_is_recorded_once_and_never_reaches_the_server()
 {
    let world = world_with(Pdp::Rule(read_only_tools)).await;
    let token = world.user_token();
    warm(&world, &token).await;

    let (status, headers, answer) = world
        .post_raw(
            Some(&token),
            json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call", "params": {"name": "resolve_incident", "arguments": {"incident_id": INCIDENT}}}).to_string(),
            &[("x-request-id", "req-42")],
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(answer["id"], 8);
    assert_eq!(answer["error"]["code"], -32001);
    assert_eq!(reason(&answer), "no_permitting_policy");
    assert!(answer.get("result").is_none());
    let text = answer.to_string();
    assert!(!text.contains("members-read-only"));
    assert_eq!(headers["x-request-id"], "req-42");
    assert_eq!(world.upstream.log.len(), 0);

    let records = world.decisions.all();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.kind, "mcp_decision");
    assert_eq!(record.method, "tools/call");
    assert_eq!(record.tool.as_deref(), Some("resolve_incident"));
    assert_eq!(record.subject, "alice-sub");
    assert_eq!(record.subject_type, "User");
    assert_eq!(record.groups, ["Dev"]);
    assert_eq!(record.agent, "claude-code");
    assert!(record.delegated);
    assert_eq!(record.org, ORG);
    assert_eq!(record.decision, "deny");
    assert_eq!(record.reason, Some("no_permitting_policy"));
    assert_eq!(record.policies, ["members-read-only"]);
    assert_eq!(record.request_id, "req-42");
    assert_eq!(record.arguments["incident_id"], INCIDENT);
}

#[tokio::test]
async fn a_permitted_call_is_forwarded_and_recorded_once_with_the_policies_that_decided() {
    let world = world_with(Pdp::Rule(read_only_tools)).await;
    let token = world.user_token();
    warm(&world, &token).await;
    let answer = world
        .call(
            &token,
            "search_logs",
            &json!({"service": "checkout", "limit": 5, "nested": {"a": 1}}),
        )
        .await;
    assert_eq!(answer["result"]["content"][0]["text"], "ran search_logs");
    let records = world.decisions.all();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].decision, "permit");
    assert_eq!(records[0].policies, ["members-read"]);
    assert_eq!(records[0].reason, None);
    assert_eq!(records[0].target["service"], "checkout");
    assert_eq!(records[0].arguments.len(), 2);
    assert!(!records[0].arguments.contains_key("nested"));
    assert_eq!(world.upstream.log.rpc("tools/call").len(), 1);
}

#[tokio::test]
async fn a_call_to_a_tool_the_server_does_not_have_is_refused() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let answer = world
        .call(&world.user_token(), "drop_database", &json!({}))
        .await;
    assert_eq!(reason(&answer), "unknown_tool");
    assert_eq!(world.realm.pdp_calls(), 0);
    assert!(world.upstream.log.rpc("tools/call").is_empty());
    let records = world.decisions.all();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].reason, Some("unknown_tool"));
    let again = world
        .call(&world.user_token(), "drop_database", &json!({}))
        .await;
    assert_eq!(reason(&again), "unknown_tool");
    assert_eq!(world.upstream.log.rpc("tools/list").len(), 1);
}

#[tokio::test]
async fn when_the_pdp_fails_every_call_is_refused_and_the_server_is_never_reached() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let token = world.user_token();
    warm(&world, &token).await;
    let modes = [
        ("a server error", Pdp::Status(500), "pdp_unavailable"),
        ("a bad gateway", Pdp::Status(502), "pdp_unavailable"),
        (
            "no such realm endpoint",
            Pdp::Status(404),
            "pdp_unavailable",
        ),
        (
            "a forbidden service account",
            Pdp::Status(403),
            "pdp_unavailable",
        ),
        ("a slow answer", Pdp::Slow(1500, permit_all), "pdp_timeout"),
        ("not json", Pdp::Body("<html>".to_owned()), "pdp_malformed"),
        (
            "a decision that is not a boolean",
            Pdp::Body(r#"{"decision":"true"}"#.to_owned()),
            "pdp_malformed",
        ),
        (
            "a missing decision",
            Pdp::Body("{}".to_owned()),
            "pdp_malformed",
        ),
        (
            "a null decision",
            Pdp::Body(r#"{"decision":null}"#.to_owned()),
            "pdp_malformed",
        ),
    ];
    let count = modes.len();
    for (name, mode, expected) in modes {
        world.realm.set_pdp(mode);
        let answer = world
            .call(&token, "search_logs", &json!({"service": "checkout"}))
            .await;
        assert_eq!(reason(&answer), expected, "{name}");
        assert!(answer.get("result").is_none(), "{name}");
    }
    assert_eq!(world.upstream.log.len(), 0);
    let records = world.decisions.all();
    assert_eq!(records.len(), count);
    assert!(
        records
            .iter()
            .all(|record| record.decision == "deny" && record.policies.is_empty())
    );
    let reasons: Vec<_> = records.iter().filter_map(|record| record.reason).collect();
    assert_eq!(reasons[0], "pdp_unavailable");
    assert_eq!(reasons[4], "pdp_timeout");
    assert_eq!(reasons[5], "pdp_malformed");
}

#[tokio::test]
async fn when_the_pdp_fails_a_list_shows_no_tool_and_says_why() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let token = world.user_token();
    warm(&world, &token).await;
    let modes = [
        (Pdp::Status(500), "pdp_unavailable"),
        (Pdp::Slow(1500, permit_all), "pdp_timeout"),
        (Pdp::Body("nope".to_owned()), "pdp_malformed"),
        (Pdp::ShortBatch, "pdp_malformed"),
        (
            Pdp::Body(r#"{"evaluations":[{"decision":"yes"}]}"#.to_owned()),
            "pdp_malformed",
        ),
    ];
    for (mode, expected) in modes {
        world.realm.set_pdp(mode);
        let answer = world.rpc(&token, "tools/list", json!({})).await;
        assert_eq!(reason(&answer), expected);
        assert!(answer.get("result").is_none());
    }
    assert!(world.upstream.log.rpc("tools/call").is_empty());
    let records = world.decisions.all();
    assert_eq!(records.len(), 5);
    assert!(
        records
            .iter()
            .all(|record| record.decision == "deny" && record.tool.is_none())
    );
}

#[tokio::test]
async fn a_pdp_that_cannot_be_reached_or_will_not_give_a_token_refuses() {
    let down = World::start_at(clock_at("2026-11-03T09:05:00Z"), |config| {
        config.pdp.url = closed_port_url();
    })
    .await;
    let token = down.user_token();
    let answer = down.call(&token, "search_logs", &json!({})).await;
    assert_eq!(reason(&answer), "pdp_unavailable");
    assert!(down.upstream.log.rpc("tools/call").is_empty());

    let world = world_with(Pdp::Rule(permit_all)).await;
    world.realm.token.lock().unwrap().failing_clients = vec!["pep".to_owned()];
    let answer = world
        .call(&world.user_token(), "search_logs", &json!({}))
        .await;
    assert_eq!(reason(&answer), "pdp_unavailable");
    assert_eq!(world.realm.pdp_calls(), 0);
    assert!(world.upstream.log.rpc("tools/call").is_empty());

    let empty = world_with(Pdp::Rule(permit_all)).await;
    empty.realm.token.lock().unwrap().empty_clients = vec!["pep".to_owned()];
    let answer = empty
        .call(&empty.user_token(), "search_logs", &json!({}))
        .await;
    assert_eq!(reason(&answer), "pdp_malformed");
    assert_eq!(empty.realm.pdp_calls(), 0);
}

#[tokio::test]
async fn the_pdp_is_called_with_its_own_cached_service_token() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let token = world.user_token();
    world.call(&token, "search_logs", &json!({})).await;
    world.call(&token, "search_logs", &json!({})).await;
    assert_eq!(world.realm.evaluation().len(), 2);
    for seen in world.realm.evaluation() {
        assert_eq!(seen.header("authorization"), "Bearer cc-pep");
    }
    let pep: Vec<_> = world
        .realm
        .client_credentials()
        .into_iter()
        .filter(|seen| seen.form["client_id"] == "pep")
        .collect();
    assert_eq!(pep.len(), 1);
    assert_eq!(pep[0].form["client_secret"], "pep-secret");
}

#[tokio::test]
async fn a_call_is_refused_when_the_token_exchange_fails_and_the_server_is_not_reached() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    world.realm.token.lock().unwrap().failing_clients = vec!["gateway".to_owned()];
    let (status, _, _) = world
        .post(
            Some(&world.user_token()),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search_logs", "arguments": {}}}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(world.upstream.log.len(), 0);
    let (status, _, _) = world
        .post(
            Some(&world.user_token()),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
    assert_eq!(world.upstream.log.len(), 0);
}

struct Scenario {
    name: &'static str,
    tool: &'static str,
    arguments: Value,
    break_it: fn(&World),
}

fn incident_body(severity: &str, services: &[&str]) -> Value {
    json!({
        "id": INCIDENT, "title": "t", "severity": severity, "status": "investigating",
        "public": false, "project": null, "opened_ms": 1, "resolved_ms": null,
        "alert": null, "services": services, "paged_schedule": null,
        "attachments": [], "timeline": []
    })
}

#[tokio::test]
async fn a_lookup_that_applies_and_fails_refuses_the_call_with_pip_unavailable() {
    let scenarios = [
        Scenario {
            name: "the clock fails",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("clock", 500, &json!({})),
        },
        Scenario {
            name: "the clock is slow",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| {
                w.api.reply_after(
                    "GET",
                    &format!("/api/orgs/{ORG}/clock"),
                    200,
                    r#"{"timezone":"UTC"}"#,
                    1500,
                )
            },
        },
        Scenario {
            name: "the clock names no zone",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("clock", 200, &json!({"timezone": "Mars/Olympus"})),
        },
        Scenario {
            name: "the clock answers unreadably",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("clock", 200, &json!([])),
        },
        Scenario {
            name: "the clock is missing",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("clock", 404, &json!({})),
        },
        Scenario {
            name: "the clock is refused",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("clock", 403, &json!({})),
        },
        Scenario {
            name: "the on-call state fails",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("oncall/now", 500, &json!({})),
        },
        Scenario {
            name: "the on-call state is slow",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| {
                w.api.reply_after(
                    "GET",
                    &format!("/api/orgs/{ORG}/oncall/now"),
                    200,
                    r#"{"on_call":true,"schedules":[]}"#,
                    1500,
                )
            },
        },
        Scenario {
            name: "the on-call state is unreadable",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("oncall/now", 200, &json!({"on_call": "yes"})),
        },
        Scenario {
            name: "the on-call state is missing",
            tool: "search_logs",
            arguments: json!({}),
            break_it: |w| w.reply_api("oncall/now", 404, &json!({})),
        },
        Scenario {
            name: "the incident fails",
            tool: "resolve_incident",
            arguments: json!({"incident_id": INCIDENT}),
            break_it: |w| w.reply_api(&format!("incidents/{INCIDENT}"), 500, &json!({})),
        },
        Scenario {
            name: "the incident is unreadable",
            tool: "get_incident",
            arguments: json!({"incident_id": INCIDENT}),
            break_it: |w| {
                w.reply_api(
                    &format!("incidents/{INCIDENT}"),
                    200,
                    &json!({"severity": 1}),
                )
            },
        },
        Scenario {
            name: "the incident is slow",
            tool: "comment_incident",
            arguments: json!({"incident_id": INCIDENT, "text": "x"}),
            break_it: |w| {
                w.api.reply_after(
                    "GET",
                    &format!("/api/orgs/{ORG}/incidents/{INCIDENT}"),
                    200,
                    "{}",
                    1500,
                )
            },
        },
        Scenario {
            name: "the placement fails",
            tool: "search_logs",
            arguments: json!({"service": "checkout"}),
            break_it: |w| w.reply_api("services/checkout/placement", 500, &json!({})),
        },
        Scenario {
            name: "the placement is unreadable",
            tool: "search_logs",
            arguments: json!({"service": "checkout"}),
            break_it: |w| {
                w.reply_api(
                    "services/checkout/placement",
                    200,
                    &json!({"environments": "prod"}),
                )
            },
        },
        Scenario {
            name: "the placement of an incident's service fails",
            tool: "resolve_incident",
            arguments: json!({"incident_id": INCIDENT}),
            break_it: |w| {
                w.reply_api(
                    &format!("incidents/{INCIDENT}"),
                    200,
                    &incident_body("sev2", &["checkout"]),
                );
                w.reply_api("services/checkout/placement", 503, &json!({}));
            },
        },
    ];
    for scenario in scenarios {
        let world = world_with(Pdp::Rule(permit_all)).await;
        (scenario.break_it)(&world);
        let answer = world
            .call(&world.user_token(), scenario.tool, &scenario.arguments)
            .await;
        assert_eq!(
            reason(&answer),
            "pip_unavailable",
            "{}: {answer}",
            scenario.name
        );
        assert!(answer.get("result").is_none(), "{}", scenario.name);
        assert_eq!(world.realm.pdp_calls(), 0, "{}", scenario.name);
        assert!(
            world.upstream.log.rpc("tools/call").is_empty(),
            "{}",
            scenario.name
        );
        let records = world.decisions.all();
        assert_eq!(records.len(), 1, "{}", scenario.name);
        assert_eq!(records[0].decision, "deny", "{}", scenario.name);
        assert_eq!(
            records[0].reason,
            Some("pip_unavailable"),
            "{}",
            scenario.name
        );
    }
}

#[tokio::test]
async fn a_list_is_refused_when_the_clock_or_the_on_call_state_cannot_be_read() {
    for broken in [
        (|w: &World| w.reply_api("clock", 500, &json!({}))) as fn(&World),
        |w| w.reply_api("oncall/now", 500, &json!({})),
    ] {
        let world = world_with(Pdp::Rule(permit_all)).await;
        broken(&world);
        let answer = world
            .rpc(&world.user_token(), "tools/list", json!({}))
            .await;
        assert_eq!(reason(&answer), "pip_unavailable");
        assert_eq!(world.realm.pdp_calls(), 0);
        assert!(world.upstream.log.is_empty_for_tools());
    }
}

trait Quiet {
    fn is_empty_for_tools(&self) -> bool;
}

impl Quiet for Log {
    fn is_empty_for_tools(&self) -> bool {
        self.rpc("tools/list").is_empty() && self.rpc("tools/call").is_empty()
    }
}

#[tokio::test]
async fn the_gateways_own_token_is_used_for_lookups_and_names_the_agent_audience() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    world.reply_api(
        "services/checkout/placement",
        200,
        &json!({"project": "shop", "environments": ["staging"]}),
    );
    world
        .call(
            &world.user_token(),
            "search_logs",
            &json!({"service": "checkout"}),
        )
        .await;
    let lookups = world.api.log.all();
    assert_eq!(lookups.len(), 3);
    for lookup in &lookups {
        assert_eq!(
            lookup.header("authorization"),
            format!("Bearer cc-gateway-{AUDIENCE}")
        );
        assert_eq!(lookup.method, "GET");
    }
    let oncall = lookups
        .iter()
        .find(|seen| seen.path.ends_with("/oncall/now"))
        .unwrap();
    assert_eq!(oncall.query, "email=alice%40example.com");
    let credentials = world
        .realm
        .client_credentials()
        .into_iter()
        .find(|seen| seen.form.contains_key("audience"))
        .unwrap();
    assert_eq!(credentials.form["audience"], AUDIENCE);
    assert_eq!(credentials.form["client_id"], "gateway");
}

#[tokio::test]
async fn an_incident_that_does_not_exist_adds_no_attribute_and_the_server_answers() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let answer = world
        .call(
            &world.user_token(),
            "get_incident",
            &json!({"incident_id": INCIDENT}),
        )
        .await;
    assert_eq!(answer["result"]["content"][0]["text"], "ran get_incident");
    let sent = &world.realm.evaluation()[0].body;
    assert_eq!(sent["context"]["target"], json!({}));
    assert_eq!(
        sent["context"]["arguments"],
        json!({"incident_id": INCIDENT})
    );
}

#[tokio::test]
async fn a_service_the_registry_does_not_know_adds_no_placement() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    world
        .call(
            &world.user_token(),
            "search_logs",
            &json!({"service": "ghost"}),
        )
        .await;
    let sent = &world.realm.evaluation()[0].body;
    assert_eq!(sent["context"]["target"], json!({"service": "ghost"}));
}

#[tokio::test]
async fn a_service_that_cannot_be_looked_up_safely_is_refused_before_any_lookup() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let answer = world
        .call(
            &world.user_token(),
            "search_logs",
            &json!({"service": ".."}),
        )
        .await;
    assert_eq!(reason(&answer), "invalid_target");
    assert_eq!(world.api.log.len(), 0);
    assert_eq!(world.realm.pdp_calls(), 0);
}

#[tokio::test]
async fn a_service_name_is_encoded_into_the_lookup_path() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    world
        .call(
            &world.user_token(),
            "search_logs",
            &json!({"service": "a/b?c#d e"}),
        )
        .await;
    let lookup = world
        .api
        .log
        .all()
        .into_iter()
        .find(|seen| seen.path.contains("/services/"))
        .unwrap();
    assert_eq!(
        lookup.path,
        format!("/api/orgs/{ORG}/services/a%2Fb%3Fc%23d%20e/placement")
    );
    assert_eq!(lookup.query, "");
}

#[tokio::test]
async fn the_pdp_request_for_a_call_has_exactly_the_documented_shape() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    world.reply_api(
        "oncall/now",
        200,
        &json!({"on_call": true, "schedules": ["primary"]}),
    );
    world.reply_api(
        &format!("incidents/{INCIDENT}"),
        200,
        &incident_body("sev1", &["checkout"]),
    );
    world.reply_api(
        "services/checkout/placement",
        200,
        &json!({"project": "shop", "environments": ["production"]}),
    );
    let answer = world
        .call(
            &world.user_token(),
            "resolve_incident",
            &json!({"incident_id": INCIDENT}),
        )
        .await;
    assert_eq!(
        answer["result"]["content"][0]["text"],
        "ran resolve_incident"
    );
    let sent = &world.realm.evaluation();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent[0].body,
        json!({
            "subject": {
                "type": "User",
                "id": "alice-sub",
                "properties": {
                    "email": "alice@example.com",
                    "groups": ["Dev"],
                    "roles": ["member"],
                    "on_call": true,
                    "on_call_schedules": ["primary"],
                },
            },
            "action": {"name": "call_tool"},
            "resource": {
                "type": "Tool",
                "id": "ferristrace/resolve_incident",
                "properties": {
                    "tool": "resolve_incident",
                    "organisation": ORG,
                    "access": "write",
                    "destructive": true,
                },
            },
            "context": {
                "agent": "claude-code",
                "delegated": true,
                "target": {
                    "severity": "sev1",
                    "incident_status": "investigating",
                    "services": ["checkout"],
                    "service": "checkout",
                    "project": "shop",
                    "environments": ["production"],
                    "environment": "production",
                },
                "arguments": {"incident_id": INCIDENT},
                "time": {
                    "weekday": 2,
                    "hour": 10,
                    "minute": 5,
                    "timezone": "Europe/Paris",
                    "iso_utc": "2026-11-03T09:05:00Z",
                },
            },
        })
    );
    let record = &world.decisions.all()[0];
    assert_eq!(record.target["severity"], "sev1");
    assert_eq!(record.target["environment"], "production");
    assert_eq!(record.policies, ["permit-all"]);
}

#[tokio::test]
async fn a_search_names_its_targets_and_a_write_does_not_read_the_incident_cache() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    world.reply_api(
        &format!("incidents/{INCIDENT}"),
        200,
        &incident_body("sev3", &[]),
    );
    let token = world.user_token();
    world
        .call(&token, "search_logs", &json!({"service": "checkout", "severity": "error", "namespace": "payments", "pod": "web-1", "text": "boom"}))
        .await;
    let sent = &world.realm.evaluation()[0].body;
    assert_eq!(
        sent["context"]["target"],
        json!({"service": "checkout", "log_severity": "error", "namespace": "payments", "pod": "web-1"})
    );

    world
        .call(&token, "get_incident", &json!({"incident_id": INCIDENT}))
        .await;
    world
        .call(&token, "get_incident", &json!({"incident_id": INCIDENT}))
        .await;
    let fetched = |world: &World| {
        world
            .api
            .log
            .to_path(&format!("/incidents/{INCIDENT}"))
            .len()
    };
    assert_eq!(fetched(&world), 1);
    world
        .call(
            &token,
            "resolve_incident",
            &json!({"incident_id": INCIDENT}),
        )
        .await;
    world
        .call(
            &token,
            "resolve_incident",
            &json!({"incident_id": INCIDENT}),
        )
        .await;
    assert_eq!(fetched(&world), 3);
}

#[tokio::test]
async fn the_clock_and_the_on_call_state_are_read_once_per_window() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let token = world.user_token();
    for _ in 0..3 {
        world.call(&token, "list_services", &json!({})).await;
    }
    world.rpc(&token, "tools/list", json!({})).await;
    assert_eq!(world.api.log.to_path("/clock").len(), 1);
    assert_eq!(world.api.log.to_path("/oncall/now").len(), 1);
}

#[tokio::test]
async fn an_autonomous_client_is_a_client_subject_and_no_on_call_lookup_is_made() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let answer = world
        .call(&world.client_token(), "search_logs", &json!({}))
        .await;
    assert_eq!(answer["result"]["content"][0]["text"], "ran search_logs");
    let sent = &world.realm.evaluation()[0].body;
    assert_eq!(
        sent["subject"],
        json!({"type": "Client", "id": "claude-code"})
    );
    assert_eq!(sent["context"]["delegated"], false);
    assert_eq!(sent["context"]["agent"], "claude-code");
    assert!(world.api.log.to_path("/oncall/now").is_empty());
    let record = &world.decisions.all()[0];
    assert_eq!(record.subject_type, "Client");
    assert_eq!(record.subject, "claude-code");
    assert!(!record.delegated);
}

#[tokio::test]
async fn a_delegated_token_without_an_address_is_a_user_with_no_on_call_attributes() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let mut claims = world.claims();
    claims.as_object_mut().unwrap().remove("email");
    world.call(&sign(&claims), "search_logs", &json!({})).await;
    let sent = &world.realm.evaluation()[0].body;
    let properties = sent["subject"]["properties"].as_object().unwrap();
    assert!(!properties.contains_key("email"));
    assert!(!properties.contains_key("on_call"));
    assert!(!properties.contains_key("on_call_schedules"));
    assert!(properties.contains_key("groups"));
    assert!(world.api.log.to_path("/oncall/now").is_empty());
}

#[tokio::test]
async fn the_act_claim_of_the_callers_token_is_seen_by_the_policy_and_the_log() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let mut claims = world.claims();
    claims["act"] = json!({"client_id": "some-agent"});
    world.call(&sign(&claims), "search_logs", &json!({})).await;
    let sent = &world.realm.evaluation()[0].body;
    assert_eq!(sent["context"]["act"], json!({"client_id": "some-agent"}));
    assert_eq!(
        world.decisions.all()[0].act,
        Some(json!({"client_id": "some-agent"}))
    );
}

#[tokio::test]
async fn roles_may_come_from_the_realm_access_claim() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let mut claims = world.claims();
    let map = claims.as_object_mut().unwrap();
    map.remove("roles");
    map.insert("realm_access".to_owned(), json!({"roles": ["sre"]}));
    world.call(&sign(&claims), "search_logs", &json!({})).await;
    let sent = &world.realm.evaluation()[0].body;
    assert_eq!(sent["subject"]["properties"]["roles"], json!(["sre"]));
}

fn dev_reads_checkout_on_weekdays(request: &Value) -> Verdict {
    let dev = request["subject"]["properties"]["groups"]
        .as_array()
        .is_some_and(|groups| groups.iter().any(|group| group == "Dev"));
    let tool = request["resource"]["properties"]["tool"]
        .as_str()
        .unwrap_or("");
    let context = &request["context"];
    let weekday = context["time"]["weekday"].as_i64().unwrap_or(0);
    let hour = context["time"]["hour"].as_i64().unwrap_or(-1);
    let allowed = dev
        && matches!(tool, "search_logs" | "search_traces")
        && context["target"]["service"] == "checkout"
        && (1..=5).contains(&weekday)
        && (9..18).contains(&hour);
    if allowed {
        Verdict::permit("dev-checkout-weekdays")
    } else {
        Verdict::deny("dev-checkout-weekdays")
    }
}

#[tokio::test]
async fn the_worked_example_is_decided_on_the_organisations_local_time() {
    struct Case {
        name: &'static str,
        at: &'static str,
        service: &'static str,
        group: &'static str,
        tool: &'static str,
        permitted: bool,
    }
    let case = |name, at, service, group, tool, permitted| Case {
        name,
        at,
        service,
        group,
        tool,
        permitted,
    };
    let cases = [
        case(
            "tuesday 10:05 local",
            "2026-11-03T09:05:00Z",
            "checkout",
            "Dev",
            "search_logs",
            true,
        ),
        case(
            "tuesday 20:05 local",
            "2026-11-03T19:05:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "saturday 10:05 local",
            "2026-11-07T09:05:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "sunday 10:05 local",
            "2026-11-08T09:05:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "payments on tuesday 10:05",
            "2026-11-03T09:05:00Z",
            "payments",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "support on tuesday 10:05",
            "2026-11-03T09:05:00Z",
            "checkout",
            "Support",
            "search_logs",
            false,
        ),
        case(
            "a tool the rule does not name",
            "2026-11-03T09:05:00Z",
            "checkout",
            "Dev",
            "get_incident",
            false,
        ),
        case(
            "last minute 17:59 local",
            "2026-11-03T16:59:00Z",
            "checkout",
            "Dev",
            "search_logs",
            true,
        ),
        case(
            "18:00 local",
            "2026-11-03T17:00:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "09:00 local is 08:00 utc",
            "2026-11-03T08:00:00Z",
            "checkout",
            "Dev",
            "search_logs",
            true,
        ),
        case(
            "08:30 local is 07:30 utc",
            "2026-11-03T07:30:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "friday night is saturday local",
            "2026-11-06T23:30:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "summer time monday 09:30 local",
            "2026-03-30T07:30:00Z",
            "checkout",
            "Dev",
            "search_logs",
            true,
        ),
        case(
            "winter time friday 08:30 local",
            "2026-03-27T07:30:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
        case(
            "the hour after the spring change",
            "2026-03-30T06:59:00Z",
            "checkout",
            "Dev",
            "search_logs",
            false,
        ),
    ];
    for case in cases {
        let world = World::start_at(clock_at(case.at), |_| {}).await;
        world
            .realm
            .set_pdp(Pdp::Rule(dev_reads_checkout_on_weekdays));
        let mut claims = world.claims();
        claims["groups"] = json!([case.group]);
        let answer = world
            .call(&sign(&claims), case.tool, &json!({"service": case.service}))
            .await;
        if case.permitted {
            assert_eq!(
                answer["result"]["content"][0]["text"],
                format!("ran {}", case.tool),
                "{}",
                case.name
            );
            assert_eq!(world.decisions.all()[0].decision, "permit", "{}", case.name);
        } else {
            assert_eq!(reason(&answer), "no_permitting_policy", "{}", case.name);
            assert!(
                world.upstream.log.rpc("tools/call").is_empty(),
                "{}",
                case.name
            );
            assert_eq!(world.decisions.all()[0].decision, "deny", "{}", case.name);
        }
    }
}

#[tokio::test]
async fn the_organisations_zone_is_the_one_the_clock_route_gives() {
    let world = World::start_at(clock_at("2026-11-03T09:05:00Z"), |_| {}).await;
    world.reply_api("clock", 200, &json!({"timezone": "Asia/Tokyo"}));
    world.realm.set_pdp(Pdp::Rule(permit_all));
    world
        .call(&world.user_token(), "search_logs", &json!({}))
        .await;
    let time = &world.realm.evaluation()[0].body["context"]["time"];
    assert_eq!(time["timezone"], "Asia/Tokyo");
    assert_eq!(time["hour"], 18);
    assert_eq!(time["minute"], 5);
    assert_eq!(time["weekday"], 2);
    assert_eq!(time["iso_utc"], "2026-11-03T09:05:00Z");
}

#[tokio::test]
async fn methods_the_gateway_does_not_serve_are_refused_recorded_and_never_forwarded() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let token = world.user_token();
    for method in [
        "resources/list",
        "resources/read",
        "prompts/list",
        "prompts/get",
        "completion/complete",
        "logging/setLevel",
        "tools/call/extra",
    ] {
        let answer = world
            .rpc(
                &token,
                method,
                json!({"uri": "file:///etc/passwd", "name": "x"}),
            )
            .await;
        assert_eq!(reason(&answer), "method_not_allowed", "{method}");
        assert_eq!(answer["id"], 1);
    }
    assert_eq!(world.upstream.log.len(), 0);
    assert_eq!(world.realm.pdp_calls(), 0);
    let records = world.decisions.all();
    assert_eq!(records.len(), 7);
    assert!(
        records
            .iter()
            .all(|record| record.reason == Some("method_not_allowed") && record.tool.is_none())
    );
}

#[tokio::test]
async fn malformed_requests_are_400s_and_reach_nothing() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    let token = world.user_token();
    let cases: Vec<(&str, String, i64)> = vec![
        ("not json", "{".to_owned(), -32700),
        ("a batch", json!([{"jsonrpc": "2.0", "id": 1, "method": "tools/list"}]).to_string(), -32600),
        ("a bare response", json!({"jsonrpc": "2.0", "id": 1, "result": {}}).to_string(), -32600),
        ("no method", json!({"jsonrpc": "2.0", "id": 1}).to_string(), -32600),
        ("a call without a name", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {}}).to_string(), -32602),
        ("arguments that are a list", json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search_logs", "arguments": [1]}}).to_string(), -32602),
    ];
    for (name, body, code) in cases {
        let (status, _, answer) = world.post_raw(Some(&token), body, &[]).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(answer["error"]["code"], code, "{name}");
    }
    let call = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search_logs", "arguments": {}}}).to_string();
    for (name, headers) in [
        ("another method", [("mcp-method", "tools/list")]),
        ("another tool", [("mcp-name", "resolve_incident")]),
    ] {
        let (status, _, answer) = world.post_raw(Some(&token), call.clone(), &headers).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{name}");
        assert_eq!(reason(&answer), "header_mismatch", "{name}");
    }
    let huge = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"ping","params":{{"pad":"{}"}}}}"#,
        "x".repeat(2 * 1024 * 1024)
    );
    let (status, _, _) = world.post_raw(Some(&token), huge, &[]).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(world.upstream.log.len(), 0);
    assert_eq!(world.realm.pdp_calls(), 0);
}

#[tokio::test]
async fn a_list_the_server_answers_oddly_is_never_passed_on_unfiltered() {
    for (mode, expected) in [
        (UpstreamMode::ListAsEventStream, StatusCode::BAD_GATEWAY),
        (UpstreamMode::ListGarbage, StatusCode::BAD_GATEWAY),
        (UpstreamMode::Down, StatusCode::INTERNAL_SERVER_ERROR),
    ] {
        let world = world_with(Pdp::Rule(permit_all)).await;
        world.upstream.set_mode(mode);
        let (status, _, answer) = world
            .post(
                Some(&world.user_token()),
                &json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
            )
            .await;
        assert_eq!(status, expected);
        assert!(answer.get("result").is_none());
        assert_eq!(world.realm.pdp_calls(), 0);
    }
    let world = world_with(Pdp::Rule(permit_all)).await;
    world.upstream.set_mode(UpstreamMode::ListError);
    let answer = world
        .rpc(&world.user_token(), "tools/list", json!({}))
        .await;
    assert_eq!(answer["error"]["message"], "boom");
    assert_eq!(world.realm.pdp_calls(), 0);
}

#[tokio::test]
async fn an_unreachable_mcp_server_is_a_502() {
    let world = World::start_at(clock_at("2026-11-03T09:05:00Z"), |config| {
        config.upstream = closed_port_url();
    })
    .await;
    world.realm.set_pdp(Pdp::Rule(permit_all));
    let (status, _, _) = world
        .post(
            Some(&world.user_token()),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_GATEWAY);
}

#[tokio::test]
async fn a_second_org_is_a_separate_resource_with_its_own_audience_and_lookups() {
    let world = world_with(Pdp::Rule(permit_all)).await;
    world.api.reply(
        "GET",
        &format!("/api/orgs/{OTHER_ORG}/clock"),
        200,
        &json!({"timezone": "UTC"}),
    );
    world.api.reply(
        "GET",
        &format!("/api/orgs/{OTHER_ORG}/oncall/now"),
        200,
        &json!({"on_call": false, "schedules": []}),
    );
    let mut claims = world.claims();
    claims["aud"] = json!(canonical(OTHER_ORG));
    let token = sign(&claims);
    let response = world
        .http
        .post(world.endpoint(OTHER_ORG))
        .bearer_auth(&token)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "search_logs", "arguments": {}}}))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let sent = &world.realm.evaluation()[0].body;
    assert_eq!(sent["resource"]["properties"]["organisation"], OTHER_ORG);
    assert_eq!(sent["context"]["time"]["timezone"], "UTC");
    assert_eq!(
        world.upstream.log.rpc("tools/call")[0].path,
        format!("/orgs/{OTHER_ORG}/mcp")
    );
    let replay = world
        .http
        .post(world.endpoint(ORG))
        .bearer_auth(&token)
        .json(&json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}))
        .send()
        .await
        .unwrap();
    assert_eq!(replay.status(), 401);
}

#[tokio::test]
async fn unknown_keys_do_not_make_the_gateway_hammer_the_issuer() {
    let world = World::start().await;
    let good = world.claims();
    for _ in 0..5 {
        let (status, _, _) = world
            .post(
                Some(&sign_with_kid(Some("rotated-away"), &good)),
                &json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(
        world
            .realm
            .jwks_fetches
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
    let (status, _, _) = world
        .post(
            Some(&world.user_token()),
            &json!({"jsonrpc": "2.0", "id": 1, "method": "ping"}),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        world
            .realm
            .jwks_fetches
            .load(std::sync::atomic::Ordering::SeqCst),
        1
    );
}
