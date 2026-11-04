# 0021. A session is named by a public id

Status: superseded by 0031
Date: 2026-10-27

## Context
`sessions` is keyed by `hash`, the sha256 digest of the token the browser
carries (ADR 0006). That was enough while the only questions asked of the
table were "does this token still open" and "revoke this token", both of
which the caller answers with the token it already holds.

The security screen asks a different question: show me every session this
account keeps, and end that one. The browser has to name a session it does
not hold the token for, which the current schema cannot do. The only
identifier a row has is the digest of someone's live credential.

A digest is not reversible, so handing it out does not leak a token. It is
still the lookup key for the only table that decides whether a request is
authenticated, and it would travel in a JSON body, through a browser cache,
into a frontend log, and past whoever is reading over a shoulder. A value
that appears in a `WHERE` clause guarding authentication should not also be
an element of a public list.

The screen also wants to say which device a row is, and the table records
nothing but timestamps.

## Decision
`sessions` gains two columns:

```sql
ALTER TABLE sessions ADD COLUMN id uuid NOT NULL DEFAULT gen_random_uuid();
ALTER TABLE sessions ADD COLUMN user_agent text;
CREATE UNIQUE INDEX sessions_by_id ON sessions (id);
```

`id` is a v4 uuid minted at issue, carries no secret and is the only name a
session has outside the server. `hash` stays the primary key and stays the
only thing a token resolves against, so authentication is unchanged. Every
statement that revokes by `id` is also scoped by `account_id`, which is what
stops one account naming another's session.

`user_agent` holds the header as sent, trimmed and capped at 256 bytes by the
`Agent` newtype, and is nullable because a client need not send one. The
server stores the string and does not parse it: "Firefox on macOS" is a
rendering, not a fact, and it belongs where it can be corrected without a
migration. The browser renders it.

Listing and revoking live behind a second port rather than on `SessionStore`:

```rust
pub trait SessionRoll {
    fn live(&self, account: AccountId, current: &SessionToken) -> ... Vec<LiveSession>;
    fn revoke_one(&self, account: AccountId, id: SessionId) -> ... bool;
    fn revoke_others(&self, account: AccountId, current: &SessionToken) -> ... u64;
}
```

`SessionStore` is a dependency of eleven use cases that will never list a
session, and ADR 0019 already settled that shape for the second factor: two
ports, one adapter. `PostgresSessions` implements both.

`live` and `revoke_others` take the current token because the adapter is the
only place allowed to hash it: the first marks the caller's own row, the
second spares it.

No ip address and no geolocation. Behind the reverse proxy a self-hosted
deployment usually sits behind, the address recorded is the proxy's, and a
wrong city on a security screen is worse than no city. It is also personal
data this feature does not need.

## Alternatives rejected
- Name a session by its digest. Reversible to nothing, but it makes the
  authentication key a list element, and a leaked list then lets anyone who
  can reach the endpoint revoke sessions by guessing nothing.
- Name a session by `created_at`. Not unique, and a client that revokes by
  timestamp races the row it means to close.
- Put `live` and the two revocations on `SessionStore`. Every use case that
  only resolves a token would carry three methods it never calls, and every
  test double in the workspace would grow three stubs.
- Parse the user agent on the server into a browser and an operating system.
  The parse is a guess that ages with every browser release; doing it in the
  browser means correcting it is a front-end change, not a migration.
- Reuse `ResetStore::revoke_sessions` for "sign out other sessions". It
  revokes every row for the account including the caller's, which is right
  after a password change and wrong here.

## Consequences
- Gains: the security screen can list and end sessions, and a session records
  where it was opened. `sessions_by_id` is unique, so a revocation by id
  touches one row or none.
- Costs: `SessionStore::issue` gains an `Option<&Agent>`, which ripples to
  `SignIn::begin`, `SecondStep::verify`, `FederatedSignIn::finish` and to
  every test double. One migration on a live table, which is additive and
  backfills with a default.
- What becomes harder: nothing reads `user_agent` server side, so a rule that
  needs to (refusing a new device, say) has to parse it after all, and would
  be better served by a column it writes itself.
- What becomes easier: a session is addressable. Admin-side revocation, a
  device-named audit trail and an email on a new sign-in all have something to
  point at that is not a credential.
