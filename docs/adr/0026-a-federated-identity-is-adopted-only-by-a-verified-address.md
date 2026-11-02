# 0026. A federated identity is adopted only by a verified address

Status: accepted
Date: 2026-11-02

## Context
`ProvisionPolicy` is `{ auto_provision: bool, link_by_email: bool }`
(`crates/domain/src/identity/value_objects.rs:195-197`), and its `Default`
impl keeps `{ auto_provision: true, link_by_email: false }`
(`crates/domain/src/identity/value_objects.rs:202-205`). `PostgresAccounts::provision`
resolves a subject already linked first (`crates/postgres/src/accounts.rs:93-95`);
failing that, it looks the identity's address up in `accounts` only when
`policy.link_by_email`, and otherwise creates a new account when
`policy.auto_provision` (`crates/postgres/src/accounts.rs:102-121`).

Lot 6A makes federated sign-in the only way in. With `link_by_email: false`
the documented quick start breaks: `README.md:14` has the operator run
`ferristrace bootstrap --email you@example.com` first, which founds the
organisation and creates a locked account from that address. Their first
sign-in then resolves no linked subject and no address match, so it
provisions a *second* account that owns nothing, and the person who just
founded the organisation sees none of it. `deploy/README.md:264-268` works
around exactly this by telling the reader to sign in before bootstrapping —
the opposite of the order `README.md:14` teaches, and not a fix anyone
reaches by following the quick start as written.

So `link_by_email` has to go on for the federated path. But an address
alone is not proof of who holds it: if the realm never verified the
address, anybody who can obtain a token naming `you@example.com` would
inherit whichever account already holds that address, organisation
included. `ResolvedIdentity` already carries `email_verified`
(`crates/domain/src/identity/value_objects.rs:187-192`), populated from the
id token's `email_verified` claim, defaulted to `false` when the claim is
absent (`crates/infra/src/oidc.rs:154`,
`claims.email_verified().unwrap_or(false)`). It is measured, not assumed,
that FerrisKey 0.10.1 sends both `email` and `email_verified` on the id
token of a new confidential client, because `email` is a DEFAULT scope
there and both of its protocol mappers are `id.token.claim=true`.

## Decision
`ProvisionPolicy::default()` stays `{ auto_provision: true, link_by_email: false }`;
it still answers correctly for a local identity, which needs no trust
decision at all (ADR 0013). The server's own composition now asks for the
federated behaviour explicitly: `apps/server/src/main.rs:249-252`
constructs `ProvisionPolicy { auto_provision: true, link_by_email: true }`
rather than taking the default.

`PostgresAccounts::provision` only runs the address lookup when
`policy.link_by_email && identity.email_verified`
(`crates/postgres/src/accounts.rs:102`). A missing or `false` claim is
never read as verified — the oidc adapter's `unwrap_or(false)` already
guarantees that at the boundary, and the provisioning code repeats the
check rather than trusting the boolean's provenance. An identity that
fails either half of that condition gets its own account, linked fresh,
same as an address nobody has registered.

With this in place, `bootstrap --email a@b` followed by a first FerrisKey
sign-in as a verified `a@b` lands on the account `bootstrap` created,
owning the organisation it founded — the order the quick start already
teaches.

## Alternatives rejected
- Keep `link_by_email: false` and reverse the documented order, so the
  operator signs in before bootstrapping. The quick start
  (`README.md:14`) is where people start; teaching the opposite order
  there, and only there, is fragile and easy to miss, and `deploy/README.md`
  already shows what happens when the two docs disagree.
- Link on the address unconditionally. This is the takeover described
  above: an unverified address is just a claim, and `link_by_email` would
  hand over any account that address already owns.
- Make `bootstrap` take the FerrisKey subject instead of an address. An
  operator would have to read a uuid out of the FerrisKey console before
  founding anything, which is a harder first command than the one address
  the quick start already asks for.

## Consequences
- Gains: the documented bootstrap-then-sign-in order produces one
  account, not two, and an unverified address can never adopt an
  account it does not already prove it holds.
- Costs: provisioning now depends on a claim the identity provider
  controls; a FerrisKey realm that stops asserting `email_verified`
  (or asserts it falsely) silently drops back to "always provision a new
  account" for every address, with no error surfaced to the operator.
- What becomes harder: diagnosing "I signed in and got a second,
  empty account" now requires knowing not just that the address matched,
  but that the token also carried `email_verified: true` — a detail
  invisible from the sign-in screen.
