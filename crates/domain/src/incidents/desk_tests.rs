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
use crate::oncall::Paging;
use crate::organisation::{OrgRole, Whoami};
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Crew, IncidentLedger, Notebook, OncallLedger, StatusLedger};

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
    OncallLedger,
    OncallLedger,
    StatusLedger,
>;

struct World {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
    notebook: Notebook,
    keys: Ring,
    oncall: OncallLedger,
    status: StatusLedger,
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
            oncall: crate::oncall::fixtures::oncall(),
            status: StatusLedger::default(),
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
            Paging::new(self.oncall.clone(), self.oncall.clone()),
            self.status.clone(),
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
                        placement: crate::placement::Placement::default(),
                    }),
                    condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
                    window_secs: 300,
                    pending_secs: 0,
                    severity,
                    no_data: NoDataPolicy::Ok,
                    channels: Vec::new(),
                    page: None,
                    mentions: Default::default(),
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
        false,
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
        false,
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
            false,
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

fn paging(title: &str) -> IncidentDraft {
    IncidentDraft {
        page: Some(crate::oncall::fixtures::weekly().id),
        ..titled(title)
    }
}

#[tokio::test]
async fn spec_l2_5_declaring_with_paging_pages_the_person_on_call_at_that_time() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let opened_at = crate::oncall::fixtures::START - NOW + 7 * 86_400;

    let opened = desk
        .open(
            &session(),
            an_org(MINE),
            &paging("Checkout down"),
            at(opened_at),
        )
        .await
        .expect("declared with paging");

    assert_eq!(
        opened.incident.paged_schedule,
        Some(crate::oncall::fixtures::weekly().id)
    );
    let pages = world.oncall.pages();
    assert_eq!(pages.len(), 1);
    assert_eq!(
        pages[0].target,
        crate::oncall::fixtures::person(crate::oncall::fixtures::ADA),
        "the second week of the rotation"
    );
    assert_eq!(
        pages[0].source,
        crate::oncall::PageSource::Incident(opened.incident.id)
    );
    let kinds: Vec<_> = world
        .incidents
        .timeline()
        .into_iter()
        .map(|entry| (entry.author, entry.change.kind()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            (Author::Account(account(ME)), EntryKind::Opened),
            (Author::System, EntryKind::Paged),
        ]
    );
    assert_eq!(
        world.notebook.entries()[0].detail.get("paged"),
        Some(Some(
            crate::oncall::fixtures::weekly().id.to_string().as_str()
        ))
    );
}

#[tokio::test]
async fn resolving_a_paged_incident_resolves_its_page() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let opened_at = crate::oncall::fixtures::START - NOW;
    let opened = desk
        .open(
            &session(),
            an_org(MINE),
            &paging("Checkout down"),
            at(opened_at),
        )
        .await
        .expect("declared");

    desk.amend(
        &session(),
        an_org(MINE),
        opened.incident.id,
        IncidentPatch {
            status: Some(IncidentStatus::Resolved),
            ..IncidentPatch::default()
        },
        at(opened_at + 60),
    )
    .await
    .expect("resolved");

    assert_eq!(
        world.oncall.pages()[0].state,
        crate::oncall::PageState::Resolved
    );
}

#[tokio::test]
async fn declaring_with_an_unknown_schedule_is_refused() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);

    let refused = desk
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                page: Some(crate::oncall::ScheduleId::fresh()),
                ..titled("Nowhere")
            },
            at(0),
        )
        .await;

    assert_eq!(refused.map(|_| ()), Err(IncidentError::NoSuchSchedule));
    assert!(world.incidents.incidents().is_empty());
}

fn a_status_page(
    world: &World,
    org: TenantId,
    slug: &str,
    public: bool,
) -> crate::status::StatusPageId {
    let id = crate::status::StatusPageId::fresh();
    world.status.put_page(crate::status::StatusPage {
        id,
        org,
        name: crate::status::PageName::try_from("Acme").expect("a name"),
        slug: crate::status::Slug::try_from(slug).expect("a slug"),
        public,
        hide_from_search: false,
        theme: crate::status::Theme::default(),
        intro: None,
        created_ms: 1,
        updated_ms: 1,
    });
    id
}

fn a_subscriber(world: &World, page: crate::status::StatusPageId, email: &str, confirmed: bool) {
    world.status.put_subscriber(crate::status::Subscriber {
        id: crate::status::SubscriberId::fresh(),
        page,
        email: crate::identity::Email::try_from(email).expect("an address"),
        confirmed,
        invited_ms: 1,
    });
}

fn updates(world: &World) -> Vec<(crate::status::UpdateKind, String)> {
    world
        .status
        .letters()
        .into_iter()
        .filter_map(|letter| match letter.body {
            crate::status::LetterBody::Update(bulletin) => {
                Some((bulletin.kind, bulletin.text.to_string()))
            }
            crate::status::LetterBody::Confirm => None,
        })
        .collect()
}

#[tokio::test]
async fn a_comment_keeps_whether_it_is_public_in_the_timeline() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let opened = desk
        .open(&session(), an_org(MINE), &titled("Checkout down"), at(0))
        .await
        .expect("opened");
    let id = opened.incident.id;

    for (text, public) in [("internal", false), ("for everyone", true)] {
        desk.comment(
            &session(),
            an_org(MINE),
            id,
            &CommentBody::try_from(text).expect("a body"),
            public,
            at(1),
        )
        .await
        .expect("commented");
    }

    let comments: Vec<(String, bool)> = world
        .incidents
        .timeline()
        .into_iter()
        .filter_map(|entry| match entry.change {
            Change::Comment { body, public } => Some((body.as_str().to_owned(), public)),
            _ => None,
        })
        .collect();
    assert_eq!(
        comments,
        vec![
            ("internal".to_owned(), false),
            ("for everyone".to_owned(), true)
        ]
    );
    let audited = world
        .notebook
        .entries()
        .into_iter()
        .filter(|entry| entry.action == Action::IncidentCommented)
        .map(|entry| entry.detail.get("public").flatten().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    assert_eq!(
        audited,
        vec![Some("false".to_owned()), Some("true".to_owned())]
    );
}

#[tokio::test]
async fn a_public_comment_on_a_public_incident_reaches_each_confirmed_subscriber_once() {
    let world = World::new();
    let page = a_status_page(&world, an_org(MINE), "acme", true);
    let hidden = a_status_page(&world, an_org(MINE), "acme-internal", false);
    let theirs = a_status_page(&world, an_org(THEIRS), "theirs", true);
    a_subscriber(&world, page, "one@example.com", true);
    a_subscriber(&world, page, "two@example.com", true);
    a_subscriber(&world, page, "pending@example.com", false);
    a_subscriber(&world, hidden, "hidden@example.com", true);
    a_subscriber(&world, theirs, "stranger@example.com", true);
    let desk = world.desk(OrgRole::Member);
    let public = desk
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                public: true,
                ..titled("Checkout down")
            },
            at(0),
        )
        .await
        .expect("opened")
        .incident
        .id;

    desk.comment(
        &session(),
        an_org(MINE),
        public,
        &CommentBody::try_from("the primary db is full").expect("a body"),
        false,
        at(1),
    )
    .await
    .expect("commented");
    desk.comment(
        &session(),
        an_org(MINE),
        public,
        &CommentBody::try_from("Some payments fail.").expect("a body"),
        true,
        at(2),
    )
    .await
    .expect("commented");

    let comments: Vec<_> = updates(&world)
        .into_iter()
        .filter(|(kind, _)| *kind == crate::status::UpdateKind::Comment)
        .collect();
    assert_eq!(
        comments.len(),
        2,
        "one per confirmed subscriber of a public page"
    );
    assert!(
        comments
            .iter()
            .all(|(_, text)| text == "Some payments fail.")
    );
    let reached: Vec<_> = world
        .status
        .letters()
        .iter()
        .map(|letter| letter.page)
        .collect();
    assert!(reached.iter().all(|held| *held == page));
}

#[tokio::test]
async fn opening_and_moving_a_public_incident_is_announced_and_a_private_one_never() {
    let world = World::new();
    let page = a_status_page(&world, an_org(MINE), "acme", true);
    a_subscriber(&world, page, "one@example.com", true);
    let desk = world.desk(OrgRole::Member);
    let private = desk
        .open(&session(), an_org(MINE), &titled("Quiet"), at(0))
        .await
        .expect("opened")
        .incident
        .id;
    desk.comment(
        &session(),
        an_org(MINE),
        private,
        &CommentBody::try_from("public but on a private incident").expect("a body"),
        true,
        at(1),
    )
    .await
    .expect("commented");
    desk.amend(
        &session(),
        an_org(MINE),
        private,
        IncidentPatch {
            status: Some(IncidentStatus::Identified),
            ..IncidentPatch::default()
        },
        at(2),
    )
    .await
    .expect("amended");
    assert!(updates(&world).is_empty());

    let public = desk
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                public: true,
                ..titled("Loud")
            },
            at(3),
        )
        .await
        .expect("opened")
        .incident
        .id;
    desk.amend(
        &session(),
        an_org(MINE),
        public,
        IncidentPatch {
            status: Some(IncidentStatus::Monitoring),
            severity: Some(IncidentSeverity::Sev1),
            ..IncidentPatch::default()
        },
        at(4),
    )
    .await
    .expect("amended");

    assert_eq!(
        updates(&world),
        vec![
            (
                crate::status::UpdateKind::Opened,
                "investigating".to_owned()
            ),
            (crate::status::UpdateKind::Status, "monitoring".to_owned()),
        ]
    );
}

fn called_by(client: &str, claimed: &[&str], jti: &str) -> Reader {
    use crate::agents::{AgentCall, VerifiedToken};

    Reader::Agent(AgentCall {
        token: VerifiedToken {
            client_id: client.into(),
            scopes: claimed.iter().map(|scope| (*scope).into()).collect(),
            jti: jti.into(),
            expires_at_unix_secs: 4_000_000_000,
            delegated: None,
        },
        route: "/api/orgs/{org}/incidents/{id}".into(),
    })
}

fn called_for(client: &str, claimed: &[&str], jti: &str, email: Option<&str>) -> Reader {
    use crate::agents::Delegation;

    let Reader::Agent(mut call) = called_by(client, claimed, jti) else {
        unreachable!()
    };
    call.token.delegated = Some(Delegation {
        subject: "sub-dana".into(),
        email: email.map(Into::into),
        act_sub: Some("gateway".into()),
        act_client_id: None,
    });
    Reader::Agent(call)
}

fn everything(client: &str, jti: &str) -> Reader {
    called_by(
        client,
        &["telemetry:read", "incidents:comment", "incidents:resolve"],
        jti,
    )
}

fn hi() -> CommentBody {
    CommentBody::try_from("hi").expect("a body")
}

fn resolving() -> IncidentPatch {
    IncidentPatch {
        status: Some(IncidentStatus::Resolved),
        ..IncidentPatch::default()
    }
}

async fn an_open_incident(
    world: &World,
    project: Option<crate::catalogue::ProjectId>,
) -> crate::incidents::IncidentId {
    world
        .desk(OrgRole::Member)
        .open(
            &session(),
            an_org(MINE),
            &IncidentDraft {
                project,
                ..titled("Checkout down")
            },
            at(0),
        )
        .await
        .expect("opened")
        .incident
        .id
}

fn frozen(world: &World, entries: usize) {
    assert_eq!(world.incidents.timeline().len(), entries);
    assert!(world.keys.book.uses().is_empty());
}

#[tokio::test]
async fn an_agent_reads_incidents_but_open_and_attach_stay_refused_to_it() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    world.keys.book.enrol(
        an_org(MINE),
        "bot",
        &[
            AgentScope::TelemetryRead,
            AgentScope::IncidentsComment,
            AgentScope::IncidentsResolve,
        ],
        None,
    );
    let agent = everything("bot", "jti-1");

    let opened = desk
        .open(&agent, an_org(MINE), &titled("Another"), at(1))
        .await;
    let attached = desk
        .attach(
            &agent,
            an_org(MINE),
            id,
            Reference::parse(AttachmentKind::Trace, "0af7651916cd43dd8448eb211c80319c")
                .expect("a reference"),
            AttachmentLabel::try_from("slow").expect("a label"),
            at(1),
        )
        .await;

    assert_eq!(opened.err(), Some(IncidentError::ReadOnly));
    assert_eq!(attached.err(), Some(IncidentError::ReadOnly));
    assert_eq!(world.incidents.incidents().len(), 1);
    frozen(&world, 1);
}

#[tokio::test]
async fn spec_l5_4_an_agent_with_the_comment_scope_comments_as_itself_and_it_is_audited_with_the_jti()
 {
    use crate::agents::AgentScope;
    use crate::audit::Actor;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    let agent_id = world.keys.book.enrol(
        an_org(MINE),
        "bot",
        &[AgentScope::TelemetryRead, AgentScope::IncidentsComment],
        None,
    );

    let chronicle = desk
        .comment(
            &called_by("bot", &["incidents:comment"], "jti-7"),
            an_org(MINE),
            id,
            &hi(),
            false,
            at(1),
        )
        .await
        .expect("commented");

    let entries = world.incidents.timeline();
    assert_eq!(
        entries[1].author,
        Author::Agent {
            id: Some(agent_id),
            name: "triage".into(),
            on_behalf_of: None,
        }
    );
    assert_eq!(
        entries[1].change,
        Change::Comment {
            body: hi(),
            public: false
        }
    );
    assert_eq!(chronicle.author_label(&entries[1].author), "triage");
    let uses = world.keys.book.uses();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].1.actor, Actor::Agent(agent_id));
    assert_eq!(uses[0].1.action, Action::IncidentCommented);
    assert_eq!(uses[0].1.detail.get("jti"), Some(Some("jti-7")));
    assert_eq!(
        world.notebook.entries().len(),
        1,
        "only the person's opening"
    );
}

#[tokio::test]
async fn a_comment_without_the_scope_in_the_grant_or_in_the_token_is_refused_and_leaves_nothing() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    world
        .keys
        .book
        .enrol(an_org(MINE), "reader", &[AgentScope::TelemetryRead], None);
    world
        .keys
        .book
        .enrol(an_org(MINE), "bot", &[AgentScope::IncidentsComment], None);

    let not_granted = desk
        .comment(
            &everything("reader", "jti-1"),
            an_org(MINE),
            id,
            &hi(),
            false,
            at(1),
        )
        .await;
    let not_claimed = desk
        .comment(
            &called_by("bot", &["telemetry:read"], "jti-2"),
            an_org(MINE),
            id,
            &hi(),
            false,
            at(1),
        )
        .await;
    let resolve_scope_is_not_comment = desk
        .comment(
            &called_by("bot", &["incidents:resolve"], "jti-3"),
            an_org(MINE),
            id,
            &hi(),
            false,
            at(1),
        )
        .await;

    assert_eq!(not_granted.err(), Some(IncidentError::OutOfScope));
    assert_eq!(not_claimed.err(), Some(IncidentError::OutOfScope));
    assert_eq!(
        resolve_scope_is_not_comment.err(),
        Some(IncidentError::OutOfScope)
    );
    frozen(&world, 1);
}

#[tokio::test]
async fn an_agent_never_publishes_a_comment() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    world
        .keys
        .book
        .enrol(an_org(MINE), "bot", &[AgentScope::IncidentsComment], None);

    let refused = desk
        .comment(
            &called_by("bot", &["incidents:comment"], "jti-1"),
            an_org(MINE),
            id,
            &hi(),
            true,
            at(1),
        )
        .await;

    assert_eq!(refused.err(), Some(IncidentError::OutOfScope));
    frozen(&world, 1);
}

#[tokio::test]
async fn resolving_takes_the_resolve_scope_and_is_audited_once_with_the_jti() {
    use crate::agents::AgentScope;
    use crate::audit::Actor;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    let agent_id = world.keys.book.enrol(
        an_org(MINE),
        "bot",
        &[AgentScope::IncidentsComment, AgentScope::IncidentsResolve],
        None,
    );

    let without = desk
        .amend(
            &called_by("bot", &["incidents:comment"], "jti-1"),
            an_org(MINE),
            id,
            resolving(),
            at(1),
        )
        .await;
    frozen(&world, 1);
    let resolved = desk
        .amend(
            &called_by("bot", &["incidents:resolve"], "jti-2"),
            an_org(MINE),
            id,
            resolving(),
            at(2),
        )
        .await
        .expect("resolved");

    assert_eq!(without.err(), Some(IncidentError::OutOfScope));
    assert_eq!(resolved.incident.status, IncidentStatus::Resolved);
    let entries = world.incidents.timeline();
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[1].author,
        Author::Agent {
            id: Some(agent_id),
            name: "triage".into(),
            on_behalf_of: None,
        }
    );
    let uses = world.keys.book.uses();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].1.actor, Actor::Agent(agent_id));
    assert_eq!(uses[0].1.action, Action::IncidentResolved);
    assert_eq!(uses[0].1.detail.get("jti"), Some(Some("jti-2")));
}

#[tokio::test]
async fn an_agent_may_change_nothing_but_the_status_to_resolved() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    world.keys.book.enrol(
        an_org(MINE),
        "bot",
        &[AgentScope::IncidentsComment, AgentScope::IncidentsResolve],
        None,
    );
    let agent = everything("bot", "jti-1");
    let patches = [
        IncidentPatch {
            severity: Some(IncidentSeverity::Sev1),
            ..resolving()
        },
        IncidentPatch {
            owner: Some(Some(account(ME))),
            ..resolving()
        },
        IncidentPatch {
            owner: Some(None),
            ..resolving()
        },
        IncidentPatch {
            title: Some(IncidentTitle::try_from("Renamed").expect("a title")),
            ..resolving()
        },
        IncidentPatch {
            public: Some(true),
            ..resolving()
        },
        IncidentPatch {
            status: Some(IncidentStatus::Monitoring),
            ..IncidentPatch::default()
        },
        IncidentPatch {
            status: Some(IncidentStatus::Investigating),
            ..IncidentPatch::default()
        },
        IncidentPatch {
            severity: Some(IncidentSeverity::Sev1),
            ..IncidentPatch::default()
        },
        IncidentPatch::default(),
    ];

    for patch in patches {
        let refused = desk.amend(&agent, an_org(MINE), id, patch, at(1)).await;
        assert_eq!(refused.err(), Some(IncidentError::OutOfScope));
    }

    frozen(&world, 1);
    assert_eq!(
        world.incidents.incidents()[0].status,
        IncidentStatus::Investigating
    );
    assert_eq!(
        world.incidents.incidents()[0].severity,
        IncidentSeverity::Sev3
    );
}

#[tokio::test]
async fn a_project_bound_agent_acts_only_on_its_projects_incidents() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let ours = an_open_incident(&world, Some(a_project(OURS))).await;
    let unbound_incident = an_open_incident(&world, None).await;
    let both = [AgentScope::IncidentsComment, AgentScope::IncidentsResolve];
    world
        .keys
        .book
        .enrol(an_org(MINE), "bound", &both, Some(a_project(OURS)));
    world
        .keys
        .book
        .enrol(an_org(MINE), "other", &both, Some(a_project(77)));
    world.keys.book.enrol(an_org(MINE), "whole", &both, None);
    let bound = everything("bound", "jti-1");

    let other = everything("other", "jti-9");
    let elsewhere = desk
        .comment(&other, an_org(MINE), ours, &hi(), false, at(1))
        .await;
    let projectless = desk
        .comment(&bound, an_org(MINE), unbound_incident, &hi(), false, at(1))
        .await;
    let resolve_elsewhere = desk
        .amend(&other, an_org(MINE), ours, resolving(), at(1))
        .await;
    let resolve_projectless = desk
        .amend(&bound, an_org(MINE), unbound_incident, resolving(), at(1))
        .await;
    frozen(&world, 2);
    let own = desk
        .comment(&bound, an_org(MINE), ours, &hi(), false, at(2))
        .await;
    let whole_agent = desk
        .comment(
            &everything("whole", "jti-2"),
            an_org(MINE),
            unbound_incident,
            &hi(),
            false,
            at(2),
        )
        .await;

    assert_eq!(elsewhere.err(), Some(IncidentError::NoSuchIncident));
    assert_eq!(projectless.err(), Some(IncidentError::NoSuchIncident));
    assert_eq!(resolve_elsewhere.err(), Some(IncidentError::NoSuchIncident));
    assert_eq!(
        resolve_projectless.err(),
        Some(IncidentError::NoSuchIncident)
    );
    assert!(own.is_ok());
    assert!(whole_agent.is_ok());
    assert_eq!(world.keys.book.uses().len(), 2);
}

#[tokio::test]
async fn a_revoked_unknown_or_foreign_agent_is_refused_without_a_trace() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    let revoked = world.keys.book.enrol(
        an_org(MINE),
        "gone",
        &[AgentScope::IncidentsComment, AgentScope::IncidentsResolve],
        None,
    );
    world.keys.book.revoke_now(revoked);
    world.keys.book.enrol(
        an_org(THEIRS),
        "foreign",
        &[AgentScope::IncidentsComment, AgentScope::IncidentsResolve],
        None,
    );

    for client in ["gone", "nobody", "foreign"] {
        let agent = everything(client, "jti-1");
        let commented = desk
            .comment(&agent, an_org(MINE), id, &hi(), false, at(1))
            .await;
        let resolved = desk
            .amend(&agent, an_org(MINE), id, resolving(), at(1))
            .await;
        assert_eq!(commented.err(), Some(IncidentError::NoSession), "{client}");
        assert_eq!(resolved.err(), Some(IncidentError::NoSession), "{client}");
    }

    frozen(&world, 1);
}

#[tokio::test]
async fn resolving_an_already_resolved_incident_is_still_audited_once() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    world
        .keys
        .book
        .enrol(an_org(MINE), "bot", &[AgentScope::IncidentsResolve], None);
    let agent = called_by("bot", &["incidents:resolve"], "jti-1");
    desk.amend(&agent, an_org(MINE), id, resolving(), at(1))
        .await
        .expect("resolved");

    desk.amend(&agent, an_org(MINE), id, resolving(), at(2))
        .await
        .expect("a no-op");

    assert_eq!(world.incidents.timeline().len(), 2);
    assert_eq!(world.keys.book.uses().len(), 2);
}

#[tokio::test]
async fn spec_l7_4_a_delegated_comment_names_the_person_in_the_timeline_and_the_audit_entry() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    let agent_id =
        world
            .keys
            .book
            .enrol(an_org(MINE), "bot", &[AgentScope::IncidentsComment], None);

    desk.comment(
        &called_for(
            "bot",
            &["incidents:comment"],
            "jti-9",
            Some("dana@example.com"),
        ),
        an_org(MINE),
        id,
        &hi(),
        false,
        at(1),
    )
    .await
    .expect("commented");

    let entries = world.incidents.timeline();
    assert_eq!(
        entries[1].author,
        Author::Agent {
            id: Some(agent_id),
            name: "triage".into(),
            on_behalf_of: Some("dana@example.com".into()),
        }
    );
    let uses = world.keys.book.uses();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].1.detail.get("jti"), Some(Some("jti-9")));
    assert_eq!(uses[0].1.detail.get("on_behalf_of"), Some(Some("sub-dana")));
    assert_eq!(
        uses[0].1.detail.get("on_behalf_of_email"),
        Some(Some("dana@example.com"))
    );
}

#[tokio::test]
async fn spec_l7_4_a_delegated_resolve_without_an_email_names_the_subject() {
    use crate::agents::AgentScope;

    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = an_open_incident(&world, None).await;
    world
        .keys
        .book
        .enrol(an_org(MINE), "bot", &[AgentScope::IncidentsResolve], None);

    desk.amend(
        &called_for("bot", &["incidents:resolve"], "jti-3", None),
        an_org(MINE),
        id,
        resolving(),
        at(2),
    )
    .await
    .expect("resolved");

    let entries = world.incidents.timeline();
    assert!(matches!(
        &entries[1].author,
        Author::Agent { on_behalf_of: Some(person), .. } if &**person == "sub-dana"
    ));
    let uses = world.keys.book.uses();
    assert_eq!(uses[0].1.action, Action::IncidentResolved);
    assert_eq!(uses[0].1.detail.get("on_behalf_of"), Some(Some("sub-dana")));
    assert_eq!(uses[0].1.detail.get("on_behalf_of_email"), None);
}

#[tokio::test]
async fn spec_l7_an_incident_from_an_alert_tells_the_services_the_alert_concerns() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let rule = world
        .a_rule(an_org(MINE), AlertSeverity::Critical, AlertStatus::Firing)
        .await;
    let from_alert = desk
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
    let by_hand = an_open_incident(&world, None).await;

    let dossier = desk
        .get(&session(), an_org(MINE), from_alert.incident.id)
        .await
        .expect("read");
    let manual = desk
        .get(&session(), an_org(MINE), by_hand)
        .await
        .expect("read");

    assert_eq!(
        dossier.briefed.alert.map(|alert| alert.services),
        Some(vec![Box::<str>::from("checkout")])
    );
    assert_eq!(manual.briefed.alert, None);
}
