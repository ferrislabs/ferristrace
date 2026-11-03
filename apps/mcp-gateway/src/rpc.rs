use axum::http::HeaderMap;
use serde_json::{Map, Value, json};

use crate::reason::Reason;

pub const CODE_PARSE_ERROR: i64 = -32700;
pub const CODE_INVALID_REQUEST: i64 = -32600;
pub const CODE_INVALID_PARAMS: i64 = -32602;
pub const CODE_REFUSED: i64 = -32001;

pub const MCP_METHOD: &str = "mcp-method";
pub const MCP_NAME: &str = "mcp-name";

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Map<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Pass,
    ListTools,
    CallTool,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub id: Value,
    pub method: String,
    pub kind: Kind,
    pub call: Option<ToolCall>,
    pub paged: bool,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rejection {
    pub id: Value,
    pub code: i64,
    pub message: &'static str,
}

impl Rejection {
    pub fn body(&self) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": self.id,
            "error": { "code": self.code, "message": self.message },
        })
    }
}

fn bad_request(id: Value, code: i64, message: &'static str) -> Rejection {
    Rejection { id, code, message }
}

pub fn refusal(id: &Value, reason: Reason) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": CODE_REFUSED,
            "message": format!("refused: {} ({})", reason.explanation(), reason.code()),
            "data": { "reason": reason.code() },
        },
    })
}

fn kind_of(method: &str) -> Kind {
    match method {
        "initialize" | "server/discover" | "ping" => Kind::Pass,
        "tools/list" => Kind::ListTools,
        "tools/call" => Kind::CallTool,
        other if other.starts_with("notifications/") => Kind::Pass,
        _ => Kind::Other,
    }
}

pub fn parse(body: &[u8]) -> Result<Message, Rejection> {
    let parsed: Value = serde_json::from_slice(body)
        .map_err(|_| bad_request(Value::Null, CODE_PARSE_ERROR, "parse error"))?;
    let Value::Object(object) = parsed else {
        return Err(bad_request(
            Value::Null,
            CODE_INVALID_REQUEST,
            "a single JSON-RPC request is expected",
        ));
    };
    let id = object.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return Err(bad_request(id, CODE_INVALID_REQUEST, "invalid request"));
    };
    let kind = kind_of(method);
    let call = if kind == Kind::CallTool {
        let params = object.get("params");
        let Some(name) = params
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
        else {
            return Err(bad_request(id, CODE_INVALID_PARAMS, "invalid params"));
        };
        let arguments = match params.and_then(|params| params.get("arguments")) {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(arguments)) => arguments.clone(),
            Some(_) => return Err(bad_request(id, CODE_INVALID_PARAMS, "invalid params")),
        };
        Some(ToolCall {
            name: name.to_owned(),
            arguments,
        })
    } else {
        None
    };
    let paged = kind == Kind::ListTools
        && object
            .get("params")
            .and_then(|params| params.get("cursor"))
            .is_some_and(|cursor| !cursor.is_null());
    Ok(Message {
        id,
        method: method.to_owned(),
        kind,
        call,
        paged,
        value: Value::Object(object),
    })
}

pub fn rewritten(message: &Message, arguments: &Map<String, Value>) -> Option<Vec<u8>> {
    let mut value = message.value.clone();
    let params = value.as_object_mut()?.get_mut("params")?.as_object_mut()?;
    params.insert("arguments".to_owned(), Value::Object(arguments.clone()));
    serde_json::to_vec(&value).ok()
}

pub fn listing_body(cursor: Option<&str>) -> Option<Vec<u8>> {
    let mut request =
        json!({"jsonrpc": "2.0", "id": "ferristrace-gateway-discovery", "method": "tools/list"});
    if let Some(cursor) = cursor {
        request["params"] = json!({ "cursor": cursor });
    }
    with_meta(request.to_string().as_bytes()).or_else(|| Some(request.to_string().into_bytes()))
}

pub fn headers_agree(headers: &HeaderMap, message: &Message) -> bool {
    let read = |name: &str| {
        let mut values = headers.get_all(name).iter();
        let first = values.next().map(|value| value.to_str().ok());
        match (first, values.next()) {
            (None, _) => Ok(None),
            (Some(Some(value)), None) => Ok(Some(value)),
            _ => Err(()),
        }
    };
    let method_ok = match read(MCP_METHOD) {
        Ok(None) => true,
        Ok(Some(method)) => method == message.method,
        Err(()) => false,
    };
    let name_ok = match (read(MCP_NAME), &message.call) {
        (Ok(None), _) | (Ok(Some(_)), None) => true,
        (Ok(Some(name)), Some(call)) => name == call.name,
        (Err(()), _) => false,
    };
    method_ok && name_ok
}

pub fn with_meta(body: &[u8]) -> Option<Vec<u8>> {
    let mut value: Value = serde_json::from_slice(body).ok()?;
    let params = value
        .as_object_mut()?
        .entry("params")
        .or_insert_with(|| json!({}))
        .as_object_mut()?;
    let meta = params
        .entry("_meta")
        .or_insert_with(|| json!({}))
        .as_object_mut()?;
    let mut changed = false;
    for (key, fill) in [
        (
            "io.modelcontextprotocol/protocolVersion",
            json!(PROTOCOL_2026),
        ),
        ("io.modelcontextprotocol/clientCapabilities", json!({})),
    ] {
        if !meta.contains_key(key) {
            meta.insert(key.to_owned(), fill);
            changed = true;
        }
    }
    changed.then(|| value.to_string().into_bytes())
}

pub const PROTOCOL_2026: &str = "2026-07-28";

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn headers(method: Option<&str>, name: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(method) = method {
            headers.insert(MCP_METHOD, HeaderValue::from_str(method).unwrap());
        }
        if let Some(name) = name {
            headers.insert(MCP_NAME, HeaderValue::from_str(name).unwrap());
        }
        headers
    }

    fn call(name: &str) -> Message {
        parse(
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":{"a":1}}})
                .to_string()
                .as_bytes(),
        )
        .unwrap()
    }

    #[test]
    fn a_tool_call_carries_its_name_and_arguments() {
        let message = call("search_logs");
        assert_eq!(message.kind, Kind::CallTool);
        let call = message.call.unwrap();
        assert_eq!(call.name, "search_logs");
        assert_eq!(call.arguments["a"], json!(1));
        assert_eq!(message.id, json!(7));
    }

    #[test]
    fn a_call_without_arguments_has_an_empty_map() {
        let message = parse(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"t","arguments":null}}"#).unwrap();
        assert!(message.call.unwrap().arguments.is_empty());
        let message =
            parse(br#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"t"}}"#)
                .unwrap();
        assert!(message.call.unwrap().arguments.is_empty());
    }

    #[test]
    fn arguments_that_are_not_an_object_are_invalid_params() {
        let rejection = parse(br#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"t","arguments":[1]}}"#).unwrap_err();
        assert_eq!(rejection.code, CODE_INVALID_PARAMS);
        assert_eq!(rejection.id, json!(4));
    }

    #[test]
    fn methods_are_classified() {
        let kind = |method: &str| {
            parse(
                json!({"jsonrpc":"2.0","id":1,"method":method,"params":{"name":"t"}})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap()
            .kind
        };
        assert_eq!(kind("initialize"), Kind::Pass);
        assert_eq!(kind("server/discover"), Kind::Pass);
        assert_eq!(kind("ping"), Kind::Pass);
        assert_eq!(kind("notifications/initialized"), Kind::Pass);
        assert_eq!(kind("notifications/cancelled"), Kind::Pass);
        assert_eq!(kind("tools/list"), Kind::ListTools);
        assert_eq!(kind("tools/call"), Kind::CallTool);
        assert_eq!(kind("resources/read"), Kind::Other);
        assert_eq!(kind("prompts/get"), Kind::Other);
        assert_eq!(kind("resources/list"), Kind::Other);
        assert_eq!(kind("completion/complete"), Kind::Other);
        assert_eq!(kind("tools/call/extra"), Kind::Other);
        assert_eq!(kind("notificationsx"), Kind::Other);
    }

    #[test]
    fn garbage_is_a_parse_error() {
        let rejection = parse(b"{").unwrap_err();
        assert_eq!(rejection.code, CODE_PARSE_ERROR);
        assert_eq!(rejection.body()["id"], Value::Null);
    }

    #[test]
    fn a_batch_a_bare_response_or_a_non_string_method_is_refused() {
        assert_eq!(parse(b"[]").unwrap_err().code, CODE_INVALID_REQUEST);
        assert_eq!(
            parse(b"[{\"method\":\"tools/call\"}]").unwrap_err().code,
            CODE_INVALID_REQUEST
        );
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":1,"result":{}}"#)
                .unwrap_err()
                .code,
            CODE_INVALID_REQUEST
        );
        assert_eq!(
            parse(br#"{"jsonrpc":"2.0","id":1,"method":5}"#)
                .unwrap_err()
                .code,
            CODE_INVALID_REQUEST
        );
    }

    #[test]
    fn a_call_without_a_name_is_invalid_params_and_keeps_its_id() {
        for body in [
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{}}"#,
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call"}"#,
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":""}}"#,
            r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":3}}"#,
        ] {
            let rejection = parse(body.as_bytes()).unwrap_err();
            assert_eq!(rejection.code, CODE_INVALID_PARAMS);
            assert_eq!(rejection.id, json!(9));
        }
    }

    #[test]
    fn matching_or_absent_headers_pass() {
        assert!(headers_agree(
            &headers(Some("tools/call"), Some("search_logs")),
            &call("search_logs")
        ));
        assert!(headers_agree(&headers(None, None), &call("t")));
        assert!(headers_agree(
            &headers(Some("tools/call"), None),
            &call("t")
        ));
    }

    #[test]
    fn a_header_that_disagrees_with_the_body_is_refused() {
        assert!(!headers_agree(
            &headers(Some("tools/call"), Some("other")),
            &call("t")
        ));
        assert!(!headers_agree(
            &headers(Some("tools/list"), Some("t")),
            &call("t")
        ));
        assert!(!headers_agree(
            &headers(Some("tools/list"), None),
            &call("t")
        ));
        assert!(!headers_agree(&headers(None, Some("other")), &call("t")));
    }

    #[test]
    fn duplicate_or_unreadable_headers_are_refused() {
        let mut twice = headers(Some("tools/call"), None);
        twice.append(MCP_METHOD, HeaderValue::from_static("tools/call"));
        assert!(!headers_agree(&twice, &call("t")));
        let mut opaque = HeaderMap::new();
        opaque.insert(MCP_METHOD, HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap());
        assert!(!headers_agree(&opaque, &call("t")));
    }

    #[test]
    fn a_name_header_is_ignored_for_a_method_without_a_name() {
        let list = parse(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert!(headers_agree(
            &headers(Some("tools/list"), Some("x")),
            &list
        ));
    }

    #[test]
    fn a_rewritten_body_carries_the_given_arguments_and_keeps_the_rest() {
        let message = parse(
            json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"t","arguments":{"a":" x "},"_meta":{"k":1}}})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
        let mut arguments = Map::new();
        arguments.insert("a".to_owned(), json!("x"));
        let body: Value =
            serde_json::from_slice(&rewritten(&message, &arguments).unwrap()).unwrap();
        assert_eq!(body["params"]["arguments"], json!({"a": "x"}));
        assert_eq!(body["params"]["name"], "t");
        assert_eq!(body["params"]["_meta"], json!({"k": 1}));
        assert_eq!(body["id"], 7);
    }

    #[test]
    fn a_page_cursor_is_noticed() {
        let first = parse(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert!(!first.paged);
        let next =
            parse(br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"cursor":"c2"}}"#)
                .unwrap();
        assert!(next.paged);
    }

    #[test]
    fn a_refusal_carries_the_reason_and_never_the_policy() {
        let body = refusal(&json!("x"), Reason::NoPermittingPolicy);
        assert_eq!(body["jsonrpc"], "2.0");
        assert_eq!(body["id"], "x");
        assert_eq!(body["error"]["code"], CODE_REFUSED);
        assert_eq!(body["error"]["data"]["reason"], "no_permitting_policy");
        assert!(
            body["error"]["message"]
                .as_str()
                .unwrap()
                .contains("no_permitting_policy")
        );
        assert!(body.get("result").is_none());
    }

    #[test]
    fn the_2026_meta_is_added_when_missing_and_left_alone_when_present() {
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let added: Value = serde_json::from_slice(&with_meta(body).unwrap()).unwrap();
        assert_eq!(
            added["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"],
            json!(PROTOCOL_2026)
        );
        assert!(with_meta(added.to_string().as_bytes()).is_none());
    }
}
