# 0028. Local passwords and the second factor leave the product

Status: accepted
Date: 2026-11-02

## Context
Two ways in stood side by side. Local passwords were the default
(`local_password` was `true`), argon2 hashed them, `/auth/sign-in` took an
address and a password, `/auth/reset/*` mailed a single-use token, and a TOTP
second factor guarded the result. OIDC stood beside them, complete and
optional.

The second way in made the first one a liability. Every password reset was
a mail path, every account created by a command line had a token to hand
over and a hash to keep locked, and every sign-in had to decide whether a
second factor stood in front of it. FerrisKey already owns all of that:
credentials, recovery, second factors and the realm they live in. Agents
already trust it (lot 5). Lot 6A made it the way in by default (ADR 0026,
ADR 0027).

## Decision
A person reaches FerrisTrace through a FerrisKey authorization code
exchanged at `/auth/oidc/callback`, and nothing else. No route, port,
adapter, column or command accepts, stores, hashes, mails or checks a
password or a second factor.

- `/auth/sign-in`, `/auth/password`, `/auth/reset/*` and `/auth/2fa/*` do
  not exist, with an identity provider configured or without one.
- `Issuer` is a newtype over the issuer url. `Issuer::Local`,
  `PasswordHasher`, `Argon2Hasher` and the argon2 dependency are gone, so is
  the `local_password` setting and every branch it guarded.
- `create-account` and `reset-password` are gone. `bootstrap --email` still
  founds the organisation and creates the account, prints no token, and
  says how that person signs in.
- Inviting a member creates the account when the address has none and
  returns that fact. It no longer hands out a token.
- `password_resets`, `auth_factors`, `recovery_codes`,
  `second_step_challenges` and `accounts.password_hash` are dropped by one
  migration. Its down step recreates them empty and cannot recover rows.
- The seal and `FERRISTRACE_SECRETS__AT_REST_KEY` stay: webhook secrets are
  sealed with them, and a secret written before this change still opens.
- Sessions keep their public id, expiry, listing and revocation (ADR 0021).
  Session tokens stay SHA-256 (ADR 0006).
- With no identity provider configured the server still starts, still
  ingests and still answers keys and agent tokens. `/auth/methods` answers
  `{"oidc":false}` and the sign-in screen says sign in is unavailable.

This supersedes ADR 0009 (an account is created locked by a reset token),
0010 (a chosen password is asked only for length), 0013 (a local identity is
its own account), 0019 (a second factor is answered behind a challenge) and
0020 (a TOTP secret is sealed at rest). It narrows ADR 0006: the argon2id
half goes, the session half stands.

## Alternatives rejected
- Keep local passwords behind a flag set to `false`. The code stays,
  stays tested and stays reachable by one environment variable, for a way in
  nobody is meant to use.
- Import existing passwords into FerrisKey. Hashes are not portable between
  the two, and a bulk import is a migration of somebody else's data. A
  person signs in through FerrisKey or not at all.
- Keep TOTP for deployments with no FerrisKey. That deployment has no way
  in at all, so a second step in front of it protects nothing.

## Consequences
- Gains: there is one place a credential lives and one place it is
  checked. The hashing path, the reset mail and the second-step state
  machine leave the tree.
- Costs: this is breaking. A deployment that relied on passwords has no way
  in until an administrator configures FerrisKey, and a person who had a
  password, a second factor or recovery codes here starts again there. The
  release takes a minor bump (ADR 0018).
- What becomes harder: signing in without an identity provider. There is no
  longer a break-glass password; the break-glass is FerrisKey's own admin
  console.
