# FerrisTrace

Distributed tracing you can host yourself. An OTLP endpoint takes spans over
gRPC or HTTP, ClickHouse keeps them, Postgres keeps the accounts, the
organisations, the projects and the keys, and a web application reads the
traces back.

One command brings the whole thing up, and one more gives it a way in:
[deploy/README.md](deploy/README.md).

```sh
cd deploy
docker compose up --build --detach
docker compose exec server ferristrace bootstrap --email you@example.com
```

The server then answers OTLP over gRPC on 4317, and OTLP over HTTP, the API
and the web application on <http://localhost:4318>. Keys that send or read
telemetry are made under Settings > Project > Api keys, and every change to an
organisation is listed for its owners and administrators under Settings >
Organisation > Audit log.

## What is in here

| Path | What it holds |
| --- | --- |
| `crates/domain` | The domain, one directory per bounded context: value objects, entities, port traits, domain services. Depends on nothing that does I/O. |
| `crates/application` | The use cases: each control plane use case runs in one transaction through the unit of work port. |
| `crates/macros` | `#[repository]` and `#[transactional]`. |
| `crates/otlp` | OTLP protobuf in, domain spans out. |
| `crates/postgres` | The control plane adapters and their migrations. |
| `crates/clickhouse` | The span store adapters and their migrations. |
| `crates/redis` | The usage counter. |
| `crates/infra` | S3 photos, SMTP, OIDC, the secret seal, clocks, in-memory adapters. |
| `crates/api` | The driving adapters: the HTTP routers, the OTLP gRPC service, the DTOs. |
| `crates/ee` | The cloud build, behind the `ee` feature. Empty in this repository. |
| `apps/server` | The binary: configuration, wiring, the CLI, the server. |
| `web` | The front end: React, Vite, TypeScript. |
| `deploy` | The self-hosted stack and its guide. |
| `docs/adr` | Every architecture decision, in the order it was taken. |

Dependencies point inward: the adapter crates know `crates/application`
and `crates/domain`, `crates/api` knows `crates/application`, and neither
the application nor the domain knows any of them. `apps/server` is the only
place that names a concrete adapter. See ADR 0023 and ADR 0024.

## Working on it

Rust 1.99.1, edition 2024, pinned in `rust-toolchain.toml`. Node 26 and
pnpm 10 for the front end.

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && pnpm install && pnpm typecheck
```

The integration tests want a real backend and are skipped without one. Set
`REQUIRE_DATABASE_URL`, `REQUIRE_CLICKHOUSE_URL` or `REQUIRE_REDIS_URL` to
turn a missing backend into a failure instead of a skip:

```sh
DATABASE_URL=postgres://ferristrace:ferristrace@localhost:5432/ferristrace \
REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
```

Running the front end against the stack, without rebuilding the image, is in
[deploy/README.md](deploy/README.md#changing-the-front-end-without-rebuilding-anything).

## Releases

A tag `vX.Y.Z` publishes two images for linux/amd64 and linux/arm64:
`ghcr.io/ferrislabs/ferristrace:X.Y.Z` is the community build and
`:X.Y.Z-ee` the cloud one. Both carry provenance and an SBOM.

The number follows SemVer read for a pre-1.0 product: a breaking change bumps
the minor, everything else the patch. What counts as breaking, and why the
changelog is generated from the commits rather than kept in a file, is
[ADR 0018](docs/adr/0018-a-version-is-cut-from-a-tag-that-matches-the-manifest.md).
The runbook is [docs/releasing.md](docs/releasing.md).

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) has the development setup, how to run the
tests that need a real backend, what the code is held to, and what a pull
request needs. Contributions come in under a sign-off, the
[DCO](DCO), with no CLA.

Everybody taking part is held to the
[code of conduct](CODE_OF_CONDUCT.md). A vulnerability goes to
[SECURITY.md](SECURITY.md), not to an issue.

## License

Apache-2.0. See [LICENSE](LICENSE).
