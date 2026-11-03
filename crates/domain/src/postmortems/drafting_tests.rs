use uuid::Uuid;

use super::drafting::draft;
use crate::alerts::{AlertStatus, Labels, RuleId};
use crate::identity::{AccountId, Email};
use crate::incidents::{
    AlertRef, Attachment, AttachmentId, AttachmentKind, AttachmentLabel, Author, Change, Chronicle,
    CommentBody, Incident, IncidentId, IncidentSeverity, IncidentStatus, IncidentTitle, Person,
    Reference, TimelineEntry, TimelineId,
};
use crate::oncall::PageId;
use crate::telemetry::TenantId;

const BASE_MS: u64 = 1_700_000_000_000;
const MINUTE: u64 = 60_000;
const TRACE: &str = "0af7651916cd43dd8448eb211c80319c";

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn rule() -> RuleId {
    RuleId::new(Uuid::from_u128(500))
}

fn page() -> PageId {
    PageId::new(Uuid::from_u128(600))
}

fn incident(resolved_ms: Option<u64>) -> Incident {
    Incident {
        id: IncidentId::new(Uuid::from_u128(1)),
        org: TenantId::new(Uuid::from_u128(2)),
        project: None,
        title: IncidentTitle::try_from("Checkout down").expect("a title"),
        severity: IncidentSeverity::Sev2,
        status: if resolved_ms.is_some() {
            IncidentStatus::Resolved
        } else {
            IncidentStatus::Investigating
        },
        owner: Some(account(10)),
        public: false,
        opened_ms: BASE_MS,
        resolved_ms,
        opened_from_alert: Some(rule()),
        paged_schedule: None,
    }
}

fn entry(author: Author, change: Change, at_ms: u64) -> TimelineEntry {
    TimelineEntry {
        id: TimelineId::fresh(),
        incident: IncidentId::new(Uuid::from_u128(1)),
        org: TenantId::new(Uuid::from_u128(2)),
        author,
        change,
        at_ms,
    }
}

fn attachment(kind: AttachmentKind, reference: &str, label: &str) -> Attachment {
    Attachment {
        id: AttachmentId::fresh(),
        incident: IncidentId::new(Uuid::from_u128(1)),
        reference: Reference::parse(kind, reference).expect("a reference"),
        label: AttachmentLabel::try_from(label).expect("a label"),
        at_ms: BASE_MS + 4 * MINUTE,
    }
}

fn chronicle(entries: Vec<TimelineEntry>) -> Chronicle {
    Chronicle {
        entries,
        people: vec![
            Person {
                account: account(9),
                email: Some(Email::try_from("ada@example.com").expect("an email")),
                name: Some("Ada".into()),
            },
            Person {
                account: account(10),
                email: Some(Email::try_from("grace@example.com").expect("an email")),
                name: None,
            },
        ],
        alerts: vec![AlertRef {
            id: rule(),
            name: "Checkout errors".into(),
            services: vec!["checkout".into()],
        }],
    }
}

fn the_resolved_story() -> (Incident, Chronicle, Vec<Attachment>) {
    let trace = attachment(AttachmentKind::Trace, TRACE, "the failing payment");
    let logs = attachment(
        AttachmentKind::Logs,
        "service=checkout level>=error",
        "checkout [errors]",
    );
    let metric = attachment(AttachmentKind::Metric, "http.server.duration", "latency");
    let entries = vec![
        entry(
            Author::Account(account(9)),
            Change::Opened {
                title: IncidentTitle::try_from("Checkout down").expect("a title"),
                severity: IncidentSeverity::Sev2,
                alert: Some(rule()),
            },
            BASE_MS,
        ),
        entry(
            Author::System,
            Change::Alert {
                rule: rule(),
                from: AlertStatus::Pending,
                to: AlertStatus::Firing,
                value: Some(9.5),
                labels: Labels::none(),
                context: None,
            },
            BASE_MS,
        ),
        entry(
            Author::System,
            Change::Paged {
                page: page(),
                step: 0,
                target: Some(account(10)),
            },
            BASE_MS + 1_000,
        ),
        entry(
            Author::System,
            Change::Deploy {
                service: "checkout".into(),
                version: "2.4.1".into(),
                environment: "prod".into(),
            },
            BASE_MS + 90_000,
        ),
        entry(
            Author::Account(account(10)),
            Change::Acknowledged { page: page() },
            BASE_MS + 2 * MINUTE,
        ),
        entry(
            Author::Account(account(9)),
            Change::Comment {
                body: CommentBody::try_from("rolled back\nthe | release  ").expect("a body"),
                public: false,
            },
            BASE_MS + 3 * MINUTE,
        ),
        entry(
            Author::Account(account(9)),
            Change::Attachment(trace.clone()),
            BASE_MS + 4 * MINUTE,
        ),
        entry(
            Author::Account(account(9)),
            Change::Status {
                from: IncidentStatus::Investigating,
                to: IncidentStatus::Resolved,
            },
            BASE_MS + 10 * MINUTE,
        ),
    ];
    (
        incident(Some(BASE_MS + 10 * MINUTE)),
        chronicle(entries),
        vec![trace, logs, metric],
    )
}

#[test]
fn a_resolved_incident_drafts_to_a_fixed_document() {
    let (incident, chronicle, attachments) = the_resolved_story();

    let body = draft(&incident, &chronicle, &attachments);

    let expected = "\
# Postmortem: Checkout down

## Summary

_Write what happened in two sentences._

## Impact

- Window: 2023-11-14 22:13:20 UTC \u{2192} 2023-11-14 22:23:20 UTC (10 minutes)
- Severity: sev2
- Owner: grace@example.com

## Timeline

| Time (UTC) | Author | What happened |
| --- | --- | --- |
| 2023-11-14 22:13:20 UTC | Ada | opened \"Checkout down\" as sev2 from the alert Checkout errors |
| 2023-11-14 22:13:20 UTC | FerrisTrace | alert Checkout errors pending \u{2192} firing (value 9.5) |
| 2023-11-14 22:13:21 UTC | FerrisTrace | paged grace@example.com (step 0) |
| 2023-11-14 22:14:50 UTC | FerrisTrace | deployed checkout 2.4.1 to prod |
| 2023-11-14 22:15:20 UTC | grace@example.com | acknowledged the page |
| 2023-11-14 22:16:20 UTC | Ada | commented: \"rolled back the \\| release\" |
| 2023-11-14 22:17:20 UTC | Ada | attached trace \"the failing payment\" |
| 2023-11-14 22:23:20 UTC | Ada | status investigating \u{2192} resolved |

## Contributing alerts

- Checkout errors, first firing 2023-11-14 22:13:20 UTC

## Deploys

- 2023-11-14 22:14:50 UTC deployed checkout 2.4.1 to prod

## Paging

- grace@example.com paged 2023-11-14 22:13:21 UTC (step 0), acknowledged 2023-11-14 22:15:20 UTC by grace@example.com

## Attachments

- [the failing payment](/traces/0af7651916cd43dd8448eb211c80319c)
- [checkout \\[errors\\]](/logs?f=service%3Dcheckout%20level%3E%3Derror)
- [latency](/metrics?f=http.server.duration)

## Action items

- [ ] 
";
    assert_eq!(body, expected);
}

#[test]
fn drafting_twice_gives_the_same_document() {
    let (incident, chronicle, attachments) = the_resolved_story();

    assert_eq!(
        draft(&incident, &chronicle, &attachments),
        draft(&incident, &chronicle, &attachments)
    );
}

#[test]
fn an_open_incident_says_it_is_still_open() {
    let entries = vec![entry(
        Author::Account(account(9)),
        Change::Opened {
            title: IncidentTitle::try_from("Checkout down").expect("a title"),
            severity: IncidentSeverity::Sev2,
            alert: None,
        },
        BASE_MS,
    )];
    let mut open = incident(None);
    open.opened_from_alert = None;
    open.owner = None;

    let body = draft(&open, &chronicle(entries), &[]);

    assert!(body.contains("- Window: 2023-11-14 22:13:20 UTC \u{2192} still open\n"));
    assert!(body.contains("- Owner: nobody\n"));
    assert!(body.contains("## Contributing alerts\n\n_No alert contributed._\n"));
    assert!(body.contains("## Deploys\n\n_Nothing was deployed._\n"));
    assert!(body.contains("## Paging\n\n_Nobody was paged._\n"));
    assert!(body.contains("## Attachments\n\n_Nothing was attached._\n"));
    assert!(body.ends_with("## Action items\n\n- [ ] \n"));
}

#[test]
fn a_comment_by_an_agent_is_told_under_the_agents_name() {
    let entries = vec![entry(
        Author::Agent {
            id: Some(crate::agents::AgentId::new(Uuid::from_u128(4))),
            name: "triage bot".into(),
            on_behalf_of: None,
        },
        Change::Comment {
            body: CommentBody::try_from("rolled back the release").expect("a body"),
            public: false,
        },
        BASE_MS,
    )];

    let body = draft(&incident(None), &chronicle(entries), &[]);

    assert!(
        body.contains("| triage bot | commented: \"rolled back the release\" |"),
        "{body}"
    );
}

#[test]
fn an_unanswered_and_escalated_page_is_told_as_such() {
    let second = PageId::new(Uuid::from_u128(601));
    let entries = vec![
        entry(
            Author::System,
            Change::Paged {
                page: second,
                step: 0,
                target: Some(account(9)),
            },
            BASE_MS,
        ),
        entry(
            Author::System,
            Change::Paged {
                page: second,
                step: 1,
                target: Some(account(10)),
            },
            BASE_MS + 15 * MINUTE,
        ),
    ];

    let body = draft(&incident(None), &chronicle(entries), &[]);

    assert!(body.contains("- Ada paged 2023-11-14 22:13:20 UTC (step 0), escalated\n"));
    assert!(body.contains(
        "- grace@example.com paged 2023-11-14 22:28:20 UTC (step 1), not acknowledged\n"
    ));
}

#[test]
fn a_one_minute_window_is_singular() {
    let one = incident(Some(BASE_MS + MINUTE));

    let body = draft(&one, &chronicle(Vec::new()), &[]);

    assert!(body.contains("(1 minute)\n"));
}

#[test]
fn every_deploy_entry_is_listed_in_order_and_an_unnamed_environment_is_left_out() {
    let entries = vec![
        entry(
            Author::System,
            Change::Deploy {
                service: "checkout".into(),
                version: "2.4.1".into(),
                environment: "prod".into(),
            },
            BASE_MS + MINUTE,
        ),
        entry(
            Author::System,
            Change::Deploy {
                service: "mailer".into(),
                version: "7".into(),
                environment: "".into(),
            },
            BASE_MS + 2 * MINUTE,
        ),
    ];

    let body = draft(&incident(None), &chronicle(entries), &[]);

    assert!(body.contains(
        "## Deploys\n\n- 2023-11-14 22:14:20 UTC deployed checkout 2.4.1 to prod\n- 2023-11-14 22:15:20 UTC deployed mailer 7\n"
    ));
}
