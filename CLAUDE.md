# Comettrace

Self-hosted distributed tracing. OTLP in over gRPC (4317) and HTTP (4318),
spans in ClickHouse, control plane in Postgres, usage counter in Redis, a
React front end served by the same binary.

Start with [README.md](README.md) for the layout and
[deploy/README.md](deploy/README.md) for running the stack.

## Layers

```
domain       <- application <- api <- apps/server
domain       <- otlp        <- api
application  <- postgres, clickhouse, redis, infra <- apps/server
```

ADR 0023 is the layout, ADR 0024 the transactions.

- `crates/domain` holds one directory per bounded context: `value_objects`,
  `entities`, `ports`, `errors`, one file per service, `*_tests.rs` beside
  each. It depends on nothing that does I/O. New public names are re-exported
  from `src/lib.rs`.
- `crates/application` owns the unit of work, the repository registry
  (`src/registry.rs`: one marker per Postgres repository, one bundle trait per
  context), `Stack`, and `Ferristrace<S>`. A control plane use case is one
  `#[transactional(..)]` method in the module of its context.
- `crates/postgres`, `crates/clickhouse`, `crates/redis`, `crates/infra`
  implement the driven ports, one backend each. A Postgres repository holds a
  `SharedTx` and is registered with `#[repository(..)]`.
- `crates/api` is the driving side: axum routers over `AppState<S>`, the
  tonic service over `Ingestion<S>`, DTOs. A DTO never leaves this crate; it
  becomes a domain type through `TryFrom` at the edge. A handler's only type
  parameter is `S: Stack`.
- `apps/server` is the only place that names a concrete adapter: the
  `Production` stack and the adapter enums in `adapters.rs`, routers in
  `routes.rs`. `main.rs` is configuration, wiring and the server; `cli.rs` is
  the clap definitions and the printing, over the application. Neither holds
  a rule.
- `crates/ee` is the cloud build behind the `ee` feature, and the `ee-gate`
  CI job proves the default build excludes it. The `layers` job proves the
  dependency arrows above.

## Conventions

- No comments in code. The reason for a change goes in the commit message,
  or in an ADR when it is a decision.
- Every architecture decision is an ADR in `docs/adr`, numbered, in the shape
  the existing ones use. The code and its ADR must not disagree.
- Commits are conventional and lowercase: `feat(orgs): ...`, `fix(photos):
  ...`, `chore: ...`, with `!` for a breaking change.
- Everything written into the repository is in English.
- Static dispatch first, then enum dispatch, then `dyn` with a reason. No
  `async-trait`.
- Errors are `thiserror` enums per boundary. `anyhow` only in `apps/server`.
- Secrets are newtypes that zeroize and refuse to print themselves.

## Commands

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && pnpm install && pnpm typecheck
```

Integration tests skip silently without a backend unless the matching
`REQUIRE_*` variable is set, which is what CI sets:

```sh
DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
DATABASE_URL=... REDIS_URL=... REQUIRE_REDIS_URL=1 cargo test -p ferristrace-redis
```

Each adapter crate keeps its guards in `tests/backend/mod.rs`.

## Working agreement

The maintainer runs the compile loop. Write the code, say what needs
checking, and leave `cargo build`, `cargo clippy`, `cargo test`,
`docker compose build` and `docker compose up` to him: cargo takes an
exclusive lock on `target/`, and a compose rebuild restarts a running dev
stack.

`tools/` is local scaffolding and is deliberately not committed.
