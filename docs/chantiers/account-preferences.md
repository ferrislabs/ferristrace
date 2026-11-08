# Account preferences and personal notifications

`web/src/pages/settings/Preferences.tsx` was entirely mock (local state, "Save" just
toasted). Making it real turned out to split into 8 independently shippable
workstreams. Decided with the user on 2026-11-19: build all 8, across sessions,
starting with 1 and 2.

## Decomposition

| # | Mission | Depends on | Status |
|---|---------|------------|--------|
| 1 | Display preferences (timezone, timestamp precision, row density, start page) persisted per account | none | **done** — PR open, see below |
| 2 | In-app notification center foundation (storage, API, bell/inbox UI), empty, no producers yet | none | pending |
| 3 | Notification preferences storage (the "notify me when X" matrix) + email/digest toggles | 2 | pending |
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

Branch `feat/account-preferences-display`. Backend: `Preferences`/`AccountTimezone`/
`TimestampPrecision`/`RowDensity`/`StartPage` in `crates/domain/src/profile/`, two new
`Accounts` port methods (`preferences_of`/`set_preferences`), migration
`20261119000001_account_preferences` (4 nullable columns on `accounts`), new routes
`GET`/`PUT /auth/me/preferences`. Frontend: `web/src/data/profile.ts`
(`usePreferences`/`useSetPreferences`), `Preferences.tsx`'s Display card wired for
real, Theme stays `localStorage`-only (deliberate — see "Decisions" below).

Verified live in the browser: changed timezone + row density, saved, reloaded,
values persisted from Postgres.

`/settings/preferences` stays marked `mock: true` in `web/src/routes.tsx` — the
Notifications half of the page is still fake. Flip it to `real` only once
workstream 3/4 land.

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

## Frozen contracts (for workstreams 2+)

Nothing is frozen yet for workstream 2 onward — it hasn't been designed. Whoever
picks this up next should write its own mini-spec (in-app notification storage
shape, bell/inbox UI, how a "read" state works) before touching code, per
`spec-driven`.

## Verify (workstream 1)

```
cargo fmt --check
cargo clippy -p ferristrace-domain -p ferristrace-application -p ferristrace-postgres -p ferristrace-api --all-targets -- -D warnings
cargo test -p ferristrace-domain -p ferristrace-application -p ferristrace-api profile
cargo test -p ferristrace-postgres --test auth   # needs DATABASE_URL, see memory smart-app-control-blocks-local-cargo
cd web && npx tsc --noEmit -p .
```
