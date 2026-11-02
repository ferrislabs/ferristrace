# 0009 — A new account is created locked

- Status: superseded by 0028
- Date: 2026-10-24

## Context

Local passwords were accepted from the moment the auth abstraction landed,
but no account could be created to use them. `INSERT INTO accounts` existed
only in the federated path, which provisions on first sign in, and in tests.
A deployment configured for local passwords could not let anybody in.

The obvious command is `create-account --email ... --password ...`. A
password passed that way lands in the shell history, in the process table
while the command runs, and in any shell transcript or CI log that captured
the invocation. It is also the operator choosing a password for somebody
else, who then has to be told it over some other channel.

The `accounts_carry_a_way_in` check constraint requires either a password
hash or an OIDC subject, so an account cannot simply be stored without one.

## Decision

`create-account --email <address>` stores the account with a hash of 32
random bytes that is never shown to anybody. No password produces it, so the
account exists and cannot be signed into. The command then mints a
single-use reset token and prints it.

The new account is entered through the reset path that already exists:
`POST /auth/reset/confirm` takes the token and the password the person
chooses, sets it, and ends every session. The operator never learns the
password.

`reset-password --email <address>` mints the same token for an account that
already exists. It is the break-glass path for a deployment with no mail
relay, where `/auth/reset/request` cannot deliver anything.

Both tokens last 24 hours rather than the hour a self-service reset gets,
because the operator usually hands this one over out of band.

`Argon2Hasher::locked` builds the unreachable hash the same way the decoy
is built, so the two cannot drift apart.

## Consequences

The bootstrap of a self-hosted deployment is four commands and no SQL:
`create-account`, then the reset confirm, then `create-org --owner`, then
`issue-key`.

The token is printed on stdout and the instructions on stderr, as
`issue-key` already does, so a pipeline captures the secret and nothing
else. It is still a secret on a terminal; that is the same exposure
`issue-key` carries and the same bar — shell access to the host.

Nothing enforces a minimum password length, here or on
`/auth/reset/confirm`. That is a real gap and it predates this decision.

`create-account` cannot be reached over the network, so there is still no
self-service sign up. For a self-hosted instance that is the intent. A
hosted deployment will need one, and it will need to decide who may create
an account before it can be written.
