use uuid::Uuid;

use super::entities::{Rotation, ScheduleDraft, SchedulePatch};
use super::errors::{BadSchedule, OncallError};
use super::fixtures::{ADA, BOB, BOSS, ME, STRANGER, at, bench, crew, ms, oncall, person, team};
use super::rota::Rota;
use super::value_objects::{
    Day, Handoff, OverrideId, RotationLength, ScheduleId, ScheduleName, TimeOfDay, Timezone,
    Weekday,
};
use crate::access::key_ring_tests::{
    MINE, MemberOfMine, OneSession, Ring, THEIRS, an_org, scopes, token,
};
use crate::access::{KeyScope, Reader, Readers};
use crate::audit::{Action, Actor, TargetKind};
use crate::organisation::{OrgRole, TeamId, Whoami};
use crate::testing::{Bench, Crew, Notebook, OncallLedger};

type Desk<'a> = Rota<OneSession, MemberOfMine, &'a Ring, Bench, Crew, OncallLedger, Notebook>;

struct World {
    oncall: OncallLedger,
    notebook: Notebook,
    keys: Ring,
}

impl World {
    fn new() -> Self {
        Self {
            oncall: oncall(),
            notebook: Notebook::default(),
            keys: Ring::default(),
        }
    }

    fn rota(&self, role: OrgRole) -> Desk<'_> {
        Rota::new(
            Readers::new(Whoami::new(OneSession, MemberOfMine(role)), &self.keys),
            bench(),
            crew(),
            self.oncall.clone(),
            self.notebook.clone(),
        )
    }

    fn logged(&self, action: Action) -> Vec<crate::audit::AuditEntry> {
        self.notebook
            .entries()
            .into_iter()
            .filter(|entry| entry.action == action)
            .collect()
    }
}

fn draft(name: &str, people: &[u128]) -> ScheduleDraft {
    ScheduleDraft {
        name: ScheduleName::try_from(name).expect("a name"),
        timezone: Timezone::try_from("Europe/Paris").expect("a timezone"),
        rotation: Rotation::new(people.iter().map(|seed| person(*seed)).collect())
            .expect("a rotation"),
        handoff: Handoff {
            weekday: Weekday::Wednesday,
            time: TimeOfDay::try_from("10:30").expect("a time"),
        },
        length: RotationLength::try_from(14).expect("a length"),
        starts_on: Day::try_from("2026-11-04").expect("a day"),
    }
}

fn primary() -> ScheduleId {
    super::fixtures::weekly().id
}

#[tokio::test]
async fn an_administrator_creates_a_schedule_and_it_is_audited() {
    let world = World::new();

    let made = world
        .rota(OrgRole::Admin)
        .create(
            &token(),
            an_org(MINE),
            team(),
            &draft("Secondary", &[BOB, ADA]),
            at(0),
        )
        .await
        .expect("an administrator creates a schedule");

    assert_eq!(made.schedule.team, team());
    assert_eq!(made.schedule.rotation.people(), &[person(BOB), person(ADA)]);
    assert!(made.overrides.is_empty());
    let logged = world.logged(Action::ScheduleCreated);
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].actor, Actor::Account(person(ME)));
    assert_eq!(logged[0].target.kind, TargetKind::Schedule);
    assert_eq!(
        logged[0].target.id.as_deref(),
        Some(made.schedule.id.to_string().as_str())
    );
    assert_eq!(logged[0].detail.get("timezone"), Some(Some("Europe/Paris")));
}

#[tokio::test]
async fn members_read_schedules_but_only_administrators_write_them() {
    let world = World::new();

    let read = world
        .rota(OrgRole::Member)
        .list(&Reader::Session(token()), an_org(MINE), team(), at(0))
        .await
        .expect("a member reads");
    let written = world
        .rota(OrgRole::Member)
        .create(&token(), an_org(MINE), team(), &draft("Mine", &[ME]), at(0))
        .await;
    let removed = world
        .rota(OrgRole::Member)
        .delete(&token(), an_org(MINE), primary())
        .await;
    let elsewhere = world
        .rota(OrgRole::Owner)
        .create(
            &token(),
            an_org(THEIRS),
            team(),
            &draft("Theirs", &[]),
            at(0),
        )
        .await;

    assert_eq!(read.len(), 1);
    assert_eq!(written.map(|_| ()), Err(OncallError::NotYoursToChange));
    assert_eq!(removed, Err(OncallError::NotYoursToChange));
    assert_eq!(elsewhere.map(|_| ()), Err(OncallError::NotYours));
    assert!(world.notebook.entries().is_empty());
}

#[tokio::test]
async fn a_read_key_reads_schedules_of_its_organisation_only() {
    let world = World::new();
    let mine = Reader::Key(
        world
            .keys
            .holding(an_org(MINE), None, scopes(&[KeyScope::Read])),
    );
    let theirs = Reader::Key(
        world
            .keys
            .holding(an_org(THEIRS), None, scopes(&[KeyScope::Read])),
    );
    let rota = world.rota(OrgRole::Member);

    let read = rota.get(&mine, an_org(MINE), primary(), at(0)).await;
    let refused = rota.get(&theirs, an_org(MINE), primary(), at(0)).await;

    assert!(read.is_ok());
    assert_eq!(refused.map(|_| ()), Err(OncallError::NotYours));
}

#[tokio::test]
async fn a_rotation_is_made_of_the_team_and_a_name_is_unique_within_it() {
    let world = World::new();
    let rota = world.rota(OrgRole::Admin);

    let outsider = rota
        .create(
            &token(),
            an_org(MINE),
            team(),
            &draft("Ops", &[ME, BOSS]),
            at(0),
        )
        .await;
    let nowhere = rota
        .create(
            &token(),
            an_org(MINE),
            TeamId::new(Uuid::from_u128(999)),
            &draft("Ops", &[]),
            at(0),
        )
        .await;
    let twice = rota
        .create(
            &token(),
            an_org(MINE),
            team(),
            &draft("Primary", &[ME]),
            at(0),
        )
        .await;

    assert_eq!(outsider.map(|_| ()), Err(OncallError::NotInTeam));
    assert_eq!(nowhere.map(|_| ()), Err(OncallError::NoSuchTeam));
    assert_eq!(twice.map(|_| ()), Err(OncallError::NameTaken));
}

#[tokio::test]
async fn an_update_records_the_fields_it_changed_and_a_no_op_records_nothing() {
    let world = World::new();
    let rota = world.rota(OrgRole::Admin);

    let changed = rota
        .update(
            &token(),
            an_org(MINE),
            primary(),
            SchedulePatch {
                rotation: Some(Rotation::new(vec![person(BOB), person(ME)]).expect("a rotation")),
                length: Some(RotationLength::try_from(1).expect("a length")),
                ..SchedulePatch::default()
            },
            at(0),
        )
        .await
        .expect("updated");
    rota.update(
        &token(),
        an_org(MINE),
        primary(),
        SchedulePatch::default(),
        at(0),
    )
    .await
    .expect("a no-op");
    let outsider = rota
        .update(
            &token(),
            an_org(MINE),
            primary(),
            SchedulePatch {
                rotation: Some(Rotation::new(vec![person(BOSS)]).expect("a rotation")),
                ..SchedulePatch::default()
            },
            at(0),
        )
        .await;

    assert_eq!(changed.schedule.length.days(), 1);
    assert_eq!(
        world.oncall.schedules()[0].rotation.people(),
        &[person(BOB), person(ME)]
    );
    let logged = world.logged(Action::ScheduleUpdated);
    assert_eq!(logged.len(), 1);
    assert_eq!(
        logged[0].detail.get("fields"),
        Some(Some("rotation,length_days"))
    );
    assert_eq!(outsider.map(|_| ()), Err(OncallError::NotInTeam));
}

#[tokio::test]
async fn deleting_a_schedule_is_audited() {
    let world = World::new();

    world
        .rota(OrgRole::Admin)
        .delete(&token(), an_org(MINE), primary())
        .await
        .expect("deleted");
    let again = world
        .rota(OrgRole::Admin)
        .delete(&token(), an_org(MINE), primary())
        .await;

    assert!(world.oncall.schedules().is_empty());
    assert_eq!(again, Err(OncallError::NoSuchSchedule));
    let logged = world.logged(Action::ScheduleDeleted);
    assert_eq!(logged.len(), 1);
    assert_eq!(logged[0].detail.get("name"), Some(Some("Primary")));
}

#[tokio::test]
async fn an_override_hands_the_pager_to_a_member_and_is_audited_both_ways() {
    let world = World::new();
    let rota = world.rota(OrgRole::Admin);

    let cover = rota
        .add_override(
            &token(),
            an_org(MINE),
            primary(),
            person(BOSS),
            ms(0),
            ms(3_600),
        )
        .await
        .expect("an override");
    let now = rota
        .now(&Reader::Session(token()), an_org(MINE), primary(), at(60))
        .await
        .expect("who is on call");
    rota.remove_override(&token(), an_org(MINE), primary(), cover.id)
        .await
        .expect("removed");
    let after = rota
        .now(&Reader::Session(token()), an_org(MINE), primary(), at(60))
        .await
        .expect("who is on call");
    let gone = rota
        .remove_override(&token(), an_org(MINE), primary(), OverrideId::fresh())
        .await;

    assert_eq!(now.person.map(|person| person.account), Some(person(BOSS)));
    assert_eq!(now.until_ms, Some(ms(3_600)));
    let held = after.person.expect("the rotation");
    assert_eq!(held.account, person(ME));
    assert_eq!(
        held.email.map(|email| email.as_str().to_owned()).as_deref(),
        Some("me@example.com")
    );
    assert_eq!(gone, Err(OncallError::NoSuchOverride));
    assert_eq!(world.logged(Action::OverrideCreated).len(), 1);
    assert_eq!(world.logged(Action::OverrideDeleted).len(), 1);
    assert_eq!(
        world.logged(Action::OverrideCreated)[0]
            .detail
            .get("account"),
        Some(Some(person(BOSS).to_string().as_str()))
    );
}

#[tokio::test]
async fn an_override_goes_to_a_member_for_a_real_window() {
    let world = World::new();
    let rota = world.rota(OrgRole::Admin);

    let stranger = rota
        .add_override(
            &token(),
            an_org(MINE),
            primary(),
            person(STRANGER),
            ms(0),
            ms(60),
        )
        .await;
    let backwards = rota
        .add_override(
            &token(),
            an_org(MINE),
            primary(),
            person(ADA),
            ms(60),
            ms(60),
        )
        .await;
    let by_member = world
        .rota(OrgRole::Member)
        .add_override(&token(), an_org(MINE), primary(), person(ME), ms(0), ms(60))
        .await;

    assert_eq!(stranger.map(|_| ()), Err(OncallError::NotAMember));
    assert_eq!(
        backwards.map(|_| ()),
        Err(OncallError::Schedule(BadSchedule::Window))
    );
    assert_eq!(by_member.map(|_| ()), Err(OncallError::NotYoursToChange));
}

#[tokio::test]
async fn a_finished_override_is_no_longer_listed() {
    let world = World::new();
    let rota = world.rota(OrgRole::Admin);
    rota.add_override(
        &token(),
        an_org(MINE),
        primary(),
        person(ADA),
        ms(0),
        ms(60),
    )
    .await
    .expect("an override");

    let during = rota
        .get(&Reader::Session(token()), an_org(MINE), primary(), at(30))
        .await
        .expect("read");
    let after = rota
        .get(&Reader::Session(token()), an_org(MINE), primary(), at(60))
        .await
        .expect("read");

    assert_eq!(during.overrides.len(), 1);
    assert!(after.overrides.is_empty());
}

#[tokio::test]
async fn nobody_is_on_call_before_the_rotation_starts() {
    let world = World::new();

    let now = world
        .rota(OrgRole::Member)
        .now(
            &Reader::Session(token()),
            an_org(MINE),
            primary(),
            crate::clock::UnixSeconds::new(1),
        )
        .await
        .expect("asked");

    assert_eq!(now.person, None);
    assert_eq!(now.until_ms, None);
}

fn override_for(account: u128, from_s: u64, to_s: u64) -> super::entities::Override {
    super::entities::Override {
        id: OverrideId::fresh(),
        schedule: primary(),
        account: person(account),
        from_ms: ms(from_s),
        to_ms: ms(to_s),
    }
}

#[tokio::test]
async fn a_person_on_the_rotation_now_is_on_call_with_the_names_of_their_schedules() {
    let world = World::new();

    let status = world
        .rota(OrgRole::Member)
        .whether_on_call(
            &Reader::Session(token()),
            an_org(MINE),
            "me@example.com",
            at(0),
        )
        .await
        .expect("a member reads");

    assert!(status.on_call);
    assert_eq!(
        status.schedules,
        vec![ScheduleName::try_from("Primary").expect("a name")]
    );
}

#[tokio::test]
async fn a_person_whose_turn_is_not_now_is_not_on_call() {
    let world = World::new();

    let status = world
        .rota(OrgRole::Member)
        .whether_on_call(
            &Reader::Session(token()),
            an_org(MINE),
            "ada@example.com",
            at(0),
        )
        .await
        .expect("a member reads");

    assert!(!status.on_call);
    assert!(status.schedules.is_empty());
}

#[tokio::test]
async fn an_override_puts_its_person_on_call_for_its_window_only() {
    let world = World::new();
    crate::oncall::Schedules::add_override(
        &world.oncall,
        an_org(MINE),
        &override_for(BOB, 100, 200),
    )
    .await
    .expect("kept");
    let rota = world.rota(OrgRole::Member);
    let who = Reader::Session(token());

    let during = rota
        .whether_on_call(&who, an_org(MINE), "bob@example.com", at(150))
        .await
        .expect("read");
    let after = rota
        .whether_on_call(&who, an_org(MINE), "bob@example.com", at(250))
        .await
        .expect("read");
    let displaced = rota
        .whether_on_call(&who, an_org(MINE), "me@example.com", at(150))
        .await
        .expect("read");

    assert!(during.on_call);
    assert!(!after.on_call);
    assert!(!displaced.on_call);
}

#[tokio::test]
async fn an_unknown_or_malformed_address_is_not_on_call_and_the_case_does_not_matter() {
    let world = World::new();
    let rota = world.rota(OrgRole::Member);
    let who = Reader::Session(token());

    let unknown = rota
        .whether_on_call(&who, an_org(MINE), "nobody@example.com", at(0))
        .await
        .expect("read");
    let malformed = rota
        .whether_on_call(&who, an_org(MINE), "not an address", at(0))
        .await
        .expect("read");
    let shouted = rota
        .whether_on_call(&who, an_org(MINE), " ME@Example.COM ", at(0))
        .await
        .expect("read");

    assert_eq!(unknown, super::entities::OnCallStatus::default());
    assert_eq!(malformed, super::entities::OnCallStatus::default());
    assert!(shouted.on_call);
}

#[tokio::test]
async fn the_lookup_is_scoped_to_the_organisation_and_open_to_keys_and_agents() {
    use crate::agents::{AgentScope, VerifiedToken};

    let world = World::new();
    let rota = world.rota(OrgRole::Member);
    let key = Reader::Key(
        world
            .keys
            .holding(an_org(MINE), None, scopes(&[KeyScope::Read])),
    );
    world
        .keys
        .book
        .enrol(an_org(MINE), "gateway", &[AgentScope::TelemetryRead], None);
    let agent = Reader::Agent(crate::agents::AgentCall {
        token: VerifiedToken {
            client_id: "gateway".into(),
            scopes: vec!["telemetry:read".into()],
            jti: "jti-1".into(),
            expires_at_unix_secs: 4_000_000_000,
            delegated: None,
        },
        route: "/api/orgs/{org}/oncall/now".into(),
    });

    let foreign = rota
        .whether_on_call(
            &Reader::Session(token()),
            an_org(THEIRS),
            "me@example.com",
            at(0),
        )
        .await;
    let by_key = rota
        .whether_on_call(&key, an_org(MINE), "me@example.com", at(0))
        .await
        .expect("a read key reads");
    let by_agent = rota
        .whether_on_call(&agent, an_org(MINE), "me@example.com", at(0))
        .await
        .expect("an agent reads");

    assert_eq!(foreign.map(|_| ()), Err(OncallError::NotYours));
    assert!(by_key.on_call);
    assert!(by_agent.on_call);
    assert_eq!(world.keys.book.uses().len(), 1);
}

#[tokio::test]
async fn every_schedule_of_the_organisation_says_who_is_on_call_now_and_until_when() {
    let world = World::new();
    let rota = world.rota(OrgRole::Member);
    let who = Reader::Session(token());
    crate::oncall::Schedules::add_override(
        &world.oncall,
        an_org(MINE),
        &override_for(BOB, 100, 200),
    )
    .await
    .expect("kept");

    let plain = rota
        .duties(&who, an_org(MINE), at(0))
        .await
        .expect("a member reads");
    let covered = rota
        .duties(&who, an_org(MINE), at(150))
        .await
        .expect("a member reads");
    let foreign = rota.duties(&who, an_org(THEIRS), at(0)).await;

    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].schedule, primary());
    assert_eq!(plain[0].team, team());
    assert_eq!(
        plain[0].name,
        ScheduleName::try_from("Primary").expect("a name")
    );
    let held = plain[0].now.person.as_ref().expect("someone is on call");
    assert_eq!(held.account, person(ME));
    assert_eq!(
        held.email.as_ref().map(crate::identity::Email::as_str),
        Some("me@example.com")
    );
    assert!(plain[0].now.until_ms.is_some());
    let cover = covered[0].now.person.as_ref().expect("cover is on call");
    assert_eq!(cover.account, person(BOB));
    assert_eq!(foreign.map(|_| ()), Err(OncallError::NotYours));
}

#[tokio::test]
async fn a_schedule_nobody_staffs_is_listed_with_no_one_on_call() {
    let world = World::new();
    world
        .rota(OrgRole::Admin)
        .create(&token(), an_org(MINE), team(), &draft("Empty", &[]), at(0))
        .await
        .expect("created");

    let duties = world
        .rota(OrgRole::Member)
        .duties(&Reader::Session(token()), an_org(MINE), at(0))
        .await
        .expect("a member reads");

    let empty = duties
        .iter()
        .find(|duty| duty.name.as_str() == "Empty")
        .expect("listed");
    assert_eq!(empty.now.person, None);
    assert_eq!(empty.now.until_ms, None);
}
