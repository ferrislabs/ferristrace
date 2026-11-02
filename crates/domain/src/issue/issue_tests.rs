use uuid::Uuid;

use super::entities::Issue;
use super::errors::BadFingerprint;
use super::fingerprint::{fingerprint, normalise};
use super::value_objects::{Counted, Decision, Fingerprint, Occurrence, Status};
use crate::catalogue::ProjectId;
use crate::identity::AccountId;
use crate::telemetry::{SpanId, TraceId};

fn an_occurrence(service: &str, kind: &str, message: &str) -> Occurrence {
    Occurrence {
        service: service.into(),
        exception_type: kind.into(),
        message: message.into(),
        stacktrace: None,
        trace_id: TraceId::try_from([7u8; 16].as_slice()).expect("a trace id"),
        span_id: SpanId::try_from([3u8; 8].as_slice()).expect("a span id"),
        time_unix_nanos: 1,
    }
}

fn same(left: &str, right: &str) -> bool {
    fingerprint(&an_occurrence("checkout", "PspTimeoutError", left))
        == fingerprint(&an_occurrence("checkout", "PspTimeoutError", right))
}

#[test]
fn digits_do_not_separate_two_instances_of_one_error() {
    assert!(same(
        "upstream psp-gateway timed out after 3000 ms",
        "upstream psp-gateway timed out after 812 ms"
    ));
    assert!(
        !same(
            "upstream psp-gateway timed out",
            "upstream psp-gateway refused the charge"
        ),
        "different prose is a different error"
    );
}

#[test]
fn a_uuid_or_a_hex_blob_does_not_separate_them() {
    assert!(same(
        "order 6f1c9a52-0e44-4f0a-9f6f-0a5f6f0a5f6f was not found",
        "order 11112222-3333-4444-5555-666677778888 was not found"
    ));
    assert!(same(
        "span 4bf92f3577b34da6 is orphaned",
        "span 45c48cce2e2d7fbd is orphaned"
    ));
    assert!(
        !same("order abc was not found", "order abcd was not found"),
        "a short word is prose, not an identifier"
    );
}

#[test]
fn a_quoted_value_does_not_separate_them() {
    assert!(same(
        "Cannot read properties of undefined (reading 'token')",
        "Cannot read properties of undefined (reading 'session')"
    ));
    assert!(same(
        r#"column "created_at" does not exist"#,
        r#"column "deleted_at" does not exist"#
    ));
}

#[test]
fn only_the_last_segment_of_a_path_is_kept() {
    assert!(same(
        "cannot open /var/lib/ferristrace/queue.db",
        "cannot open /tmp/queue.db"
    ));
    assert!(
        !same(
            "cannot open /var/lib/ferristrace/queue.db",
            "cannot open /var/lib/ferristrace/spool.db"
        ),
        "a different file is a different error"
    );
}

#[test]
fn whitespace_and_case_do_not_separate_them() {
    assert!(same("Pool   exhausted\n", "pool exhausted"));
}

#[test]
fn the_exception_type_separates_two_errors_with_one_message() {
    let timeout = an_occurrence("checkout", "PspTimeoutError", "the call failed");
    let refused = an_occurrence("checkout", "PspRefusedError", "the call failed");

    assert_ne!(fingerprint(&timeout), fingerprint(&refused));
}

#[test]
fn the_service_separates_the_same_exception_in_two_places() {
    let here = an_occurrence("checkout", "PspTimeoutError", "the call failed");
    let there = an_occurrence("payments", "PspTimeoutError", "the call failed");

    assert_ne!(fingerprint(&here), fingerprint(&there));
}

#[test]
fn a_field_boundary_cannot_be_moved_to_collide() {
    let split = an_occurrence("checkout", "Psp", "Timeout the call failed");
    let other = an_occurrence("checkout", "PspTimeout", "the call failed");

    assert_ne!(fingerprint(&split), fingerprint(&other));
}

#[test]
fn a_fingerprint_is_stable_and_sixteen_bytes() {
    let once = fingerprint(&an_occurrence("checkout", "Boom", "it broke 7 times"));
    let twice = fingerprint(&an_occurrence("checkout", "Boom", "it broke 9 times"));

    assert_eq!(once, twice);
    assert_eq!(once.as_bytes().len(), 16);
    assert_eq!(once.to_string().len(), 32);
}

#[test]
fn a_fingerprint_round_trips_through_hex() {
    let print = fingerprint(&an_occurrence("checkout", "Boom", "it broke"));

    let read = Fingerprint::try_from(print.to_string().as_str()).expect("it reads back");

    assert_eq!(read, print);
    assert_eq!(Fingerprint::try_from("abcd"), Err(BadFingerprint));
    assert_eq!(
        Fingerprint::try_from("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"),
        Err(BadFingerprint)
    );
    assert_eq!(Fingerprint::try_from(""), Err(BadFingerprint));
}

#[test]
fn normalising_keeps_the_shape_a_reader_would_recognise() {
    assert_eq!(
        normalise("POST /api/checkout returned 502 after 1200 ms"),
        "post checkout returned 0 after 0 ms"
    );
}

fn counted(last_seen: u64) -> Counted {
    Counted {
        project: ProjectId::new(Uuid::from_u128(4)),
        print: fingerprint(&an_occurrence("checkout", "Boom", "it broke")),
        exception_type: "Boom".into(),
        message: "it broke".into(),
        service: "checkout".into(),
        count: 3,
        first_seen_unix_nanos: 10,
        last_seen_unix_nanos: last_seen,
    }
}

fn resolved_at(moment: u64) -> Decision {
    Decision {
        status: Status::Resolved,
        owner: None,
        decided_at_unix_nanos: moment,
        decided_by: Some(AccountId::new(Uuid::from_u128(9))),
    }
}

#[test]
fn an_issue_nobody_touched_is_unresolved_and_not_regressed() {
    let issue = Issue::of(counted(20), Decision::default());

    assert_eq!(issue.decision.status, Status::Unresolved);
    assert!(!issue.regressed);
    assert_eq!(issue.decision.decided_by, None);
}

#[test]
fn an_occurrence_after_a_resolution_is_a_regression_that_keeps_its_status() {
    let regressed = Issue::of(counted(300), resolved_at(200));
    let quiet = Issue::of(counted(100), resolved_at(200));

    assert!(regressed.regressed);
    assert_eq!(
        regressed.decision.status,
        Status::Resolved,
        "a regression does not reopen the issue on its own"
    );
    assert!(!quiet.regressed);
}

#[test]
fn a_status_round_trips_through_its_wire_name() {
    for status in [Status::Unresolved, Status::Resolved, Status::Ignored] {
        assert_eq!(Status::parse(status.as_str()), Some(status));
    }
    assert_eq!(Status::parse("wontfix"), None);
}
