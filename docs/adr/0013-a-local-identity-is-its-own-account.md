# 0013 — A local identity is its own account

- Status: superseded by 0028
- Date: 2026-10-24

## Context

Signing in with a local password had never worked through the real
composition, and nothing could have noticed.

`SignIn::begin` asks the provider to resolve an identity, then asks the
account directory to provision it. `LocalPasswordProvider` resolves
`Issuer::Local` with the account's own id as the subject. `PostgresAccounts`
then looked for an `identity_links` row for that pair, found none — local
sign-ins never wrote one — and fell through to the provisioning rules.
`ProvisionPolicy::default()` carries `link_by_email: false` and
`auto_provision: true`, so it tried to insert a **second** account for an
address the first one already held, hit the unique index on `email`, and
answered 503.

Every test passed. `LocalPasswordProvider::sign_in` was tested on its own
and is correct. The HTTP sign-in test uses a fake account directory that
answers with an account. Nothing put the real provider and the real
directory in the same sentence until a stack came up in CI and a shell
script tried to sign in.

## Decision

A local identity is resolved by its subject, which **is** the account id.
`PostgresAccounts::provision` answers it by confirming that account exists
and returns it. It writes no `identity_links` row and consults no
provisioning policy.

The alternative was setting `link_by_email: true`, which would have
worked. It was wrong: that flag answers a question of trust — whether an
identity provider asserting an email may take over an existing account —
and it would have had to be answered yes for the federated provider too,
because the policy is one value shared by both sources. A local subject
needs no trust decision at all, because the provider found the account by
its password before saying its name.

## Consequences

A local account that no longer exists refuses sign-in as
`NotProvisioned`, which the directory maps to `Rejected`, so the caller
is told the credentials match nothing rather than told the store broke.
That is the right answer for a deleted account.

`link_by_email` now applies only to federated sign-in, which is the only
place the question it answers makes sense. Its default stays `false`.

Three tests put the real provider, the real directory and the real session
store together: a correct password opens a session and leaves exactly one
account; a wrong one opens nothing and still leaves one; and a local
identity naming an account nobody holds provisions nothing. That shape of
test — the whole composition against a real database — is what was
missing, not more cases.
