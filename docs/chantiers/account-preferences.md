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
| 3 | Notification preferences storage (the "notify me when X" matrix) + email/digest toggles | 2 | **done** — PR #299 open |
| 4 | Wire status-page events and member join/leave into the notification system | 2, 3 | pending |
| 5 | New detector: a service stops sending spans (no detector exists today) | 2, 3 | pending |
| 6 | New detector: a new exception/issue type is seen | 2, 3 | pending |
| 7 | API key expiry: add the concept to the domain, then a detector | 2, 3 | pending |
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
- API keys have **no expiry/TTL field at all** in the domain model — must be added
  before workstream 7 can do anything.
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

## Frozen contracts (for workstreams 4+)

- `InboxDesk::notify(draft, now_ms) -> Result<Option<InboxItem>, InboxError>` —
  a producer calls this and gets `None` back when the account suppressed that
  kind in-app. Every future detector (5, 6, 7, 8) goes through this, never
  `InboxStore::create` directly, or preferences are bypassed.
- `InboxItemKind::{Issue, ServiceSilent, StatusPage, ApiKeyExpiring, Usage,
  Membership}` and their spellings (`as_str`/`parse`) are fixed — workstream 4's
  wiring and 5-8's new detectors raise exactly these kinds, no new variants
  without updating the preferences matrix and the frontend's `LABELS` map in
  `Preferences.tsx`.
- Workstream 7 still needs an API key expiry field on the domain before it can
  raise `ApiKeyExpiring` — nothing added here.
- Workstream 8 still needs usage/billing threshold math — nothing added here.

Whoever picks up 4+ should still write its own mini-spec per `spec-driven`
where it introduces something new (a detector, a job) — this section only
freezes what workstream 3 already decided.

## Verify (workstream 1)

```
cargo fmt --check
cargo clippy -p ferristrace-domain -p ferristrace-application -p ferristrace-postgres -p ferristrace-api --all-targets -- -D warnings
cargo test -p ferristrace-domain -p ferristrace-application -p ferristrace-api profile
cargo test -p ferristrace-postgres --test auth   # needs DATABASE_URL, see memory smart-app-control-blocks-local-cargo
cd web && npx tsc --noEmit -p .
```
