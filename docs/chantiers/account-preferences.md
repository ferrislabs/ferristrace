# Account preferences and personal notifications

`web/src/pages/settings/Preferences.tsx` was entirely mock (local state, "Save" just
toasted). Making it real turned out to split into 8 independently shippable
workstreams. Decided with the user on 2026-11-19: build all 8, across sessions,
starting with 1 and 2.

## Decomposition

| # | Mission | Depends on | Status |
|---|---------|------------|--------|
| 1 | Display preferences (timezone, timestamp precision, row density, start page) persisted per account | none | **done** — merged |
| 2 | In-app notification center foundation (storage, API, dedicated page), empty, no producers yet | none | **done** — merged |
| 3 | Notification preferences storage (the "notify me when X" matrix) + email/digest toggles | 2 | **done** — merged (#299) |
| 4a | Wire member join/leave into the notification system | 2, 3 | **done** — merged (#307) |
| 4b | Wire status-page events into the notification system | 2, 3 | owned by Nathael, not this chantier's branch |
| 5 | New detector: a service stops sending spans (no detector exists today) | 2, 3 | set aside for now |
| 6 | New detector: a new exception/issue type is seen | 2, 3 | set aside for now |
| 7a | API key expiry: add the concept to the domain (TTL at creation) | 2, 3 | **done** — PR #309 open |
| 7b | API key expiry: auto-revoke + the 7d/1d pre-expiry notice | 7a | **done** — PR #310 open, stacked on #309 |
| 8 | Usage/billing threshold crossing (50/80/100%) + the job that checks it | 2, 3 | pending |

Full audit of what already existed before this chantier started (2026-11-19):

- Status page and member events: already in the audit trail
  (`Action::IncidentOpened/Updated/.../StatusPageCreated/...`,
  `Action::MemberInvited/MemberRemoved`) — straightforward to wire once 2+3 exist.
- No "new issue type seen" detector: `Issue.first_seen_unix_nanos` exists but is
  never diffed against a watermark.
- No "service stopped sending" detector: the `silent` pattern exists for infra
  resources (`ResourceSnapshot::settled()`, `SILENT_SECS`) but nothing equivalent
  watches span ingestion per service.
- No usage/billing threshold math exists anywhere, backend or frontend — `usage.ts`
  only has a linear month-end projection.
- A reusable scheduled-job pattern already exists: `ticking(period)` in
  `apps/server/src/main.rs` (used by `evaluate_alerts`, `deliver_alerts`,
  `escalate_pages`, `announce_status`, `follow_deploys`). Any new detector/digest
  job should use it the same way.
- No in-app notification center exists anywhere in the web app (no bell, no inbox).

## Workstream 1 — done

Merged (PR #293). Backend: `Preferences`/`AccountTimezone`/`TimestampPrecision`/
`RowDensity`/`StartPage` in `crates/domain/src/profile/`, two new `Accounts` port
methods (`preferences_of`/`set_preferences`), migration
`20261119000001_account_preferences` (4 nullable columns on `accounts`), new routes
`GET`/`PUT /auth/me/preferences`. Frontend: `web/src/data/profile.ts`
(`usePreferences`/`useSetPreferences`), `Preferences.tsx`'s Display card wired for
real, Theme stays `localStorage`-only (deliberate — see "Decisions" below).

Verified live in the browser: changed timezone + row density, saved, reloaded,
values persisted from Postgres.

`/settings/preferences` stays marked `mock: true` in `web/src/routes.tsx` — the
Notifications half of the page is still fake. Flip it to `real` only once
workstream 3/4 land.

## Workstream 2 — done

Branch `feat/notification-center-foundation`. New domain module
`crates/domain/src/inbox/` (`InboxDesk`, `InboxStore` port, `InboxItem`/
`InboxItemDraft`, `InboxItemKind` enum with all 6 future producer targets already
named: `Issue`, `ServiceSilent`, `StatusPage`, `ApiKeyExpiring`, `Usage`,
`Membership`). A notification belongs to one account in one org — every read/write
is scoped to both. Postgres: `inbox_items` table (migration
`20261120000001_inbox_items`), `PostgresInbox`. Application: `notifications`,
`mark_notification_read`, `mark_all_notifications_read`, and `notify` (the call
future producers use — nothing calls it yet). API:
`GET /api/orgs/{org}/notifications`,
`POST /api/orgs/{org}/notifications/{id}/read`,
`POST /api/orgs/{org}/notifications/read-all`. Frontend: `web/src/data/notifications.ts`,
a dedicated `/notifications` page (not a topbar bell — the user chose a page over a
popover), a "Notifications" entry with an unread-count badge in the Sidebar's
account menu.

90-day retention is modeled (`InboxStore::prune`, `INBOX_RETENTION_DAYS = 90`) but
**no job calls it yet** — wiring a prune tick onto the existing `ticking()` pattern
in `apps/server/src/main.rs` is for whoever adds the first real producer, since
there is nothing to prune until one exists.

Verified: 8 domain tests, 5 postgres (live), 6 api, all passing. Migration
confirmed live on the local stack (`\d inbox_items`); two test rows seeded directly
in Postgres for manual UI verification since the Chrome extension was unavailable
this session — nobody has visually confirmed the `/notifications` page renders yet.

## Workstream 3 — done

Branch `feat/notification-preferences-matrix` (PR #299). Domain:
`NotificationPreferences` (`email_enabled`, `weekly_digest`, a `BTreeMap<InboxItemKind,
Channels>`), `Channels { in_app, email }`, `InboxItemKind::default_channels()`
(`StatusPage`/`Membership` default in-app only, the rest both — matches the old
mock table), new `NotificationSettings` port (`of`/`set`). `InboxDesk` gained a
4th generic (`N: NotificationSettings`); `notify()` now reads the account's
preferences first and returns `Ok(None)` without writing anything when the kind
is turned off in-app. Postgres: 3 nullable columns on `accounts` (migration
`20261121000001_notification_preferences`), `PostgresInbox` also implements
`NotificationSettings`, channels stored as one `jsonb` column. API:
`GET`/`PUT /auth/me/notification-preferences`. Frontend:
`useNotificationPreferences`/`useSetNotificationPreferences` in
`web/src/data/notifications.ts`, `Preferences.tsx`'s Notifications section wired
for real (same draft/save pattern as the Display card).

Verified: 786 domain, 7 live postgres (two new: defaults, round-trip), api suite
green, `tsc -b` clean. Route existence checked live (`401` without a bearer, not
`404`/`405`). No dedicated HTTP-level test for the two new routes — the JSON body
mirrors the already-tested `PreferencesBody` pattern and the business logic is
covered at the domain layer; see the PR body. Not visually confirmed in a browser
this session (Chrome extension unavailable throughout).

## Workstream 4a — done, 4b out of scope here

Branch `feat/notify-admins-on-membership-change` (PR #307). Status-page events
(the other half of the original workstream 4) are owned by Nathael separately
and were dropped from this chantier's scope — not touched here.

Domain: `Roster::invite`/`remove` now also return `Vec<AccountId>` — the other
admins/owners to notify, computed from the same `Roll::of(org)` read they
already needed, excluding the acting admin and the member the action was
about (never both, that's the whole point). A private `admins_to_notify`
helper does the filtering; no new type, no new generics on `Roster` itself.
Application: `invite_member`/`remove_member` call the existing
`Ferristrace::notify` (built by workstream 3 "for a future producer", first
one to use it) once per recipient via a small `tell_admins` helper - best
effort, a failed notify never undoes the member change, since the postgres
adapter already logs the cause downstream. Both app functions now require
`InboxRepos` alongside `RosterRepos`, which rippled into `roster_router`'s
bound and into `crates/api/tests/roster_routes.rs`'s own `Fakes` (needed a
new `Notices` fake implementing `InboxStore`/`NotificationSettings`, same
"NoInbox"-style pattern workstream 3 used elsewhere).

Verified: 32 domain roster tests (including the new recipient-filtering
ones), 17 api roster-route tests (including two asserting an admin's inbox
actually receives the item), full domain/app/api suite green, clippy/fmt
clean. Route existence checked live (401/415 without auth, not 404/405).
No new migration, no postgres changes.

## Workstream 5, 6 — set aside

Both need a genuinely new concept (a watermark/detection mechanism - no
close precedent in the codebase, see the audit above) rather than wiring an
existing event. The user chose not to build them in this pass; left pending
in the table above for whoever picks the chantier back up.

## Workstream 7a — done

Branch `feat/api-key-expiry-ttl` (PR #309). `ApiKey` gained
`expires_at_unix_nanos: Option<u64>`; a new `KeyTtl` enum (`Days30`, `Days90`,
`Days365`, `Never`) is chosen at creation only - no edit route, per the
question batch. `KeyRing::issue`/`ApiKeys::issue` take the `ttl`; the postgres
adapter derives `expires_at` from the *same* `clock_timestamp()` as
`created_at` in one `WITH stamp AS (...)` insert, so the two never drift.
Migration `20261123000001_api_key_expiry` (one nullable column). Frontend:
a `ttl` select on the create-key dialog, an "Expires" column in the list.

Decided before building (question batch): auto-revoke at the deadline
(not just a warning) and notify the key's creator, falling back to org
admins/owners when `created_by` is `None` - both carried into 7b.

Verified: 816 domain, 8 live postgres (TTL round-trip, never-expires case),
full api/application suite, `tsc -b` clean.

## Workstream 7b — done

Branch `feat/api-key-expiry-job` (PR #310, stacked on #309 - not yet merged
when this was built). An hourly sweep (`Duty::ExpireKeys`, the same
advisory-lock pattern as `evaluate_alerts`/`escalate_pages`/etc.) revokes a
key once `expires_at` passes and raises a notice at 7 days and 1 day out.
Domain: `ApiKeys::due_for_expiry_notice`/`due_for_auto_revoke`/
`mark_expiry_notice_sent` (default no-op bodies on the trait, so none of the
~20 fakes elsewhere needed updating - only the real postgres adapter and this
workstream's own test fakes implement them for real); a new `Expiry<K, R, D,
A>` desk decides *who* to notify (creator, else `Roll::of(org)` filtered to
admins) and writes the `Action::KeyRevoked` audit entry as `Actor::System`.
Application: `Ferristrace::expire_keys` loops the result through the
existing `notify`, best effort, same as 4a. Migration
`20261124000001_api_key_expiry_notices` adds two dedup booleans
(`notified_7d`/`notified_1d`) *and* extends the `alert_duties_duty_check`
constraint to admit `'expire_keys'` - caught live, not by any test, since no
test exercises that table's check constraint.

Caught by the live postgres suite, not by domain tests: the test harness's
`forward!(ApiKeys: ApiKeys { ... })` macro in
`crates/postgres/tests/backend/mod.rs` must list every port method by name -
the three new ones were silently falling through to the trait's no-op
defaults until added there.

Verified: 820 domain (4 new, covering revoke/notify/fallback/one-notice-per-slot),
12 live postgres (4 new), full suite green. Live: migration applies cleanly,
job starts without error (confirmed only after the check-constraint fix above).

## Decisions

- Theme is **not** moved to the backend. It's a per-browser cosmetic setting today
  (`web/src/lib/theme.tsx`, `localStorage`), and the page's "Only for you, on every
  device" subtitle technically overclaims for it — not fixed here, out of scope for
  workstream 1, flagged for whoever picks up workstream 2+ to decide whether it's
  worth moving once the preferences endpoint exists for other reasons anyway.
- Preferences got their **own** `GET`/`PUT /auth/me/preferences` endpoint rather
  than being folded into `Who`/`/auth/me`/`Session`. Threading them through
  `Session` (used everywhere in auth) for 4 cosmetic fields was a much larger
  blast radius for no real benefit — the settings page already fetches its own
  resource, same shape as `useOrgCard()`.
- `AccountTimezone` duplicates `OrgTimezone`'s validation (both wrap
  `jiff::tz::TimeZoneDatabase::bundled()`) rather than sharing a type — two
  independent occurrences, not a third; see `rust-dev`'s "don't abstract before
  the third occurrence."
- No domain-level error type was added for precision/density/start_page — they
  parse at the API boundary (`TimestampPrecision::parse(&str) -> Option<Self>`,
  same pattern as `ResourceKind::parse`/`AlertStatus::parse` elsewhere), 422 on
  `None`, mirroring the existing `Malformed`/`Misplaced` convention.

## Frozen contracts (for workstream 8, and 5/6 if they're picked back up)

- `InboxDesk::notify(draft, now_ms) -> Result<Option<InboxItem>, InboxError>`
  at the domain layer, or `Ferristrace::notify(account, org, kind, title, body,
  link)` at the application layer (what 4a and 7b both called) — a producer
  calls one of these and gets `None`/best-effort-skipped back when the
  account suppressed that kind in-app. Every future job goes through one of
  these, never `InboxStore::create` directly, or preferences are bypassed.
- A producer that needs "who else to tell" as a business rule computes that
  list in the domain layer next to the data it already reads (4a's
  `admins_to_notify`, 7b's `Expiry::recipients_of`), and hands plain
  `Vec<AccountId>` up to the application layer to loop over with `notify` -
  not the other way around.
- A background job that must run on only one replica claims
  `AlertDuty::lead(Duty::X, slot)` first (`slot = now.get() / tick_secs`,
  same helper `ticking()` in `apps/server/src/main.rs`), same as every
  existing job - a new `Duty` variant needs its postgres lock constant in
  `crates/postgres/src/alerts.rs::lock_of` AND a slot in the
  `alert_duties_duty_check` constraint (easy to forget - 7b's stack job
  caught it, no unit test will).
- `InboxItemKind::{Issue, ServiceSilent, StatusPage, ApiKeyExpiring, Usage,
  Membership}` and their spellings (`as_str`/`parse`) are fixed — no new
  variants without updating the preferences matrix and the frontend's
  `LABELS` map in `Preferences.tsx`.
- `StatusPage` is Nathael's to wire, not part of this chantier's remaining
  branches.
- Workstream 8 still needs usage/billing threshold math — nothing added here.

Whoever picks up 8 (or 5/6) should still write its own mini-spec per
`spec-driven` - this section only freezes what 3, 4a and 7a/7b already
decided.

## Verify (workstream 1)

```
cargo fmt --check
cargo clippy -p ferristrace-domain -p ferristrace-application -p ferristrace-postgres -p ferristrace-api --all-targets -- -D warnings
cargo test -p ferristrace-domain -p ferristrace-application -p ferristrace-api profile
cargo test -p ferristrace-postgres --test auth   # needs DATABASE_URL, see memory smart-app-control-blocks-local-cargo
cd web && npx tsc --noEmit -p .
```
