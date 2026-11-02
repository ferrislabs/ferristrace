# Contributing to Comettrace

Thanks for considering it. This page says how to get the stack running, how
to run the tests, what the code is held to, and what a pull request needs.

Everything written into this repository is in English.

Two pages sit beside this one: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md), which
everybody taking part is held to, and [SECURITY.md](SECURITY.md), which is
where a vulnerability goes instead of an issue.

## Sign off your commits

Comettrace takes contributions under the [Developer Certificate of
Origin](DCO). It is a one-line statement that you wrote the patch or
otherwise have the right to submit it under the project's licence. There is
no CLA and no copyright assignment.

Add the line to every commit:

```sh
git commit -s -m "fix(spans): stop losing the last batch on shutdown"
```

It appends `Signed-off-by: Your Name <your@email>`, which must match the
commit author. Forgot it?

```sh
git commit -s --amend --no-edit      # the last commit
git rebase --signoff origin/develop  # every commit on your branch
```

CI refuses a pull request whose commits are not signed off, one by one, so
you will know which.

## Getting the stack up

You need Docker, Rust 1.97.0 (pinned in `rust-toolchain.toml`, so `rustup`
picks it on its own) and, for the front end, Node 26 with pnpm 10.

```sh
git clone git@github.com:ferrislabs/ferristrace.git
cd comettrace/deploy
docker compose up --build --detach
docker compose exec server comettrace bootstrap --email you@example.com
```

That brings up Postgres, ClickHouse, Redis and the server, applies both sets
of migrations, and prints a single-use token that sets your password. The
application is on <http://localhost:4318>, OTLP over gRPC on 4317.
[deploy/README.md](deploy/README.md) covers the rest: settings, an identity
provider, sending a trace, and running the front end against the stack
without rebuilding the image.

## Running the tests

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && pnpm install && pnpm typecheck
```

Unit tests need nothing. Integration tests need the real backend they test,
and **skip silently without one**, so set the matching `REQUIRE_*` variable
to turn a missing backend into a failure instead of a pass:

```sh
DATABASE_URL=postgres://comettrace:comettrace@localhost:5432/comettrace \
  REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres

CLICKHOUSE_URL=http://localhost:8123 CLICKHOUSE_USER=comettrace \
  CLICKHOUSE_PASSWORD=comettrace REQUIRE_CLICKHOUSE_URL=1 \
  cargo test -p ferristrace-clickhouse

DATABASE_URL=postgres://comettrace:comettrace@localhost:5432/comettrace \
  REDIS_URL=redis://localhost:6379 REQUIRE_REDIS_URL=1 \
  cargo test -p ferristrace-redis
```

Each adapter crate keeps its guards in `tests/backend/mod.rs`. CI sets all
three.

## What the code is held to

**The layers.** The adapter crates (`postgres`, `clickhouse`, `redis`,
`infra`) and `crates/api` know `crates/application` and `crates/domain`;
neither of those knows them, and the `layers` CI job checks it. The domain
declares no infrastructure dependency and must keep declaring none. Port
traits are defined by the domain or the application and implemented by
adapters. A control plane use case runs in one transaction (ADR 0024). A DTO never leaves
`crates/api`: it becomes a domain type through `TryFrom` at the edge.
`apps/server` is the only place that names a concrete adapter.

**No comments.** The default state of a file is no comments. Name things so
the code says what it does, and put the reason for a change in the commit
message, or in an ADR when it is a decision. The exceptions are a one-line
`// SAFETY:` where a lint demands it and `// TODO(#123):` with an issue
number. No commented-out code; git remembers.

**Rust.** Static dispatch first, then enum dispatch, then `dyn` with a
reason given in the pull request. No `async-trait`: traits use
`fn f(&self) -> impl Future<Output = T> + Send`. Errors are `thiserror`
enums, one per boundary; `anyhow` only in `apps/server`. Newtypes for
identifiers and secrets, and a secret neither prints nor outlives its
scope. `unwrap` and `expect` belong in tests.

**Tests.** Many unit tests on the domain with no I/O, some integration tests
on one adapter against its real backend, property tests for parsers and
conversions. A port with several implementations gets one contract suite run
against all of them. No `sleep`; inject a clock.

**Architecture decisions** go in `docs/adr`, numbered, in the shape the
existing ones use. If a change makes an ADR wrong, the change fixes the ADR.

## Pull requests

1. Branch off `develop`, and open the pull request against `develop`.
   `main` is the released branch.
2. One theme per pull request. If it has two, it is two pull requests.
3. Commits are conventional and lowercase: `feat(orgs): ...`,
   `fix(photos): ...`, `test(storage): ...`, `docs: ...`, with `!` for a
   breaking change. Write what the commit does, and why if that is not
   obvious. CI checks the pull request title and every commit subject: a
   rebase merge keeps the subjects, and the changelog of the next release is
   generated from them.
4. Every commit signed off, see above.
5. The description says what breaks if the change is wrong, and how you
   checked it. Link the issue it closes.
6. CI must be green: `fmt`, `clippy`, `test`, the Postgres, ClickHouse and
   Redis suites, the self-hosted stack coming up on a clean machine, the
   image building for both architectures, and the dependency audit.
7. A change that touches a layer boundary, a schema or a published
   behaviour wants an ADR in the same pull request.

Nobody merges their own pull request without a review, and the maintainer
does the merge.

## Picking something to work on

The [open issues](https://github.com/ferrislabs/ferristrace/issues) carry
labels that say where and how big:

- **`good first issue`** — small, self-contained, and the surrounding code is
  already tested. Start here.
- **`help wanted`** — worth doing, nobody is on it.
- **`area/ingest`, `area/storage`, `area/web`, `area/auth`, `area/docs`** —
  which part of the system it touches.
- **`ee`** — the commercial edition. Not open to outside contributions.
- **`bug`**, **`enhancement`** — what kind of change it is.

Say on the issue that you are taking it before you start, so two people do
not write the same patch. If an issue is unclear, ask on it rather than
guessing: a question costs a day, a wrong guess costs a week.

For anything larger than a `good first issue`, open an issue first and agree
the shape before writing the code. A pull request nobody expected is the
hardest kind to accept.
