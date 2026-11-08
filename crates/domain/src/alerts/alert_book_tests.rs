use std::sync::Mutex;

use uuid::Uuid;

use super::alert_book::{AlertBook, PREVIEW_POINTS};
use super::culprits::Culprits;
use super::entities::{AlertEvent, Observation, RuleDraft, RulePatch};
use super::errors::{AlertError, BadRule, BadSilence, ProbeError};
use super::ports::{AlertChannels, SignalProbe};
use super::value_objects::{
    AlertSeverity, AlertStatus, ChannelId, ChannelName, Comparison, Condition, Definition,
    DefinitionParts, Destination, EventId, Labels, NoDataPolicy, RuleName, Signal, TraceMeasure,
    TraceSignal,
};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OTHER, OURS, OneProject, OneSession, Ring, THEIRS, a_project, an_org,
    scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::audit::{Action, TargetKind};
use crate::catalogue::ProjectId;
use crate::clock::UnixSeconds;
use crate::explore::Window;
use crate::identity::AccountId;
use crate::organisation::{OrgRole, Whoami};
use crate::telemetry::TenantId;
use crate::testing::{AlertLedger, Couriered, Notebook, OncallLedger};

const NOW: u64 = 1_700_000_000;

type Book<'a> = AlertBook<
    OneSession,
    MemberOfMine,
    &'a Ring,
    OneProject,
    AlertLedger,
    AlertLedger,
    AlertLedger,
    AlertLedger,
    OncallLedger,
    Notebook,
>;

fn book<'a>(role: OrgRole, ledger: &AlertLedger, notebook: &Notebook, keys: &'a Ring) -> Book<'a> {
    scheduled(role, ledger, notebook, keys, &OncallLedger::default())
}

fn scheduled<'a>(
    role: OrgRole,
    ledger: &AlertLedger,
    notebook: &Notebook,
    keys: &'a Ring,
    oncall: &OncallLedger,
) -> Book<'a> {
    AlertBook::new(
        Readers::new(Whoami::new(OneSession, MemberOfMine(role)), keys),
        OneProject,
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        ledger.clone(),
        oncall.clone(),
        notebook.clone(),
    )
}

fn definition(channels: Vec<ChannelId>) -> Definition {
    Definition::new(DefinitionParts {
        signal: Signal::Traces(TraceSignal {
            service: Some("checkout".into()),
            operation: None,
            environment: None,
            measure: TraceMeasure::ErrorRate,
            placement: crate::placement::Placement::default(),
        }),
        condition: Condition::new(Comparison::Above, 5.0).expect("a threshold"),
        window_secs: 300,
        pending_secs: 120,
        severity: AlertSeverity::Critical,
        no_data: NoDataPolicy::Ok,
        channels,
        page: None,
    })
    .expect("a definition")
}

fn draft(name: &str, project: Option<ProjectId>, channels: Vec<ChannelId>) -> RuleDraft {
    RuleDraft {
        name: RuleName::try_from(name).expect("a name"),
        project,
        enabled: true,
        definition: definition(channels),
    }
}

async fn a_channel(ledger: &AlertLedger, org: TenantId) -> ChannelId {
    ledger
        .create(
            org,
            &ChannelName::try_from("ops").expect("a name"),
            &Destination::parse(super::value_objects::ChannelKind::Email, "ops@example.com")
                .expect("an address"),
            None,
            AccountId::new(Uuid::from_u128(9)),
        )
        .await
        .expect("kept")
        .expect("fresh")
        .id
}

fn session() -> Reader {
    Reader::Session(token())
}

#[tokio::test]
async fn an_admin_creates_a_rule_and_the_log_records_it() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let channel = a_channel(&ledger, an_org(MINE)).await;

    let created = book(OrgRole::Admin, &ledger, &notebook, &keys)
        .create(
            &token(),
            an_org(MINE),
            &draft("Checkout errors", None, vec![channel]),
        )
        .await
        .expect("an admin writes rules");

    assert_eq!(created.rule.name.as_str(), "Checkout errors");
    assert_eq!(created.overall().status, AlertStatus::Ok);
    let entries = notebook.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].action, Action::AlertCreated);
    assert_eq!(entries[0].target.kind, TargetKind::Alert);
    assert_eq!(
        entries[0].target.id.as_deref(),
        Some(created.rule.id.to_string().as_str())
    );
    assert_eq!(entries[0].detail.get("name"), Some(Some("Checkout errors")));
}

#[tokio::test]
async fn a_member_reads_rules_but_does_not_write_them() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    book(OrgRole::Admin, &ledger, &notebook, &keys)
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created");
    let member = book(OrgRole::Member, &ledger, &notebook, &keys);

    let listed = member.list(&session(), an_org(MINE)).await.expect("a read");
    let refused = member
        .create(&token(), an_org(MINE), &draft("Two", None, Vec::new()))
        .await;
    let stranger = member
        .create(&token(), an_org(THEIRS), &draft("Two", None, Vec::new()))
        .await;

    assert_eq!(listed.len(), 1);
    assert_eq!(refused.err(), Some(AlertError::NotYoursToChange));
    assert_eq!(stranger.err(), Some(AlertError::NotYours));
    assert_eq!(notebook.entries().len(), 1);
}

#[tokio::test]
async fn a_project_bound_read_key_sees_only_its_projects_rules() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let ours = admin
        .create(
            &token(),
            an_org(MINE),
            &draft("Ours", Some(a_project(OURS)), Vec::new()),
        )
        .await
        .expect("created");
    let whole = admin
        .create(&token(), an_org(MINE), &draft("Whole", None, Vec::new()))
        .await
        .expect("created");
    let bound = Reader::Key(keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        scopes(&[KeyScope::Read]),
    ));
    let elsewhere = Reader::Key(keys.holding(an_org(THEIRS), None, scopes(&[KeyScope::Read])));

    let seen = admin.list(&bound, an_org(MINE)).await.expect("a read");
    let hidden = admin.get(&bound, an_org(MINE), whole.rule.id).await;
    let strange = admin.list(&elsewhere, an_org(MINE)).await;

    assert_eq!(
        seen.iter()
            .map(|watched| watched.rule.id)
            .collect::<Vec<_>>(),
        vec![ours.rule.id]
    );
    assert_eq!(hidden.err(), Some(AlertError::NoSuchRule));
    assert_eq!(strange.err(), Some(AlertError::NotYours));
}

#[tokio::test]
async fn a_rule_names_only_this_organisations_channels_and_projects() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let theirs = a_channel(&ledger, an_org(THEIRS)).await;
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);

    let channel = admin
        .create(&token(), an_org(MINE), &draft("One", None, vec![theirs]))
        .await;
    let project = admin
        .create(
            &token(),
            an_org(MINE),
            &draft("One", Some(a_project(OTHER)), Vec::new()),
        )
        .await;
    admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created");
    let twice = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await;

    assert_eq!(channel.err(), Some(AlertError::UnknownChannel));
    assert_eq!(project.err(), Some(AlertError::NoSuchProject));
    assert_eq!(twice.err(), Some(AlertError::NameTaken));
}

#[tokio::test]
async fn a_patch_is_merged_validated_and_audited_by_what_it_changed() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;

    let disabled = admin
        .update(
            &token(),
            an_org(MINE),
            id,
            RulePatch {
                enabled: Some(false),
                ..RulePatch::default()
            },
        )
        .await
        .expect("disabled");
    let raised = admin
        .update(
            &token(),
            an_org(MINE),
            id,
            RulePatch {
                condition: Some(Condition::new(Comparison::AtLeast, 9.0).expect("a threshold")),
                enabled: Some(true),
                ..RulePatch::default()
            },
        )
        .await
        .expect("raised");
    let refused = admin
        .update(
            &token(),
            an_org(MINE),
            id,
            RulePatch {
                window_secs: Some(10),
                ..RulePatch::default()
            },
        )
        .await;

    assert!(!disabled.rule.enabled);
    assert_eq!(disabled.rule.definition.window_secs(), 300);
    assert!(raised.rule.enabled);
    assert!((raised.rule.definition.condition().threshold() - 9.0).abs() < f64::EPSILON);
    assert_eq!(
        refused.err(),
        Some(AlertError::Rule(BadRule::Window {
            min: 60,
            max: 86_400
        }))
    );
    let actions: Vec<Action> = notebook
        .entries()
        .iter()
        .map(|entry| entry.action)
        .collect();
    assert_eq!(
        actions,
        vec![
            Action::AlertCreated,
            Action::AlertDisabled,
            Action::AlertUpdated,
            Action::AlertEnabled,
        ]
    );
}

#[tokio::test]
async fn a_deleted_rule_is_gone_and_its_deletion_is_audited() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;

    admin
        .delete(&token(), an_org(MINE), id)
        .await
        .expect("deleted");
    let again = admin.delete(&token(), an_org(MINE), id).await;

    assert_eq!(again.err(), Some(AlertError::NoSuchRule));
    assert_eq!(
        admin.get(&session(), an_org(MINE), id).await.err(),
        Some(AlertError::NoSuchRule)
    );
    assert_eq!(notebook.entries()[1].action, Action::AlertDeleted);
}

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;

#[tokio::test]
async fn an_admin_silences_a_rule_and_the_log_records_it() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;
    let until_ms = NOW * 1_000 + 4 * HOUR_MS;

    let watched = admin
        .silence(&token(), an_org(MINE), id, until_ms, UnixSeconds::new(NOW))
        .await
        .expect("an admin silences a rule");

    assert_eq!(watched.rule.silenced_until_ms, Some(until_ms));
    assert_eq!(notebook.entries()[1].action, Action::AlertSilenced);
}

#[tokio::test]
async fn an_admin_lifts_a_silence_early_and_the_log_records_it() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;
    admin
        .silence(
            &token(),
            an_org(MINE),
            id,
            NOW * 1_000 + 4 * HOUR_MS,
            UnixSeconds::new(NOW),
        )
        .await
        .expect("silenced");

    let watched = admin
        .unsilence(&token(), an_org(MINE), id)
        .await
        .expect("an admin lifts a silence");

    assert_eq!(watched.rule.silenced_until_ms, None);
    assert_eq!(notebook.entries()[2].action, Action::AlertUnsilenced);
}

#[tokio::test]
async fn a_member_cannot_silence_or_unsilence() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;
    let member = book(OrgRole::Member, &ledger, &notebook, &keys);

    let silenced = member
        .silence(
            &token(),
            an_org(MINE),
            id,
            NOW * 1_000 + 4 * HOUR_MS,
            UnixSeconds::new(NOW),
        )
        .await;
    let unsilenced = member.unsilence(&token(), an_org(MINE), id).await;

    assert_eq!(silenced.err(), Some(AlertError::NotYoursToChange));
    assert_eq!(unsilenced.err(), Some(AlertError::NotYoursToChange));
}

#[tokio::test]
async fn silencing_in_the_past_is_refused() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;

    let refused = admin
        .silence(
            &token(),
            an_org(MINE),
            id,
            NOW * 1_000 - 1,
            UnixSeconds::new(NOW),
        )
        .await;

    assert_eq!(
        refused.err(),
        Some(AlertError::Silence(BadSilence::InThePast))
    );
}

#[tokio::test]
async fn silencing_beyond_thirty_days_is_refused() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;

    let refused = admin
        .silence(
            &token(),
            an_org(MINE),
            id,
            NOW * 1_000 + 30 * DAY_MS + HOUR_MS,
            UnixSeconds::new(NOW),
        )
        .await;

    assert_eq!(
        refused.err(),
        Some(AlertError::Silence(BadSilence::TooLong { max_days: 30 }))
    );
}

#[tokio::test]
async fn the_history_reads_the_last_thirty_days_newest_first() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, Vec::new()))
        .await
        .expect("created")
        .rule
        .id;
    let now_ms = NOW * 1_000;
    for (at_ms, to) in [
        (now_ms - 31 * 86_400_000, AlertStatus::Firing),
        (now_ms - 120_000, AlertStatus::Pending),
        (now_ms - 60_000, AlertStatus::Firing),
    ] {
        ledger.put_event(AlertEvent {
            id: EventId::fresh(),
            rule: id,
            org: an_org(MINE),
            labels: Labels::none(),
            from: AlertStatus::Ok,
            to,
            value: Some(1.0),
            at_ms,
            context: None,
        });
    }

    let events = admin
        .history(
            &session(),
            an_org(MINE),
            id,
            None,
            None,
            UnixSeconds::new(NOW),
        )
        .await
        .expect("a history");

    assert_eq!(
        events.iter().map(|event| event.to).collect::<Vec<_>>(),
        vec![AlertStatus::Firing, AlertStatus::Pending]
    );
}

#[derive(Default)]
struct Curve {
    asked: Mutex<Vec<(TenantId, Option<ProjectId>, Window)>>,
}

impl SignalProbe for Curve {
    async fn observe(
        &self,
        org: TenantId,
        project: Option<ProjectId>,
        _signal: &Signal,
        window: Window,
    ) -> Result<Vec<Observation>, ProbeError> {
        self.asked
            .lock()
            .expect("the lock")
            .push((org, project, window));
        let minutes_ago = (NOW * 1_000_000_000 - window.to_unix_nanos()) / 60_000_000_000;
        let value = if minutes_ago <= 2 { 9.0 } else { 1.0 };
        Ok(vec![Observation {
            labels: Labels::none(),
            value: Some(value),
        }])
    }

    async fn culprits(
        &self,
        _org: TenantId,
        _project: Option<ProjectId>,
        _signal: &Signal,
        _window: Window,
        _labels: &Labels,
    ) -> Result<Culprits, ProbeError> {
        Ok(Culprits::default())
    }
}

#[tokio::test]
async fn a_preview_replays_the_rule_over_its_last_window() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let curve = Curve::default();
    let bound = Reader::Key(keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        scopes(&[KeyScope::Read]),
    ));

    let preview = book(OrgRole::Member, &ledger, &notebook, &keys)
        .preview(
            &bound,
            an_org(MINE),
            None,
            &definition(Vec::new()),
            &curve,
            UnixSeconds::new(NOW),
        )
        .await
        .expect("a preview");

    assert_eq!(
        preview.points.len(),
        usize::try_from(PREVIEW_POINTS).expect("small")
    );
    assert_eq!(preview.step_nanos, 10_000_000_000);
    assert_eq!(
        preview.points.last().map(|point| point.0),
        Some(NOW * 1_000_000_000)
    );
    assert_eq!(preview.points.last().and_then(|point| point.1), Some(9.0));
    assert_eq!(preview.points.first().and_then(|point| point.1), Some(1.0));
    assert!((preview.threshold - 5.0).abs() < f64::EPSILON);
    assert!(
        preview.would_fire,
        "two minutes of breach meet a two minute pending"
    );
    let asked = curve.asked.lock().expect("the lock");
    assert!(
        asked
            .iter()
            .all(|(org, project, window)| *org == an_org(MINE)
                && *project == Some(a_project(OURS))
                && window.to_unix_nanos() - window.from_unix_nanos() == 300_000_000_000)
    );
}

#[tokio::test]
async fn a_test_sends_to_every_channel_of_the_rule_and_reports_each() {
    let (ledger, notebook, keys) = (AlertLedger::default(), Notebook::default(), Ring::default());
    let channel = a_channel(&ledger, an_org(MINE)).await;
    let admin = book(OrgRole::Admin, &ledger, &notebook, &keys);
    let id = admin
        .create(&token(), an_org(MINE), &draft("One", None, vec![channel]))
        .await
        .expect("created")
        .rule
        .id;
    let (mailer, hooks) = (Couriered::default(), Couriered::default());

    let deliveries = admin
        .rehearse(
            &token(),
            an_org(MINE),
            id,
            &mailer,
            &hooks,
            UnixSeconds::new(NOW),
        )
        .await
        .expect("a test");
    let refused = book(OrgRole::Member, &ledger, &notebook, &keys)
        .rehearse(
            &token(),
            an_org(MINE),
            id,
            &mailer,
            &hooks,
            UnixSeconds::new(NOW),
        )
        .await;

    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].channel, channel);
    assert_eq!(deliveries[0].outcome, Ok(()));
    assert_eq!(mailer.sent().len(), 1);
    assert_eq!(&*mailer.sent()[0].0, "ops@example.com");
    assert_eq!(refused.err(), Some(AlertError::NotYoursToChange));
}

#[tokio::test]
async fn a_rule_pages_only_a_schedule_of_its_organisation() {
    let ledger = AlertLedger::default();
    let notebook = Notebook::default();
    let keys = Ring::default();
    let oncall = crate::oncall::fixtures::oncall();
    let book = scheduled(OrgRole::Admin, &ledger, &notebook, &keys, &oncall);
    let paging = |schedule| {
        let mut parts = definition(Vec::new()).into_parts();
        parts.page = Some(schedule);
        RuleDraft {
            definition: Definition::new(parts).expect("a definition"),
            ..draft("Paging", None, Vec::new())
        }
    };

    let unknown = book
        .create(
            &token(),
            an_org(MINE),
            &paging(crate::oncall::ScheduleId::fresh()),
        )
        .await;
    let kept = book
        .create(
            &token(),
            an_org(MINE),
            &paging(crate::oncall::fixtures::weekly().id),
        )
        .await
        .expect("a rule paging a schedule of its organisation");

    assert_eq!(unknown.map(|_| ()), Err(AlertError::UnknownSchedule));
    assert_eq!(
        kept.rule.definition.page(),
        Some(crate::oncall::fixtures::weekly().id)
    );
}
