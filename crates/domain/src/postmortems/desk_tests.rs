use uuid::Uuid;

use super::desk::PostmortemDesk;
use super::errors::{BadPostmortem, PostmortemError};
use super::value_objects::{MAX_BODY_BYTES, PostmortemBody, PostmortemStatus};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OURS, OneSession, Ring, THEIRS, a_project, an_org, scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::audit::{Action, TargetKind};
use crate::clock::UnixSeconds;
use crate::identity::AccountId;
use crate::incidents::{
    Author, Change, CommentBody, Incident, IncidentId, IncidentSeverity, IncidentStatus,
    IncidentTimeline, IncidentTitle, Person, TimelineEntry, TimelineId,
};
use crate::organisation::{OrgRole, Whoami};
use crate::testing::{AlertLedger, Crew, IncidentLedger, Notebook, PostmortemLedger};

const NOW: u64 = 1_700_000_000;
const ME: u128 = 9;
const STRANGER: u128 = 11;
const OPENED_MS: u64 = NOW * 1_000;

type Desk<'a> = PostmortemDesk<
    OneSession,
    MemberOfMine,
    &'a Ring,
    IncidentLedger,
    IncidentLedger,
    IncidentLedger,
    Crew,
    AlertLedger,
    PostmortemLedger,
    Notebook,
>;

struct World {
    alerts: AlertLedger,
    incidents: IncidentLedger,
    crew: Crew,
    notebook: Notebook,
    keys: Ring,
    postmortems: PostmortemLedger,
}

impl World {
    fn new() -> Self {
        let crew = Crew::default();
        crew.seat(an_org(MINE), account(ME), "person@example.com");
        crew.seat(an_org(THEIRS), account(STRANGER), "stranger@example.com");
        Self {
            alerts: AlertLedger::default(),
            incidents: IncidentLedger::default(),
            crew,
            notebook: Notebook::default(),
            keys: Ring::default(),
            postmortems: PostmortemLedger::default(),
        }
    }

    fn desk(&self, role: OrgRole) -> Desk<'_> {
        PostmortemDesk::new(
            Readers::new(Whoami::new(OneSession, MemberOfMine(role)), &self.keys),
            self.incidents.clone(),
            self.incidents.clone(),
            self.incidents.clone(),
            self.crew.clone(),
            self.alerts.clone(),
            self.postmortems.clone(),
            self.notebook.clone(),
        )
    }

    async fn an_incident(&self, resolved: bool, project: bool) -> IncidentId {
        let id = IncidentId::fresh();
        self.incidents.put(Incident {
            id,
            org: an_org(MINE),
            project: project.then(|| a_project(OURS)),
            title: IncidentTitle::try_from("Checkout down").expect("a title"),
            severity: IncidentSeverity::Sev2,
            status: if resolved {
                IncidentStatus::Resolved
            } else {
                IncidentStatus::Investigating
            },
            owner: None,
            public: false,
            opened_ms: OPENED_MS,
            resolved_ms: resolved.then_some(OPENED_MS + 600_000),
            opened_from_alert: None,
            paged_schedule: None,
        });
        for (offset, change) in [
            (
                0,
                Change::Opened {
                    title: IncidentTitle::try_from("Checkout down").expect("a title"),
                    severity: IncidentSeverity::Sev2,
                    alert: None,
                },
            ),
            (
                60_000,
                Change::Comment {
                    body: CommentBody::try_from("rolled back").expect("a body"),
                    public: false,
                },
            ),
        ] {
            IncidentTimeline::append(
                &self.incidents,
                &TimelineEntry {
                    id: TimelineId::fresh(),
                    incident: id,
                    org: an_org(MINE),
                    author: Author::Account(account(ME)),
                    change,
                    at_ms: OPENED_MS + offset,
                },
            )
            .await
            .expect("appended");
        }
        id
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

fn body(text: &str) -> PostmortemBody {
    PostmortemBody::try_from(text).expect("a body")
}

#[tokio::test]
async fn a_resolved_incident_drafts_with_its_window_and_every_entry() {
    let world = World::new();
    let id = world.an_incident(true, false).await;

    let written = world
        .desk(OrgRole::Member)
        .draft(&session(), an_org(MINE), id, false, at(5))
        .await
        .expect("drafted");

    let text = written.postmortem.body.as_str();
    assert!(text.contains(
        "- Window: 2023-11-14 22:13:20 UTC \u{2192} 2023-11-14 22:23:20 UTC (10 minutes)"
    ));
    assert!(text.contains("opened \"Checkout down\" as sev2"));
    assert!(text.contains("commented: \"rolled back\""));
    assert_eq!(written.postmortem.status, PostmortemStatus::Draft);
    assert!(!written.postmortem.edited);
    assert_eq!(written.postmortem.created_ms, (NOW + 5) * 1_000);
    assert_eq!(written.postmortem.published_ms, None);
    assert_eq!(
        written.author.as_ref().map(Person::label),
        Some("person@example.com")
    );
}

#[tokio::test]
async fn an_open_incident_drafts_with_a_still_open_window() {
    let world = World::new();
    let id = world.an_incident(false, false).await;

    let written = world
        .desk(OrgRole::Member)
        .draft(&session(), an_org(MINE), id, false, at(5))
        .await
        .expect("drafted");

    assert!(
        written
            .postmortem
            .body
            .as_str()
            .contains("\u{2192} still open")
    );
}

#[tokio::test]
async fn there_is_one_postmortem_per_incident_and_an_untouched_draft_is_regenerated() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = world.an_incident(false, false).await;
    let first = desk
        .draft(&session(), an_org(MINE), id, false, at(5))
        .await
        .expect("drafted");
    IncidentTimeline::append(
        &world.incidents,
        &TimelineEntry {
            id: TimelineId::fresh(),
            incident: id,
            org: an_org(MINE),
            author: Author::Account(account(ME)),
            change: Change::Comment {
                body: CommentBody::try_from("fixed it").expect("a body"),
                public: false,
            },
            at_ms: OPENED_MS + 120_000,
        },
    )
    .await
    .expect("appended");

    let second = desk
        .draft(&session(), an_org(MINE), id, false, at(50))
        .await
        .expect("regenerated");

    assert_eq!(world.postmortems.postmortems().len(), 1);
    assert_eq!(second.postmortem.id, first.postmortem.id);
    assert_eq!(second.postmortem.created_ms, first.postmortem.created_ms);
    assert_eq!(second.postmortem.updated_ms, (NOW + 50) * 1_000);
    assert!(second.postmortem.body.as_str().contains("fixed it"));
}

#[tokio::test]
async fn an_edited_draft_is_refused_unless_an_admin_forces_it() {
    let world = World::new();
    let member = world.desk(OrgRole::Member);
    let id = world.an_incident(true, false).await;
    member
        .draft(&session(), an_org(MINE), id, false, at(1))
        .await
        .expect("drafted");
    member
        .edit(&session(), an_org(MINE), id, &body("my own words"), at(2))
        .await
        .expect("edited");

    let plain = member
        .draft(&session(), an_org(MINE), id, false, at(3))
        .await;
    let member_forces = member
        .draft(&session(), an_org(MINE), id, true, at(3))
        .await;
    let forced = world
        .desk(OrgRole::Admin)
        .draft(&session(), an_org(MINE), id, true, at(4))
        .await
        .expect("regenerated");

    assert_eq!(plain.err(), Some(PostmortemError::Edited));
    assert_eq!(member_forces.err(), Some(PostmortemError::NotAdmin));
    assert!(forced.postmortem.body.as_str().starts_with("# Postmortem:"));
    assert!(!forced.postmortem.edited);
    assert_eq!(world.postmortems.postmortems().len(), 1);
}

#[tokio::test]
async fn publishing_is_for_admins_and_makes_the_postmortem_read_only() {
    let world = World::new();
    let member = world.desk(OrgRole::Member);
    let admin = world.desk(OrgRole::Admin);
    let id = world.an_incident(true, false).await;
    member
        .draft(&session(), an_org(MINE), id, false, at(1))
        .await
        .expect("drafted");

    let refused = member.publish(&session(), an_org(MINE), id, at(2)).await;
    let published = admin
        .publish(&session(), an_org(MINE), id, at(3))
        .await
        .expect("published");
    let edited = member
        .edit(&session(), an_org(MINE), id, &body("late"), at(4))
        .await;
    let redrafted = member
        .draft(&session(), an_org(MINE), id, false, at(4))
        .await;
    let forced = admin.draft(&session(), an_org(MINE), id, true, at(4)).await;
    let unrefused = member.unpublish(&session(), an_org(MINE), id, at(5)).await;
    let back = admin
        .unpublish(&session(), an_org(MINE), id, at(6))
        .await
        .expect("unpublished");
    let edited_again = member
        .edit(&session(), an_org(MINE), id, &body("now allowed"), at(7))
        .await
        .expect("edited");

    assert_eq!(refused.err(), Some(PostmortemError::NotAdmin));
    assert_eq!(published.postmortem.status, PostmortemStatus::Published);
    assert_eq!(published.postmortem.published_ms, Some((NOW + 3) * 1_000));
    assert_eq!(edited.err(), Some(PostmortemError::Published));
    assert_eq!(redrafted.err(), Some(PostmortemError::Published));
    assert_eq!(forced.err(), Some(PostmortemError::Published));
    assert_eq!(unrefused.err(), Some(PostmortemError::NotAdmin));
    assert_eq!(back.postmortem.status, PostmortemStatus::Draft);
    assert_eq!(back.postmortem.published_ms, None);
    assert_eq!(edited_again.postmortem.body.as_str(), "now allowed");
    assert!(edited_again.postmortem.edited);
}

#[tokio::test]
async fn reading_needs_a_postmortem_and_a_key_never_writes() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let id = world.an_incident(true, false).await;
    let key = Reader::Key(
        world
            .keys
            .holding(an_org(MINE), None, scopes(&[KeyScope::Read])),
    );

    let none_yet = desk.get(&key, an_org(MINE), id).await;
    let key_drafts = desk.draft(&key, an_org(MINE), id, false, at(1)).await;
    desk.draft(&session(), an_org(MINE), id, false, at(1))
        .await
        .expect("drafted");
    let key_edits = desk.edit(&key, an_org(MINE), id, &body("x"), at(2)).await;
    let key_publishes = world
        .desk(OrgRole::Admin)
        .publish(&key, an_org(MINE), id, at(2))
        .await;
    let read = desk.get(&key, an_org(MINE), id).await.expect("a read");
    let listed = desk.summaries(&key, an_org(MINE)).await.expect("a list");
    let unknown = desk
        .get(&session(), an_org(MINE), IncidentId::fresh())
        .await;
    let unknown_draft = desk
        .draft(&session(), an_org(MINE), IncidentId::fresh(), false, at(1))
        .await;
    let stranger = desk.get(&session(), an_org(THEIRS), id).await;
    let stranger_writes = desk
        .draft(&session(), an_org(THEIRS), id, false, at(1))
        .await;

    assert_eq!(none_yet.err(), Some(PostmortemError::NoSuchPostmortem));
    assert_eq!(key_drafts.err(), Some(PostmortemError::ReadOnly));
    assert_eq!(key_edits.err(), Some(PostmortemError::ReadOnly));
    assert_eq!(key_publishes.err(), Some(PostmortemError::ReadOnly));
    assert_eq!(read.postmortem.incident, id);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].incident, id);
    assert_eq!(unknown.err(), Some(PostmortemError::NoSuchIncident));
    assert_eq!(unknown_draft.err(), Some(PostmortemError::NoSuchIncident));
    assert_eq!(stranger.err(), Some(PostmortemError::NotYours));
    assert_eq!(stranger_writes.err(), Some(PostmortemError::NotYours));
}

#[tokio::test]
async fn a_project_bound_key_sees_only_its_projects_postmortems() {
    let world = World::new();
    let desk = world.desk(OrgRole::Member);
    let ours = world.an_incident(true, true).await;
    let whole = world.an_incident(true, false).await;
    for id in [ours, whole] {
        desk.draft(&session(), an_org(MINE), id, false, at(1))
            .await
            .expect("drafted");
    }
    let bound = Reader::Key(world.keys.holding(
        an_org(MINE),
        Some(a_project(OURS)),
        scopes(&[KeyScope::Read]),
    ));

    let seen = desk.summaries(&bound, an_org(MINE)).await.expect("a list");
    let hidden = desk.get(&bound, an_org(MINE), whole).await;
    let shown = desk.get(&bound, an_org(MINE), ours).await;

    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].incident, ours);
    assert_eq!(hidden.err(), Some(PostmortemError::NoSuchIncident));
    assert!(shown.is_ok());
}

#[test]
fn a_body_is_not_empty_and_not_beyond_two_hundred_kilobytes() {
    assert_eq!(
        PostmortemBody::try_from("  \n").err(),
        Some(BadPostmortem::Body {
            max: MAX_BODY_BYTES
        })
    );
    assert!(PostmortemBody::try_from("x".repeat(MAX_BODY_BYTES).as_str()).is_ok());
    assert!(PostmortemBody::try_from("x".repeat(MAX_BODY_BYTES + 1).as_str()).is_err());
}

#[tokio::test]
async fn every_postmortem_write_is_audited() {
    let world = World::new();
    let member = world.desk(OrgRole::Member);
    let admin = world.desk(OrgRole::Admin);
    let id = world.an_incident(true, false).await;
    member
        .draft(&session(), an_org(MINE), id, false, at(1))
        .await
        .expect("drafted");
    member
        .edit(&session(), an_org(MINE), id, &body("words"), at(2))
        .await
        .expect("edited");
    admin
        .publish(&session(), an_org(MINE), id, at(3))
        .await
        .expect("published");
    admin
        .unpublish(&session(), an_org(MINE), id, at(4))
        .await
        .expect("unpublished");
    let refused = member.publish(&session(), an_org(MINE), id, at(5)).await;

    let entries = world.notebook.entries();
    let actions: Vec<Action> = entries.iter().map(|entry| entry.action).collect();
    assert_eq!(
        actions,
        vec![
            Action::PostmortemDrafted,
            Action::PostmortemEdited,
            Action::PostmortemPublished,
            Action::PostmortemUnpublished,
        ]
    );
    assert!(refused.is_err());
    assert!(entries.iter().all(|entry| {
        entry.org == an_org(MINE)
            && entry.target.kind == TargetKind::Postmortem
            && entry.detail.get("incident") == Some(Some(id.to_string().as_str()))
    }));
    assert_eq!(entries[0].detail.get("regenerated"), Some(Some("false")));
    assert_eq!(entries[1].detail.get("bytes"), Some(Some("5")));
}
