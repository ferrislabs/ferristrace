# 0008 — Roles are granted from the command line

- Status: accepted
- Date: 2026-10-24

## Context

`org_members` and the `org_role` enum have existed since the control plane
landed, and nothing read or wrote them. No organisation could be created
either: `INSERT INTO orgs` appeared only in tests, so a fresh deployment held
no organisation, no member and therefore no role.

A read-only membership port would have answered the empty list forever. To
give the role something to govern, both ends are needed: somebody to grant it
and somebody to read it.

The reader is settled by what exists. `/v1/traces` authenticates an ingest
key, not a person, and `/auth/*` consults no role. The first thing that can
read a role is therefore the first endpoint that reads a session:
`GET /auth/me`.

The writer is the open question. An HTTP endpoint would be the obvious place,
but granting a role over HTTP has to be authorised, and authorising it means
answering "who may grant" before any organisation has an owner — the
bootstrap problem. The first owner cannot be appointed by a rule that reads
the table it is about to fill.

## Decision

Roles are granted by the operator from the command line:

- `ferristrace create-org --name <name> --owner <email>` creates the
  organisation and makes an existing account its owner, in one transaction.
- `ferristrace grant-role --org <id> --member <email> --role <role>` gives an
  existing account a role, replacing the one it already held.

Both require an account to exist already — they appoint, they do not invite.
Neither is reachable over the network.

`GET /auth/me` returns the caller's account, the organisation its session was
opened against, and every membership it holds. It is the only reader.

`OrgRole` carries the comparison the endpoints will need, `at_least`, so a
future check reads `who.may_act_in(org, OrgRole::Admin)` rather than matching
on three variants in three places.

## Consequences

The bootstrap needs shell access to the host, which is the right bar for
appointing the first owner of a self-hosted deployment and the same bar
`issue-key` already sets.

An invitation flow — an owner adding a member over HTTP — is still missing,
and it is the natural next step now that there is an owner to authorise it
and `may_act_in` to ask. When it lands, `grant-role` stays as the
break-glass path for an organisation whose owners have all left.

`create-org` cannot create the account it makes an owner. A deployment
using federated sign in provisions accounts on first sign in, so the
operator signs in once and then founds the organisation. A deployment using
local passwords has no account-creation path at all yet; that gap is real
and tracked separately.
