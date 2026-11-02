# 0019. A second factor is answered behind a challenge

Status: accepted
Date: 2026-10-26

## Context
`SignIn::begin` resolves a proof into an identity, provisions an account and
returns a `SessionToken`, in that order, with nothing in between
(`crates/core/src/signin.rs`). A correct password is therefore a session, and
the only way to add a second step would be to let the route ask for one after
the fact, holding a session it is supposed to withhold.

The settings screen already offers two factors and ten recovery codes, so the
domain has to answer three questions at once: what a factor is, how a sign-in
stops halfway, and what proves the second half.

Three shapes were available for the stop.

A session flagged `half_open`, upgraded by a later call. Every reader of a
session would then have to ask whether it is whole, and every one that forgets
is a hole. The flag would reach `Session`, which thirteen routes resolve.

A token the browser holds in script memory and sends back in a header. It
keeps the server stateless between the two steps, and puts a bearer secret
where any script on the page can read it, while the session cookie next to it
is `HttpOnly` precisely so nothing can.

A challenge row, named by a `HttpOnly` cookie, spent once. It is the shape the
password reset already uses, down to the sha256 digest and the window the
adapter owns.

## Decision
A sign-in that meets a confirmed factor opens a challenge instead of a
session, and `SignIn::begin` says which happened in its return type:

```rust
pub enum Opened {
    Session(SessionToken),
    SecondStep { challenge: ChallengeToken, offered: Vec<FactorKind> },
}
```

No caller can hold a session it has not earned, because there is no session
to hold until `SecondStep::verify` answers the challenge. The type carries the
invariant; no test stands in for it.

A factor is one row in `auth_factors`, keyed by `(account_id, kind)`:

```sql
CREATE TABLE auth_factors (
    account_id uuid NOT NULL REFERENCES accounts (id) ON DELETE CASCADE,
    kind text NOT NULL,
    secret bytea NOT NULL,
    created_at bigint NOT NULL,
    confirmed_at bigint,
    last_step bigint,
    PRIMARY KEY (account_id, kind)
);
```

One row per method, not one column per method: `kind` is text, so the email
factor is an insert rather than a migration, and an account keeps at most one
authenticator because the primary key says so. `confirmed_at` is null until
its owner reads a code off the thing and sends it back, and only confirmed
rows are offered at sign-in, so an abandoned enrolment locks nobody out.
`last_step` holds the 30-second step the last accepted answer fell in, and an
answer is admitted only past it, so a code read over a shoulder dies with its
step instead of lasting the drift window.

Recovery codes are their own table, one row per code, sha256 like a session
token and for the same reason ADR 0006 gives: they are minted with enough
entropy that a digest is enough, and argon2 over ten codes at every sign-in
would cost what argon2 costs for no gain. They are minted with the first
factor and dropped with the last, so an account never holds codes that open
nothing.

The challenge is `second_step_challenges`: a digest, the account, a five
minute window, and an attempt count. Five answers close it. The token behind
the digest travels in a `ft_second_step` cookie carrying the same flags as the
session cookie, `HttpOnly` included.

The domain reaches all of this through two ports rather than one. `SignIn`
takes `Gate`, which declares `offered` and `open` and nothing else;
`SecondFactors` and `SecondStep` take `FactorStore`, which declares the
eleven methods the settings and verify paths need. One adapter implements
both. A use case that needs two methods should not compile against nine it
never calls.

## Alternatives rejected
- The `half_open` session flag, above: it moves the rule into every reader of
  a session instead of keeping it in the one place that issues sessions.
- A challenge token in the response body: it hands a bearer secret to scripts
  on a page whose session cookie is deliberately out of their reach.
- One port with thirteen methods: `SignIn` would depend on recovery codes,
  enrolment and regeneration to ask one question.
- A `factors` column on `accounts`, a bitmask or a text array: the secret has
  to live somewhere per method anyway, and a second method would then be a
  migration rather than a row.
- Reusing `password_resets` for the challenge: the two have different windows,
  different answers and only the digest in common, and a shared table would
  let a reset link answer a sign-in challenge.

## Consequences
- Gains: nothing between a correct password and a session cookie except a row
  that has to be spent. The rule lives in one enum, and the thirteen routes
  that resolve a session are untouched.
- Costs: `SignIn` grows a fifth generic parameter, and `/auth/sign-in` now has
  two success shapes the front end must tell apart. A deployment with no
  enrolled factor sees neither.
- What becomes harder: a client that posts credentials and expects a cookie
  must read the status code. 204 is a session, 200 is a challenge. The
  self-hosted CLI does not sign in, so only the browser is affected.
- What becomes easier: the email factor is a `FactorKind` variant, a digest
  column beside the challenge, and a mailer call. No schema change, no change
  to the sign-in contract.
