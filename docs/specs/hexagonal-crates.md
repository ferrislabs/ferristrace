# Hexagonal crates

Goal
  Split the workspace into domain, application and one crate per driven
  backend, run every multi-step control plane use case in one Postgres
  transaction, and give the HTTP layer one state type, without changing
  anything a client, an operator or a stored row can observe.

Context
  The workspace already keeps infrastructure out of `crates/core`, but
  `crates/core` is 31 flat files that each mix value objects, ports, a
  service and tests, and `crates/storage` holds Postgres, ClickHouse, Redis,
  S3, SMTP, OIDC, argon2 and two pure policies in one crate. Every Postgres
  adapter opens its own transaction, so `Provisioning::bootstrap` runs four
  of them and a failure between two leaves an organisation without its
  project. Every handler repeats the generic bounds of the service it
  serves, and `apps/server/src/main.rs` builds seven `PostgresSessions`.
  `apps/server/src/cli.rs` names `PostgresControlPlane`, which ADR 0017 says
  it does not. The target is the layering of `/opt/ferrislabs/autharie`
  (`autharie-domain`, `autharie-core`, `autharie-postgres`, `autharie-macros`)
  with two of its shortcuts left out: its application crate depends on its
  Postgres crate, and its domain derives `serde` and `utoipa`.

Glossary
  Stack       the set of concrete adapter types one deployment runs on, named
              once in `apps/server`.
  Unit of work a port that runs a closure inside one transaction and commits
              on `Ok`.
  Registry    the marker types and the `RepoFor` trait through which a use
              case asks a unit of work for the repository of one domain.

Invariants
  - `ferristrace-domain` depends on no crate that does I/O.  [test: cargo tree in CI]
  - `ferristrace-application` depends on no sqlx, axum, tonic, clickhouse,
    redis, reqwest, lettre or tokio.  [test: cargo tree in CI]
  - `ferristrace-api` depends on no adapter crate.  [test: cargo tree in CI]
  - Only `apps/server` names a concrete adapter.  [test: the three above]
  - A control plane use case that writes to Postgres writes in one
    transaction or not at all.  [type: repositories are built from the
    transaction handle only]
  - Every registry marker has a Postgres repository.  [type: `Repositories`
    bound on the stack named in `apps/server`]
  - No port is defined in an adapter crate.  [review]
  - HTTP routes, status codes, response bodies, cookies, gRPC codes, CLI
    commands and output, the `comettrace` binary name, `COMETTRACE_*`
    settings, token prefixes, and both migration sets are unchanged.
    [test: existing suites and the self-hosting CI job]

Out of scope
  - OpenAPI.
  - Cucumber scenarios or an e2e crate.
  - Any new feature, route, migration or setting.
  - Removing the `&T` and `Arc<T>` blanket impls on ports.
  - Renaming the binary, the image, the `COMETTRACE_` prefix or the
    `comettrace` database.

Design
  Crates, all under `crates/`, all named `ferristrace-*`:

    domain        contexts, each a directory:
                    <context>/mod.rs          re-exports
                    <context>/value_objects.rs
                    <context>/entities.rs     when the context has any
                    <context>/ports.rs
                    <context>/service.rs      one file per service if several
                    <context>/errors.rs
                    <context>/tests.rs        #[cfg(test)]
                  contexts: identity, organisation, catalogue, profile,
                  telemetry, explore, issue, provisioning; `clock.rs` and
                  `secret.rs` stay at the root as the shared kernel.
                  The crate root re-exports every public name it exports
                  today, so downstream imports keep working.
    application   registry, unit of work port, `Stack`, `Ferristrace<S>`
                  (control plane use cases) and `Ingestion<S>` (hot path).
                  One module per context implementing its use cases.
    macros        `#[repository]`, `#[transactional]`.
    postgres      `SharedTx`, the `Postgres` unit of work, every Postgres
                  adapter, `migrations/postgres`, `load_allowances`,
                  `in_scratch_tx` for tests.
    clickhouse    sink, reader, issues, migrator, `migrations/clickhouse`.
    redis         `RedisMeter`.
    infra         S3 photos, SMTP mailer, OIDC provider, argon2 hasher,
                  secret seal, system and fixed clocks, the in-memory sink,
                  meter and directory.
    otlp          unchanged apart from its name.
    api           `AppState<S>`, a `Session` extractor, DTOs, routers.
    ee            unchanged apart from its name.
  apps/server     package `ferristrace-server`, binary `comettrace`:
                  configuration, the `Stack` impl, the adapter enums,
                  wiring, the loops that call application jobs, serving.

  Dependency graph (arrows point at what is depended on):

    domain   <- application <- api <- server
    domain   <- otlp        <- api
    application <- postgres, clickhouse, redis, infra <- server
    macros   <- application, postgres

  Ports:
    Domain ports stay in the domain context that owns them, with their
    current signatures (`&self`, `-> impl Future<Output = _> + Send`).
    The unit of work and the registry are application ports.
    Dispatch: static generics through `S: Stack`; enum dispatch in
    `apps/server` where a deployment chooses between adapters at start.
    No `dyn`, no `async-trait` (tonic's `#[tonic::async_trait]` stays where
    tonic requires it).

  Adapters:
    A Postgres repository holds a `SharedTx` and nothing else, and is
    registered with `#[repository(domain = X, backend = Postgres)]`.
    Adapters that are not transactional (ingest key directory, ClickHouse,
    Redis, S3, SMTP, OIDC, argon2, clocks) are fields of the stack, built
    once at start.

  Errors:
    One `thiserror` enum per context in `<context>/errors.rs`. A Postgres
    write port returns its own context's error, never `ReadError`.
    Application methods return the error of the context they serve; a
    begin or commit failure becomes that context's unavailable variant,
    named in `#[transactional(.., unavailable = X::Variant)]`.
    `ferristrace-api` keeps one `*Refused` enum per router.

Acceptance
  @spec-hex-1  `cargo tree -p ferristrace-domain -e normal` lists none of
               sqlx, axum, tonic, tokio, clickhouse, redis, reqwest, lettre.
  @spec-hex-2  `cargo tree -p ferristrace-application -e normal` lists none
               of sqlx, axum, tonic, tokio, clickhouse, redis, reqwest,
               lettre.
  @spec-hex-3  `cargo tree -p ferristrace-api -e normal` lists none of
               ferristrace-postgres, -clickhouse, -redis, -infra.
  @spec-hex-4  `bootstrap` interrupted after the organisation is founded and
               before its project is claimed leaves no organisation, against
               a real Postgres.
  @spec-hex-5  Inviting an address that already carries an account adds a
               membership and creates no account, decided by the domain and
               tested without a database.
  @spec-hex-6  Every test that passes on `main` passes, with its assertions
               unchanged; only its setup may change.
  @spec-hex-7  The `self-hosting` CI job passes without edits to its steps.
  @spec-hex-8  No handler in `ferristrace-api` takes a type parameter other
               than `S: Stack`.
  @spec-hex-9  `apps/server/src/cli.rs` names no type from an adapter crate.

Simulation
  No. The change moves code and widens transactions; it adds no
  concurrency, no timing and no distribution.

Workstreams
  Run one after another on one branch, one commit each, each leaving the
  workspace compiling (`cargo check --workspace --all-targets` in its own
  `CARGO_TARGET_DIR`).

  W0  orchestrator   rename the crates, create the empty ones, write
                     `macros`, the registry, the unit of work and `Stack`,
                     `SharedTx` and the `Postgres` unit of work.
  W1  domain         move `crates/core` into `crates/domain` by context,
                     re-exporting every current name. Depends on W0.
  W2  adapters       split `crates/storage` into postgres, clickhouse,
                     redis, infra; move `StaticQuota` to the domain.
                     Depends on W0. Independent of W1 if W1 keeps names.
  W3  sessions       convert `PostgresSessions`, `PostgresMemberships` and
                     `PostgresAccounts` to `SharedTx`, register them, and
                     build `Whoami` inside a transaction. Depends on W1, W2.
  W4  contexts       per context group, in parallel once W3 lands, each
                     owning its domain context directory, its Postgres
                     adapters, its application module, its router and its
                     tests:
                       a  organisation, catalogue, teams, roster, profile
                       b  identity: sign-in, sessions, reset, password,
                          second factor, oidc
                       c  telemetry, explore, issue, usage
                       d  provisioning and the command line
  W5  server         `AppState<S>`, the `Stack` impl, router assembly,
                     background jobs, ADRs, CLAUDE.md, CI cargo-tree checks.

  Orchestrator-owned convergence points: every `Cargo.toml`,
  `crates/application/src/registry.rs`, `crates/application/src/stack.rs`,
  `crates/application/src/lib.rs`, `crates/api/src/lib.rs`,
  `crates/api/src/state.rs`, `apps/server/src/main.rs`,
  `crates/postgres/src/lib.rs`.
  Regenerated at integration: `Cargo.lock` (`cargo check --workspace`).

Frozen contracts
  ```rust
  // ferristrace-application::registry
  pub trait UnitOfWork: Send + Sync + 'static {
      type Tx: Clone + Send + Sync + 'static;

      fn transaction<T, E, F, Fut>(
          &self,
          unavailable: impl Fn() -> E + Send,
          work: F,
      ) -> impl Future<Output = Result<T, E>> + Send
      where
          T: Send,
          E: Send,
          F: FnOnce(Self::Tx) -> Fut + Send,
          Fut: Future<Output = Result<T, E>> + Send;
  }

  pub trait RepoFor<D>: UnitOfWork {
      type Repo: Send + Sync;

      fn build(&self, tx: &Self::Tx) -> Self::Repo;
  }

  pub mod domain {
      // one zero-sized marker per repository domain, e.g.
      #[derive(Debug)]
      pub struct Sessions;
  }

  pub trait Repositories:
      UnitOfWork
      + RepoFor<domain::Sessions, Repo: ferristrace_domain::SessionStore>
      // + one line per marker
  {
  }

  // ferristrace-application::stack
  pub trait Stack: Send + Sync + 'static {
      type Repos: Repositories;
      // + one associated type per non-transactional adapter, each bounded
      //   by its domain port and Send + Sync + 'static
  }

  #[derive(Debug)]
  pub struct Ferristrace<S: Stack> { /* repos + adapters + settings */ }
  ```

  ```rust
  // ferristrace-macros, usage
  #[repository(domain = Sessions, backend = Postgres)]
  pub struct PostgresSessions { tx: SharedTx, /* clock, lifetime */ }

  impl<S: Stack> Ferristrace<S> {
      #[transactional(sessions, memberships, orgs, unavailable = SettingError::StoreUnreachable)]
      pub async fn rename_org(
          &self,
          token: &SessionToken,
          org: TenantId,
          name: &OrgName,
      ) -> Result<Organisation, SettingError> {
          OrgSettings::new(Whoami::new(sessions, memberships), orgs, &self.photos)
              .rename(token, org, name)
              .await
      }
  }
  ```

  `#[transactional(a, b, unavailable = E)]` expands to
  `self.repos.transaction(|| E, move |tx| async move { let a = <S::Repos as
  RepoFor<registry::domain::A>>::build(&self.repos, &tx); ... body })`.
  The method's impl block names its stack parameter `S`.

  `SharedTx` is `Arc<tokio::sync::Mutex<Option<sqlx::Transaction<'static,
  sqlx::Postgres>>>>`; the `Postgres` unit of work takes the transaction
  back with `Arc::try_unwrap` after the work and rolls back if a clone
  outlived it.

Risks / open questions
  - Postgres adapters that a request path reads without writing now open a
    transaction per request. Accepted: one BEGIN/COMMIT pair on a pooled
    connection. The ingest key directory stays outside, on the pool, because
    it is on the hot path (ADR 0004).   owner: W3
  - Pull request #159 changes `Dockerfile` and `ci.yaml` on another branch;
    whichever merges second rebases the package name.   owner: maintainer

Verify (exit)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --locked -- -D warnings
  cargo test --workspace --locked
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  REDIS_URL=... REQUIRE_REDIS_URL=1 cargo test -p ferristrace-redis
  cd web && pnpm typecheck

Verify (loop)
  CARGO_TARGET_DIR=/tmp/ferristrace-check cargo check --workspace --all-targets

Amended: 2026-11-02 `Stack::Repos` is bounded by `UnitOfWork` only; each use-case impl block and router bounds `S::Repos` by its context's bundle trait in `registry.rs`, and the server asserts `Repositories` on its backend. This lets a test stack implement one context.
Amended: 2026-11-02 no `persistence` crate: `SharedTx` and the `Postgres<C>` unit of work live in `ferristrace-postgres` (ADR 0023).
Amended: 2026-11-02 `UnitOfWork::transaction` takes a `Settle` policy; `#[transactional(.., settle = always)]` commits on refusal, used by the second-step answer and the OIDC callback (ADR 0024).
Amended: 2026-11-02 the Postgres write ports return the shared-kernel `StoreError::Unreachable` (renamed from `ReadError`) rather than one error per context: the only variant was "unreachable", and one type in the kernel says it once.
Amended: 2026-11-02 the domain depends on `time` (no I/O) for the billing calendar, moved from infra with `StaticQuota`.
Amended: 2026-11-02 the ingest key directory has its own two-connection pool; a refused request no longer extends its session (ADR 0024, Costs).
Amended: 2026-11-02 known debt: `ferristrace-postgres` depends on `ferristrace-infra` for the argon2 hasher, the seal and the test clock.
