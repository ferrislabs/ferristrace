mod support;

use ferristrace_mcp::{ApiClient, TOOL_NAMES, router as mcp_router};
use rmcp::{
    ClientHandler, ClientLifecycleMode, ClientServiceExt, ServiceExt,
    model::{CallToolRequestParams, ProtocolVersion},
    service::{RoleClient, RunningService, ServiceError},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::json;
use support::*;

#[derive(Clone, Default)]
struct Probe;

impl ClientHandler for Probe {}

struct Stack {
    world: World,
    api: Fake,
}

async fn stack() -> Stack {
    let realm = start_realm().await;
    let (api, api_url) = start_api().await;
    let mcp = serve(mcp_router(ApiClient::new(&api_url).unwrap())).await;
    let config = base_config(&realm, &mcp, &api_url);
    let (upstream, _) = start_upstream().await;
    realm.set_pdp(Pdp::Rule(read_only_tools));
    let world = World::assemble(
        realm,
        upstream,
        api.clone(),
        config,
        clock_at("2026-11-03T09:05:00Z"),
    )
    .await;
    Stack { world, api }
}

async fn client(
    stack: &Stack,
    token: &str,
    lifecycle: ClientLifecycleMode,
) -> RunningService<RoleClient, Probe> {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(stack.world.endpoint(ORG)).auth_header(token),
    );
    Probe
        .serve_with_lifecycle(transport, lifecycle)
        .await
        .unwrap()
}

fn discover() -> ClientLifecycleMode {
    ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    }
}

#[tokio::test]
async fn a_client_lists_only_the_read_tools_of_the_real_mcp_server_and_calls_one_with_the_exchanged_token()
 {
    let stack = stack().await;
    let token = stack.world.user_token();
    let client = client(&stack, &token, discover()).await;

    let tools = client.list_all_tools().await.unwrap();
    let mut listed: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    listed.sort_unstable();
    let mut expected: Vec<&str> = TOOL_NAMES
        .iter()
        .copied()
        .filter(|name| {
            !matches!(
                *name,
                "comment_incident" | "resolve_incident" | "acknowledge_page"
            )
        })
        .collect();
    expected.sort_unstable();
    assert_eq!(listed, expected);
    assert_eq!(listed.len(), 12);

    stack.api.reply(
        "GET",
        &format!("/api/orgs/{ORG}/services"),
        200,
        &json!({"services": [{
            "name": "checkout", "requests": 100, "errors": 5, "per_second": 0.5,
            "error_rate": 0.05, "p50_us": 900, "p95_us": 20_000, "p99_us": 90_000,
            "last_seen_ms": 1_700_000_000_000_u64
        }]}),
    );
    let result = client
        .call_tool(CallToolRequestParams::new("list_services"))
        .await
        .unwrap();
    let text = serde_json::to_value(&result.content[0]).unwrap()["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.contains("checkout"), "{text}");

    let services = stack.api.log.to_path("/services");
    assert_eq!(services.len(), 1);
    assert_eq!(services[0].header("authorization"), "Bearer exchanged-1");
    assert!(
        stack
            .api
            .log
            .all()
            .iter()
            .all(|seen| !seen.headers.values().any(|value| value.contains(&token)))
    );
}

#[tokio::test]
async fn a_write_tool_is_refused_with_its_reason_before_the_real_server_or_ferristrace_see_it() {
    let stack = stack().await;
    let client = client(&stack, &stack.world.user_token(), discover()).await;
    client.list_all_tools().await.unwrap();
    stack.api.log.clear();
    stack.world.decisions.0.lock().unwrap().clear();

    let refused = client
        .call_tool(
            CallToolRequestParams::new("resolve_incident").with_arguments(
                json!({"incident_id": INCIDENT})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .unwrap_err();
    let ServiceError::McpError(error) = refused else {
        panic!("expected an MCP error, got {refused:?}");
    };
    assert_eq!(error.code.0, -32001);
    assert_eq!(
        error.data.as_ref().unwrap()["reason"],
        "no_permitting_policy"
    );
    assert!(error.message.contains("no_permitting_policy"));

    let mutating: Vec<_> = stack
        .api
        .log
        .all()
        .into_iter()
        .filter(|seen| seen.method != "GET")
        .collect();
    assert!(mutating.is_empty());
    assert!(
        stack
            .api
            .log
            .to_path(&format!("/incidents/{INCIDENT}"))
            .iter()
            .all(|seen| seen.method == "GET")
    );
    let records = stack.world.decisions.all();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].decision, "deny");
}

#[tokio::test]
async fn the_legacy_initialize_handshake_goes_through_the_gateway_to_the_real_server() {
    let stack = stack().await;
    let token = stack.world.user_token();
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(stack.world.endpoint(ORG))
            .auth_header(&token),
    );
    let client = Probe.serve(transport).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert_eq!(tools.len(), 12);
    assert_eq!(stack.world.realm.evaluations().len(), 1);
}

#[tokio::test]
async fn a_gateway_with_no_policy_hides_every_tool_of_the_real_server() {
    let stack = stack().await;
    stack.world.realm.set_pdp(Pdp::Rule(deny_all));
    let client = client(&stack, &stack.world.user_token(), discover()).await;
    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.is_empty());
}

#[tokio::test]
async fn a_first_call_with_no_list_learns_the_tools_nature_from_the_real_server() {
    let stack = stack().await;
    stack.world.realm.set_pdp(Pdp::Rule(permit_all));
    let client = client(&stack, &stack.world.user_token(), discover()).await;
    stack.api.reply(
        "GET",
        &format!("/api/orgs/{ORG}/services"),
        200,
        &json!({"services": []}),
    );
    client
        .call_tool(
            CallToolRequestParams::new("resolve_incident").with_arguments(
                json!({"incident_id": INCIDENT})
                    .as_object()
                    .cloned()
                    .unwrap(),
            ),
        )
        .await
        .unwrap();
    let sent = &stack.world.realm.evaluation()[0].body;
    assert_eq!(sent["resource"]["properties"]["access"], "write");
    assert_eq!(sent["resource"]["properties"]["destructive"], true);
    let patch: Vec<_> = stack
        .api
        .log
        .all()
        .into_iter()
        .filter(|seen| seen.method == "PATCH")
        .collect();
    assert_eq!(patch.len(), 1);
    assert_eq!(patch[0].header("authorization"), "Bearer exchanged-1");
    assert_eq!(
        patch[0].path,
        format!("/api/orgs/{ORG}/incidents/{INCIDENT}")
    );
}
