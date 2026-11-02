# 0023. The workspace is one crate per layer and one per backend

Status: accepted
Date: 2026-11-02

## Context
`crates/core` held the domain in 31 flat files, each mixing value objects,
port traits, a service and its tests. `crates/storage` held every driven
adapter in one crate: Postgres, ClickHouse, Redis, S3, SMTP, OIDC, argon2, the
secret seal, and two pure policies (`StaticQuota`, the billing calendar). The
Redis counter took a `PgPool` to roll its hours into Postgres, so one adapter
wrote another backend outside any port. `crates/api` depended on nothing
concrete, but every handler repeated the generic bounds of the service it
served, and `apps/server` built seven copies of the session store.

A crate boundary is the one rule the compiler enforces: a module can be
imported by mistake, a crate that is not a dependency cannot.

## Decision
The workspace is split by layer and by backend, every package named
`ferristrace-*`:

```
domain        <- application <- api <- apps/server
domain        <- otlp        <- api
application   <- postgres, clickhouse, redis, infra <- apps/server
macros        <- application, postgres
```

- `ferristrace-domain` holds one directory per bounded context (`identity`,
  `organisation`, `catalogue`, `profile`, `telemetry`, `explore`, `issue`,
  `provisioning`), each with `value_objects.rs`, `entities.rs`, `ports.rs`,
  `errors.rs`, one file per service and a sibling `*_tests.rs` per service.
  `clock`, `secret` and `store` (`StoreError`) are the shared kernel at the
  root, and the root re-exports every public name.
- `ferristrace-application` holds the unit of work port, the repository
  registry (ADR 0024), the `Stack` trait naming one deployment's adapter
  types, `Ferristrace<S>` with one method per control plane use case, and
  `Ingestion<S>` for the hot path.
- One adapter crate per backend: `postgres`, `clickhouse`, `redis`, and
  `infra` for S3, SMTP, OIDC, argon2, the seal, the clocks and the in-memory
  adapters. Every port stays in the domain or the application.
- `ferristrace-api` serves every control plane router from one
  `AppState<S>`; a handler's only type parameter is `S: Stack`, bounded by
  its context's repository bundle.
- `apps/server` defines the one `Stack` impl, the adapter enums chosen at
  start, and the wiring. The package is `ferristrace-server`; its binary is
  still `comettrace`, so the image, the compose file and every documented
  command are unchanged.

The `layers` CI job fails if `domain` or `application` gains sqlx, axum,
tonic, tokio, clickhouse, redis, reqwest or lettre, or if `api` gains an
adapter crate.

## Alternatives rejected
- Copy autharie exactly: its application crate depends on its Postgres crate
  to reach the registry, and its domain derives `serde` and `utoipa`. Here the
  Postgres crate implements the application's registry, and DTOs stay in
  `api`, so the dependency arrows keep pointing inward.
- A separate persistence crate for the shared transaction handle: it would
  hold one type used by one crate. `SharedTx` lives in `ferristrace-postgres`.
- Modules inside the existing crates instead of new crates: the layering would
  rest on review instead of the compiler.

## Consequences
- Gains: a wrong import is a compile error; a backend can be replaced by
  writing one crate; a reader finds a context in one directory.
- Costs: eleven crates instead of six, and a root re-export list to keep in
  step with the contexts.
- What becomes harder: a port change touches the domain, the adapter and the
  test fakes in `application` and `api`. `ferristrace-postgres` still depends
  on `ferristrace-infra` for the argon2 hasher, the seal and the test clock;
  that edge is accepted until those are behind ports the repositories take.
