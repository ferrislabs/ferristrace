use super::{Resource, ResourceKind, ResourceStatus};

const SECOND: u64 = 1_000_000_000;
const NOW: u64 = 10_000 * SECOND;

fn resource(last_seen: u64) -> Resource {
    Resource {
        kind: ResourceKind::Container,
        id: "web".into(),
        name: "web".into(),
        cluster: "".into(),
        environment: "".into(),
        provider: "".into(),
        region: "".into(),
        zone: "".into(),
        host_type: "".into(),
        first_seen_unix_nanos: 0,
        last_seen_unix_nanos: last_seen,
        silent: false,
        cpu_percent: Some(12.0),
        memory_bytes: Some(1024),
        services: Vec::new(),
    }
}

#[test]
fn a_resource_that_reported_a_minute_ago_is_active_and_keeps_its_figures() {
    let kept = resource(NOW - 60 * SECOND).settled(NOW);

    assert!(!kept.silent);
    assert_eq!(kept.cpu_percent, Some(12.0));
    assert_eq!(kept.memory_bytes, Some(1024));
}

#[test]
fn a_resource_quiet_for_over_five_minutes_is_silent_and_shows_no_figures() {
    let kept = resource(NOW - 301 * SECOND).settled(NOW);

    assert!(kept.silent);
    assert_eq!(kept.cpu_percent, None);
    assert_eq!(kept.memory_bytes, None);
}

#[test]
fn exactly_five_minutes_of_quiet_is_still_active() {
    assert!(!resource(NOW - 300 * SECOND).settled(NOW).silent);
}

#[test]
fn kinds_round_trip_through_their_names() {
    for kind in ResourceKind::ALL {
        assert_eq!(ResourceKind::parse(kind.name()), Some(*kind));
    }
    assert_eq!(ResourceKind::parse("database"), None);
}

#[test]
fn a_status_holds_only_for_its_own_silence() {
    assert!(ResourceStatus::Silent.holds(true));
    assert!(!ResourceStatus::Silent.holds(false));
    assert!(ResourceStatus::Active.holds(false));
    assert_eq!(ResourceStatus::parse("gone"), None);
}
