use super::errors::{BadAgentName, BadAgentScopes, BadClientId};
use super::value_objects::{AgentName, AgentScope, AgentScopes, AgentToken, ClientId};

#[test]
fn there_are_three_agent_scopes_and_no_admin_one() {
    let spelled: Vec<_> = AgentScope::ALL.iter().map(|scope| scope.as_str()).collect();

    assert_eq!(
        spelled,
        ["telemetry:read", "incidents:comment", "incidents:resolve"]
    );
    for refused in [
        "admin",
        "ingest",
        "read",
        "keys:write",
        "",
        "TELEMETRY:READ",
    ] {
        assert_eq!(AgentScope::parse(refused), None, "{refused}");
    }
}

#[test]
fn scopes_are_sorted_unique_and_never_empty() {
    let held = AgentScopes::new(&[AgentScope::IncidentsResolve, AgentScope::TelemetryRead])
        .expect("two scopes");

    assert_eq!(
        held.as_slice(),
        [AgentScope::TelemetryRead, AgentScope::IncidentsResolve]
    );
    assert_eq!(AgentScopes::new(&[]), Err(BadAgentScopes::Empty));
    assert_eq!(
        AgentScopes::new(&[AgentScope::TelemetryRead, AgentScope::TelemetryRead]),
        Err(BadAgentScopes::Repeated)
    );
    assert_eq!(
        AgentScopes::parse(&["telemetry:read", "owner"]),
        Err(BadAgentScopes::Unknown)
    );
}

#[test]
fn effective_scopes_are_the_claimed_scopes_that_were_granted() {
    let granted = AgentScopes::new(&[AgentScope::TelemetryRead, AgentScope::IncidentsComment])
        .expect("scopes");

    let effective = granted.within(&["incidents:resolve", "telemetry:read", "openid", "admin"]);
    let none = granted.within::<&str>(&[]);
    let prefix = granted.within(&["telemetry:read:extra", "telemetry"]);

    assert_eq!(effective, [AgentScope::TelemetryRead]);
    assert!(none.is_empty());
    assert!(prefix.is_empty());
}

#[test]
fn names_and_client_ids_are_trimmed_and_bounded() {
    assert_eq!(
        AgentName::try_from("  triage bot ").map(|name| name.as_str().to_owned()),
        Ok("triage bot".to_owned())
    );
    assert_eq!(AgentName::try_from("   "), Err(BadAgentName::Blank));
    assert_eq!(
        AgentName::try_from("x".repeat(65).as_str()),
        Err(BadAgentName::TooLong(64))
    );
    assert_eq!(ClientId::try_from(""), Err(BadClientId::Blank));
    assert_eq!(ClientId::try_from("two words"), Err(BadClientId::Spaced));
    assert_eq!(ClientId::try_from("tab\there"), Err(BadClientId::Spaced));
    assert_eq!(
        ClientId::try_from("c".repeat(129).as_str()),
        Err(BadClientId::TooLong(128))
    );
    assert_eq!(
        ClientId::try_from(" triage-agent ").map(|id| id.as_str().to_owned()),
        Ok("triage-agent".to_owned())
    );
}

#[test]
fn a_bearer_with_three_parts_is_an_agent_token_and_a_key_is_not() {
    assert!(AgentToken::looks_like_one("aaa.bbb.ccc"));
    assert!(!AgentToken::looks_like_one("ft_live_000000000000.secret"));
    assert!(!AgentToken::looks_like_one(
        "ft_live_000000000000.secret.more"
    ));
    assert!(!AgentToken::looks_like_one("aaa.bbb"));
    assert!(!AgentToken::looks_like_one("aaa.bbb.ccc.ddd"));
    assert!(!AgentToken::looks_like_one("aaa..ccc"));
    assert!(!AgentToken::looks_like_one(".bbb.ccc"));
    assert!(!AgentToken::looks_like_one("aaa.bbb."));
    assert!(!AgentToken::looks_like_one("ft_ss_live"));
}

#[test]
fn a_token_never_prints() {
    let token = AgentToken::from_plaintext("aaa.bbb.ccc");

    assert_eq!(format!("{token:?}"), "AgentToken(redacted)");
}
