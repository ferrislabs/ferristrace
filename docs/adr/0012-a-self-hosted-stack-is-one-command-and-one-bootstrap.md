# 0012 — A self-hosted stack is one command and one bootstrap

- Status: accepted
- Date: 2026-10-24

## Context

A self-hoster needs four processes and an account that can sign in. The
server now migrates both stores it owns, so the remaining work is bringing
the processes up and making the first account.

"On first start, create a default org" cannot be done by the server alone:
an organisation needs an owner, and only the operator knows which address
that is. Nor can it be a seed migration, because a migration runs in every
deployment including the hosted one.

## Decision

`ferristrace bootstrap --email <address>` creates the account if it is
absent, creates one organisation owned by it, and prints a single-use
reset token. **It does nothing when an organisation already exists**, so it
is safe to run again, safe to put in an entrypoint, and cannot quietly
give a deployment a second organisation.

It does not draw an ingest key. Each command prints at most one secret on
stdout, the way `issue-key` and `create-account` already do, so a pipeline
that captures stdout captures exactly one thing. `bootstrap` prints the
`issue-key` line to run next on stderr instead.

It is not run automatically by the compose file. An automatic bootstrap
would have to put the reset token in the service log, where it would sit
for the life of the deployment.

`GET /health` answers 200 with no credentials. It is a liveness answer:
the process is listening. Because migrations run before either listener
starts, a server that answers it has already applied both schemas — which
is what `depends_on: service_healthy` needs to mean.

The compose file carries `ferristrace` as the default password for
Postgres and ClickHouse, so `docker compose up` works on a clean machine
with no preparation, which is the acceptance criterion. **No database port
is published to the host**: only 4317 and 4318 are, so those defaults are
reachable only from inside the compose network. The README says to set
them for anything that is not a laptop.

`COOKIE_SECURE` defaults to false because the compose file serves plain
HTTP. A secure cookie over plain HTTP is never sent, so defaulting it to
true would make sign-in silently fail on the one setup this file
describes.

## Consequences

The CI job builds the image, brings the stack up on a clean runner, and
proves: both schemas were applied from inside the container, `bootstrap`
works and is idempotent, the token sets a password, the password opens a
session, `/auth/me` carries the owner role, a short password is refused
with 422, an issued key is accepted and an unissued one is refused with
401.

**It does not prove a span reaches ClickHouse.** It asserts the `spans`
table exists in the configured database, which proves the server reached
ClickHouse with the configured credentials, but the payload it posts is
empty so nothing is written. Proving a round-trip needs an encoded OTLP
fixture, and hand-rolling protobuf in a shell script is a worse thing to
own than this gap.

Images are not published anywhere. The compose file builds from the
checkout, so a self-hoster needs the source. Publishing on semver tags is
#67 and stays there.

## What the smoke test found

`/auth/reset/*` was mounted only when a mail relay was configured. The CLI
mints a reset token precisely so that a deployment with no relay can let
somebody in, and the endpoint that spends it was behind the very condition
that makes the CLI necessary. The token `bootstrap` printed could not be
spent at all, and the stack was unusable out of the box.

The two halves of a reset want different things. `request` needs a mailer
to send a link. `confirm` needs only the store and the hasher. So the
reset routes are now mounted whenever local passwords are on, with the
relay chosen the way the meter and the span store are: an SMTP relay when
one is configured, otherwise one that refuses to deliver and says why at
start.

`request` keeps answering 204 either way, so it still tells a caller
nothing about which addresses exist.

This is the whole argument for the job: the mistake was invisible to every
unit and integration test, because each half was correct on its own.

## And it found a second one

`tracing_subscriber::fmt()` writes to stdout. Every command that prints a
secret on stdout — `issue-key` since it landed, and now `bootstrap` —
could have a log line land in the middle of it. A sqlx migration notice
did exactly that, and the token read back was two lines rather than one.

The logger writes to stderr now. The convention that stdout carries at
most one secret and nothing else is only true if nothing else ever writes
there, and saying so was not enough to make it so.

## Amended: the control plane answers the loopback

This said no database port was published, and gave that as the reason the
default passwords were acceptable. That is no longer true of Postgres: a
database client on the host is worth more during development than the
absolute of publishing nothing, so it is bound to `127.0.0.1`.

The reason survives the change, narrowed. A port on the loopback answers
processes on this machine and nothing that can reach it over a network, so
`ferristrace/ferristrace` is still not offered to anybody who finds the
host. `POSTGRES_BIND` is what would widen that, and the README says to set
a password before touching it.

ClickHouse and Redis stay unpublished. Nothing has asked to reach them
from outside, and `docker compose exec` already answers the cases that
come up.
