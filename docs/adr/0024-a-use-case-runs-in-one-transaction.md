# 0024. A use case runs in one transaction

Status: accepted
Date: 2026-11-02

## Context
Every Postgres adapter held the pool and opened its own transaction per
method. A use case that called three adapters ran three transactions:
`Provisioning::bootstrap` founded the organisation, claimed its project and
invited its owner separately, and a failure between two left an organisation
with no project. Deciding and assigning an issue in one request committed the
status even when the assignment was refused.

## Decision
A control plane use case is a method on `Ferristrace<S>` marked
`#[transactional(repo_a, repo_b, unavailable = Error::Variant)]`. The macro
opens one transaction through the `UnitOfWork` port, builds each listed
repository from it through `RepoFor<marker>`, runs the body, commits on `Ok`
and rolls back on `Err`; a failure to open or commit returns the named
variant. Postgres repositories hold a `SharedTx` and nothing else, and are
registered with `#[repository(domain = X, backend = Postgres<C>)]`; the
server asserts at compile time that its backend registers every marker.

Two writes exist to outlive a refusal: the attempt counter of a second-step
challenge, which caps guessing, and the consumed OIDC state, which must not
be replayed. Their use cases are marked `settle = always` and commit whatever
the outcome.

Postgres aborts a transaction on its first failed statement, and a COMMIT
after that rolls back without an error. A repository method whose failure the
use case tolerates, or reports while another write must survive, runs inside
a savepoint: the OIDC account provisioning, so an address that is already
taken still leaves its flow consumed, and the password hash upgrade, so a
failed upgrade does not fail the sign-in.

The ingest key directory stays on a pool of its own (ADR 0004): the hot path
opens no control plane transaction and never waits for a connection a use
case is holding. ClickHouse, Redis, S3, SMTP and OIDC calls are not
transactional; a use case that writes to one of them and to Postgres commits
Postgres last.

## Alternatives rejected
- Keep one transaction per adapter and order the writes carefully: it leaves
  the half-written states in place and puts the rule in every caller.
- Pass `&mut Transaction` to each repository method: every port would name
  sqlx and the domain would learn that a transaction exists.
- Roll back every refusal: it silently removes the second-step attempt limit
  and lets a consumed OIDC state be replayed.

## Consequences
- Gains: a refused use case leaves nothing behind; a repository cannot reach
  the pool; a test proves atomicity with a recording unit of work and no
  database.
- Costs: a read-only request now opens a BEGIN/COMMIT pair on a pooled
  connection. A use case that calls ClickHouse, SMTP, S3 or the OIDC token
  endpoint holds its connection for the duration of that call. A session's
  sliding renewal is written by the use case that resolved it, so a request
  that is refused after the session check does not extend the session.
- What becomes harder: a use case that wants to catch a unique violation and
  keep writing needs a savepoint, because Postgres aborts the transaction on
  the first failed statement.
