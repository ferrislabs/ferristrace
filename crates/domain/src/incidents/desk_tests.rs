use uuid::Uuid;

use super::desk::IncidentDesk;
use super::entities::{Author, Change, IncidentDraft, IncidentFilter, IncidentPatch};
use super::errors::{BadIncident, IncidentError};
use super::value_objects::{
    AttachmentKind, AttachmentLabel, CommentBody, EntryKind, IncidentScope, IncidentSeverity,
    IncidentStatus, IncidentTitle, Reference,
};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OURS, OneProject, OneSession, Ring, THEIRS, a_project, an_org, scopes,
    token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::alerts::{
    AlertRules, AlertSeverity, AlertStates, AlertStatus, Comparison, Condition, Definition,
    DefinitionParts, Labels, NoDataPolicy, RuleDraft, RuleId, RuleName, RuleState, SeriesState,
    Signal, TraceMeasure, TraceSignal,
};
use crate::audit::{Action, TargetKind};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::organisation::{OrgRole, Whoami};
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Crew, IncidentLedger, Notebook};

const NOW: u64 = 1_700_000_000;
const ME: u128 = 9;
const COLLEAGUE: u128 = 10;
const STRANGER: u128 = 11;

type Desk<'a> = IncidentDesk<
    OneSession,
    MemberOfMine,
    &'a Ring,
    OneProject,
    AlertLedger,
    AlertLedger,
    IncidentLedger,
    IncidentLedger,
    IncidentLedger,
    Crew,
    Notebook,
>;

struct World {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
    notebook: Notebook,
    keys: Ring,
}

impl World {
    fn new() -> Self {
        let crew = Crew::default();
        crew.seat(an_org(MINE), account(ME), "person@example.com");
        crew.seat(an_org(MINE), account(COLLEAGUE), "colleague@example.com");
        crew.seat(an_org(THEIRS), account(STRANGER), "stranger@example.com");
        Self {
            alerts: AlertLedger::default(),
            incidents: IncidentLedger::default(),
            crew,
            notebook: Notebook::default(),
            keys: Ring::default(),
        }
    }

    fn desk(&self, role: OrgRole) -> Desk<'_> {
        IncidentDesk::new(
            Readers::new(Whoami::new(OneSession, MemberOfMine(role)), &self.keys),
            OneProject,
            self.alerts.clone(),
            self.alerts.clone(),
            self.incidents.clone(),
            self.incidents.clone(),
            self.incidents.clone(),
            self.crew.clone(),
            self.notebook.clone(),
        )
    }

    async fn a_rule(&self, org: TenantId, severity: AlertSeverity, status: AlertStatus) -> RuleId {
        let rule = AlertRules::create(
            &self.alerts,
            org,
            &RuleDraft {
                name: RuleName::try_from("Checkout errors").expect("a name"),
                project: Some(a_project(OURS)),
                enabled: true,
                definition: Definition::new(DefinitionParts {
                    signal: Signal::Traces(TraceSignal {
                        service: Some("checkout".into()),
                        operation: None,
                        environment: None,
                        measure: TraceMeasure::ErrorRate,
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                })
                .expect("a definition"),
            },
            account(ME),
        )
        .await
        .expect("kept")
        .expect("fresh");
        AlertStates::keep(
            &self.alerts,
            rule.id,
            &SeriesState {
                labels: Labels::none(),
                state: RuleState {
                    status,
                    value: Some(9.0),
                    since_ms: 1,
                    evaluated_ms: 1,
                },
            },
        )
        .await
        .expect("kept");
        rule.id
    }
}

fn account(seed: u128) -> AccountId {
    AccountId::new(Uuid::from_u128(seed))
}

fn session() -> Reader {
    Reader::Session(token())
}

fn at(seconds: u64) -> UnixSeconds {
    UnixSeconds::new(NOW + seconds)
}

fn titled(title: &str) -> IncidentDraft {
    IncidentDraft {
        title: Some(IncidentTitle::try_from(title).expect("a title")),
        ..IncidentDraft::default()
    }
}

#[tokio::test]
async fn every_change_appends_exactly_one_entry_in_order() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let opened = desk
        .open(&session(), an_org(MINE), &titled("Checkout down"), at(0))
        .await
        .expect("a member opens an incident");
    let id = opened.incident.id;

    desk.amend(
        &session(),
        an_org(MINE),
        id,
        IncidentPatch {
            title: Some(IncidentTitle::try_from("Checkout is down").expect("a title")),
            severity: Some(IncidentSeverity::Sev1),
            status: Some(IncidentStatus::Identified),
            owner: Some(Some(account(COLLEAGUE))),
            public: Some(true),
        },
        at(60),
    )
    .await
    .expect("amended");
    desk.comment(
        &session(),
        an_org(MINE),
        id,
        &CommentBody::try_from("rolled back").expect("a body"),
        at(120),
    )
    .await
    .expect("commented");
    desk.attach(
        &session(),
        an_org(MINE),
        id,
        Reference::parse(AttachmentKind::Trace, "0af7651916cd43dd8448eb211c80319c")
            .expect("a trace"),
        AttachmentLabel::try_from("the failing payment").expect("a label"),
        at(180),
    )
    .await
    .expect("attached");

    let dossier = desk
        .get(&session(), an_org(MINE), id)
        .await
        .expect("a read");
    let kinds: Vec<EntryKind> = dossier
        .chronicle
        .entries
        .iter()
        .map(|entry| entry.change.kind())
        .collect();
    assert_eq!(
        kinds,
        vec![
            EntryKind::Opened,
            EntryKind::Title,
            EntryKind::Severity,
            EntryKind::Status,
            EntryKind::Owner,
            EntryKind::Public,
            EntryKind::Comment,
            EntryKind::Attachment,
        ]
    );
    assert!(
        dossier
            .chronicle
            .entries
            .iter()
            .all(|entry| entry.author == Author::Account(account(ME)))
    );
    assert_eq!(
        dossier.chronicle.entries[2].change,
        Change::Severity {
            from: IncidentSeverity::Sev3,
            to: IncidentSeverity::Sev1
        }
    );
    let incident = &dossier.briefed.incident;
    assert_eq!(incident.title.as_str(), "Checkout is down");
    assert_eq!(incident.status, IncidentStatus::Identified);
    assert!(incident.public);
    assert_eq!(
        dossier
            .briefed
            .owner
            .as_ref()
            .and_then(|owner| owner.email.as_ref())
            .map(crate::identity::Email::as_str),
        Some("colleague@example.com")
    );
    assert_eq!(dossier.attachments.len(), 1);
    assert_eq!(dossier.attachments[0].label.as_str(), "the failing payment");
}

#[tokio::test]
async fn a_patch_that_changes_nothing_appends_nothing() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let opened = desk
        .open(&session(), an_org(MINE), &titled("Checkout down"), at(0))
        .await
        .expect("opened");

    let same = desk
        .amend(
            &session(),
            an_org(MINE),
            opened.incident.id,
            IncidentPatch {
                title: Some(IncidentTitle::try_from("Checkout down").expect("a title")),
                severity: Some(IncidentSeverity::Sev3),
                status: Some(IncidentStatus::Investigating),
                owner: Some(None),
                public: Some(false),
            },
            at(60),
        )
        .await
        .expect("amended");

    assert_eq!(same.incident, opened.incident);
    assert_eq!(world.incidents.timeline().len(), 1);
    assert_eq!(world.notebook.entries().len(), 1);
}

#[tokio::test]
async fn resolving_stamps_the_time_and_reopening_clears_it() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = desk
        .open(&session(), an_org(MINE), &titled("Checkout down"), at(0))
        .await
        .expect("opened")
        .incident
        .id;
    let status = |status| IncidentPatch {
        status: Some(status),
        ..IncidentPatch::default()
    };

    let resolved = desk
        .amend(
            &session(),
            an_org(MINE),
            id,
            status(IncidentStatus::Resolved),
            at(600),
        )
        .await
        .expect("resolved");
    let open_now = desk
        .list(&session(), an_org(MINE), IncidentFilter::default())
        .await
        .expect("a read");
    let closed = desk
        .list(
            &session(),
            an_org(MINE),
            IncidentFilter {
                scope: IncidentScope::Resolved,
                project: None,
            },
        )
        .await
        .expect("a read");
    let reopened = desk
        .amend(
            &session(),
            an_org(MINE),
            id,
            status(IncidentStatus::Monitoring),
            at(900),
        )
        .await
        .expect("reopened");

    assert_eq!(resolved.incident.resolved_ms, Some((NOW + 600) * 1_000));
    assert!(open_now.is_empty());
    assert_eq!(closed.len(), 1);
    assert_eq!(reopened.incident.resolved_ms, None);
    assert_eq!(reopened.incident.status, IncidentStatus::Monitoring);
    let actions: Vec<Action> = world
        .notebook
        .entries()
        .iter()
        .map(|entry| entry.action)
        .collect();
    assert_eq!(
        actions,
        vec![
            Action::IncidentOpened,
            Action::IncidentResolved,
            Action::IncidentReopened
        ]
    );
}

#[tokio::test]
async fn an_incident_declared_from_a_firing_alert_carries_it() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let rule = world
        .a_rule(an_org(MINE), AlertSeverity::Critical, AlertStatus::Firing)
        .await;

    let opened = desk
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                alert: Some(rule),
                ..IncidentDraft::default()
            },
            at(0),
        )
        .await
        .expect("declared");
    let chosen = desk
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                alert: Some(rule),
                severity: Some(IncidentSeverity::Sev1),
                title: Some(IncidentTitle::try_from("Payments").expect("a title")),
                ..IncidentDraft::default()
            },
            at(0),
        )
        .await
        .expect("declared");

    assert_eq!(opened.incident.title.as_str(), "Checkout errors");
    assert_eq!(opened.incident.severity, IncidentSeverity::Sev2);
    assert_eq!(opened.incident.opened_from_alert, Some(rule));
    assert_eq!(opened.incident.project, Some(a_project(OURS)));
    assert_eq!(
        opened.alert.as_ref().map(|alert| &*alert.name),
        Some("Checkout errors")
    );
    assert_eq!(chosen.incident.severity, IncidentSeverity::Sev1);
    assert_eq!(chosen.incident.title.as_str(), "Payments");
    assert_eq!(
        world.incidents.timeline()[0].change,
        Change::Opened {
            title: IncidentTitle::try_from("Checkout errors").expect("a title"),
            severity: IncidentSeverity::Sev2,
            alert: Some(rule),
        }
    );
}

#[tokio::test]
async fn only_a_firing_alert_of_the_same_organisation_declares_an_incident() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let theirs = world
        .a_rule(an_org(THEIRS), AlertSeverity::Warning, AlertStatus::Firing)
        .await;
    let quiet = world
        .a_rule(an_org(MINE), AlertSeverity::Warning, AlertStatus::Pending)
        .await;
    let from = |alert| IncidentDraft {
        alert: Some(alert),
        ..IncidentDraft::default()
    };

    let foreign = desk
        .open(&session(), an_org(MINE), &from(theirs), at(0))
        .await;
    let silent = desk
        .open(&session(), an_org(MINE), &from(quiet), at(0))
        .await;
    let untitled = desk
        .open(&session(), an_org(MINE), &IncidentDraft::default(), at(0))
        .await;

    assert_eq!(foreign.err(), Some(IncidentError::NoSuchAlert));
    assert_eq!(silent.err(), Some(IncidentError::AlertNotFiring));
    assert_eq!(
        untitled.err(),
        Some(IncidentError::Incident(BadIncident::Title { max: 200 }))
    );
    assert!(world.incidents.incidents().is_empty());
    assert!(world.notebook.entries().is_empty());
}

#[tokio::test]
async fn an_owner_is_a_member_of_the_organisation() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = desk
        .open(&session(), an_org(MINE), &titled("Checkout down"), at(0))
        .await
        .expect("opened")
        .incident
        .id;

    let refused = desk
        .amend(
            &session(),
            an_org(MINE),
            id,
            IncidentPatch {
                owner: Some(Some(account(STRANGER))),
                severity: Some(IncidentSeverity::Sev1),
                ..IncidentPatch::default()
            },
            at(60),
        )
        .await;

    assert_eq!(refused.err(), Some(IncidentError::NotAMember));
    assert_eq!(world.incidents.timeline().len(), 1, "nothing half applied");
    assert_eq!(
        world.incidents.incidents()[0].severity,
        IncidentSeverity::Sev3
    );
}

#[tokio::test]
async fn every_incident_write_is_audited() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = desk
        .open(&session(), an_org(MINE), &titled("Checkout down"), at(0))
        .await
        .expect("opened")
        .incident
        .id;
    desk.amend(
        &session(),
        an_org(MINE),
        id,
        IncidentPatch {
            severity: Some(IncidentSeverity::Sev1),
            owner: Some(Some(account(ME))),
            ..IncidentPatch::default()
        },
        at(1),
    )
    .await
    .expect("amended");
    desk.comment(
        &session(),
        an_org(MINE),
        id,
        &CommentBody::try_from("on it").expect("a body"),
        at(2),
    )
    .await
    .expect("commented");
    desk.attach(
        &session(),
        an_org(MINE),
        id,
        Reference::parse(AttachmentKind::Logs, "service=checkout").expect("a query"),
        AttachmentLabel::try_from("checkout errors").expect("a label"),
        at(3),
    )
    .await
    .expect("attached");
    desk.amend(
        &session(),
        an_org(MINE),
        id,
        IncidentPatch {
            status: Some(IncidentStatus::Resolved),
            ..IncidentPatch::default()
        },
        at(4),
    )
    .await
    .expect("resolved");

    let entries = world.notebook.entries();
    let actions: Vec<Action> = entries.iter().map(|entry| entry.action).collect();
    assert_eq!(
        actions,
        vec![
            Action::IncidentOpened,
            Action::IncidentUpdated,
            Action::IncidentCommented,
            Action::IncidentAttached,
            Action::IncidentResolved,
        ]
    );
    assert!(entries.iter().all(|entry| {
        entry.target.kind == TargetKind::Incident
            && entry.target.id.as_deref() == Some(id.to_string().as_str())
            && entry.org == an_org(MINE)
    }));
    assert_eq!(
        entries[1].detail.get("fields"),
        Some(Some("severity,owner"))
    );
}

#[tokio::test]
async fn a_read_key_reads_but_never_writes_and_a_stranger_sees_nothing() {
    let world = World::new();
    let member = world.desk(OrgRole::Member);
    let id = member
        .open(&session(), an_org(MINE), &titled("Checkout down"), at(0))
        .await
        .expect("opened")
        .incident
        .id;
    let key = Reader::Key(
        world
            .keys
            .holding(an_org(MINE), None, scopes(&[KeyScope::Read])),
    );

    let read = member.get(&key, an_org(MINE), id).await;
    let opened = member
        .open(&key, an_org(MINE), &titled("Another"), at(1))
        .await;
    let commented = member
        .comment(
            &key,
            an_org(MINE),
            id,
            &CommentBody::try_from("hi").expect("a body"),
            at(1),
        )
        .await;
    let patched = member
        .amend(
            &key,
            an_org(MINE),
            id,
            IncidentPatch {
                public: Some(true),
                ..IncidentPatch::default()
            },
            at(1),
        )
        .await;
    let stranger_reads = member
        .list(&session(), an_org(THEIRS), IncidentFilter::default())
        .await;
    let stranger_writes = member
        .open(&session(), an_org(THEIRS), &titled("Theirs"), at(1))
        .await;

    assert!(read.is_ok());
    assert_eq!(opened.err(), Some(IncidentError::ReadOnly));
    assert_eq!(commented.err(), Some(IncidentError::ReadOnly));
    assert_eq!(patched.err(), Some(IncidentError::ReadOnly));
    assert_eq!(stranger_reads.err(), Some(IncidentError::NotYours));
    assert_eq!(stranger_writes.err(), Some(IncidentError::NotYours));
    assert_eq!(world.incidents.incidents().len(), 1);
    assert_eq!(world.incidents.timeline().len(), 1);
}

#[tokio::test]
async fn a_project_bound_key_sees_only_its_projects_incidents() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let ours = desk
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                project: Some(a_project(OURS)),
                ..titled("Ours")
            },
            at(0),
        )
        .await
        .expect("opened");
    let whole = desk
        .open(&session(), an_org(MINE), &titled("Whole"), at(1))
        .await
        .expect("opened");
    let bound = Reader::Key(world.keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        scopes(&[KeyScope::Read]),
    ));

    let seen = desk
        .list(&bound, an_org(MINE), IncidentFilter::default())
        .await
        .expect("a read");
    let hidden = desk.get(&bound, an_org(MINE), whole.incident.id).await;
    let elsewhere = desk
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                project: Some(a_project(77)),
                ..titled("Nowhere")
            },
            at(2),
        )
        .await;

    assert_eq!(
        seen.iter()
            .map(|briefed| briefed.incident.id)
            .collect::<Vec<_>>(),
        vec![ours.incident.id]
    );
    assert_eq!(hidden.err(), Some(IncidentError::NoSuchIncident));
    assert_eq!(elsewhere.err(), Some(IncidentError::NoSuchProject));
}
