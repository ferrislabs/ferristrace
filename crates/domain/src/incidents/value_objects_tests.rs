use super::errors::BadIncident;
use super::value_objects::{
    AttachmentKind, AttachmentLabel, CommentBody, EntryKind, IncidentScope, IncidentSeverity,
    IncidentStatus, IncidentTitle, MAX_COMMENT_BYTES, MAX_REFERENCE_BYTES, MAX_TITLE_BYTES,
    Reference,
};
use crate::alerts::AlertSeverity;

#[test]
fn every_spelling_reads_back() {
    for severity in IncidentSeverity::ALL {
        assert_eq!(IncidentSeverity::parse(severity.as_str()), Some(*severity));
    }
    for status in IncidentStatus::ALL {
        assert_eq!(IncidentStatus::parse(status.as_str()), Some(*status));
    }
    for kind in EntryKind::ALL {
        assert_eq!(EntryKind::parse(kind.as_str()), Some(*kind));
    }
    for kind in AttachmentKind::ALL {
        assert_eq!(AttachmentKind::parse(kind.as_str()), Some(*kind));
    }
    assert_eq!(IncidentSeverity::parse("critical"), None);
}

#[test]
fn an_alert_severity_maps_to_an_incident_severity() {
    assert_eq!(
        IncidentSeverity::from(AlertSeverity::Critical),
        IncidentSeverity::Sev2
    );
    assert_eq!(
        IncidentSeverity::from(AlertSeverity::Warning),
        IncidentSeverity::Sev3
    );
    assert_eq!(
        IncidentSeverity::from(AlertSeverity::Info),
        IncidentSeverity::Sev4
    );
}

#[test]
fn only_resolved_is_closed() {
    assert!(IncidentStatus::Investigating.is_open());
    assert!(IncidentStatus::Identified.is_open());
    assert!(IncidentStatus::Monitoring.is_open());
    assert!(!IncidentStatus::Resolved.is_open());
    assert!(IncidentScope::Open.admits(IncidentStatus::Monitoring));
    assert!(!IncidentScope::Open.admits(IncidentStatus::Resolved));
    assert!(IncidentScope::Resolved.admits(IncidentStatus::Resolved));
    assert!(IncidentScope::All.admits(IncidentStatus::Investigating));
}

#[test]
fn a_title_is_trimmed_and_bounded() {
    assert_eq!(
        IncidentTitle::try_from("  Checkout down ")
            .expect("a title")
            .as_str(),
        "Checkout down"
    );
    assert_eq!(
        IncidentTitle::try_from("   "),
        Err(BadIncident::Title {
            max: MAX_TITLE_BYTES
        })
    );
    assert!(IncidentTitle::try_from("x".repeat(MAX_TITLE_BYTES + 1).as_str()).is_err());
    assert!(CommentBody::try_from("").is_err());
    assert!(CommentBody::try_from("x".repeat(MAX_COMMENT_BYTES + 1).as_str()).is_err());
    assert!(AttachmentLabel::try_from(" ").is_err());
}

#[test]
fn a_trace_reference_is_a_trace_id_and_a_query_is_bounded() {
    let trace = Reference::parse(AttachmentKind::Trace, "0AF7651916CD43DD8448EB211C80319C")
        .expect("a trace id");
    assert_eq!(trace.as_str(), "0af7651916cd43dd8448eb211c80319c");
    assert_eq!(trace.kind(), AttachmentKind::Trace);
    assert_eq!(
        Reference::parse(AttachmentKind::Trace, "nothex"),
        Err(BadIncident::TraceId)
    );
    assert_eq!(
        Reference::parse(AttachmentKind::Trace, "00000000000000000000000000000000"),
        Err(BadIncident::TraceId)
    );
    let logs =
        Reference::parse(AttachmentKind::Logs, "service=checkout&severity=error").expect("a query");
    assert_eq!(logs.as_str(), "service=checkout&severity=error");
    assert_eq!(
        Reference::parse(
            AttachmentKind::Metric,
            "m".repeat(MAX_REFERENCE_BYTES + 1).as_str()
        ),
        Err(BadIncident::Reference {
            max: MAX_REFERENCE_BYTES
        })
    );
}
