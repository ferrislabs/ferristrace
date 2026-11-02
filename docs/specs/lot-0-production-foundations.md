# Lot 0: production foundations

Goal
  Make FerrisTrace runnable and governable in production: keys a team
  manages itself, a record of who changed what, retention per organisation,
  an export, and a Helm chart.

Context
  Milestone "0 · Production foundations": #118, #173, #128 (retention part),
  #174, #84. Ingest keys (`ft_live_`) exist but are issued from the command
  line only and authenticate ingest only (ADR 0003, 0007). Retention is one
  value per instance, `spans.retention_days`, applied by a ClickHouse table
  TTL. Nothing records control plane writes. Nothing exports. There is no
  chart; FerrisKey's `charts/ferriskey` is the house model.

  One pull request per part, in this order, each leaving main releasable:
    1  API keys with scopes          #118
    2  Audit log                     #173
    3  Retention per organisation    #128
    4  Export                        #174  (import is a separate issue)
    5  Helm chart                    #84

Glossary
  API key      a secret bound to one organisation and optionally one project,
               carrying one or more scopes. Replaces "ingest key" in the UI.
  Scope        what a key may do: `ingest` (send OTLP) or `read` (read
               telemetry through /api).
  Read grant   what a valid read key resolves to: its organisation and its
               project, if any.
  Audit entry  one control plane write: actor, action, target, time.

Invariants
  - A key without `ingest` never authenticates an OTLP payload.  [test]
  - A key without `read` never authenticates an /api request.  [test]
  - A read key never writes: no route that changes state accepts it.  [test]
  - A read key bound to a project never returns another project's data.  [test]
  - A key's secret is shown once, at creation, and stored only as a digest
    (ADR 0007).  [type: the secret is a value of the issue result only]
  - Every control plane write records one audit entry in the same
    transaction as the write (ADR 0024).  [test: a refused write leaves none]
  - Audit entries are never updated or deleted by the application.  [test]
  - A span's retention never exceeds its organisation's plan allowance.  [type]
  - The export of an organisation contains nothing from another one.  [test]
  - The default build stays one binary; the chart deploys only that binary.

Out of scope
  - Import of an export (new issue).
  - Viewer role, SSO/SAML, scopes beyond `ingest` and `read` (agents, lot 5).
  - Read keys on routes other than services, traces, trace, issues, issue,
    occurrences.
  - Bundled Postgres, ClickHouse or Redis in the chart.

Design
  1 API keys
    Postgres: `api_keys` gains `name text NOT NULL DEFAULT ''`,
    `scopes text[] NOT NULL DEFAULT '{ingest}'`, `created_by uuid` (accounts,
    ON DELETE SET NULL). Existing keys keep `{ingest}`.
    Domain, new context `access`: `KeyScope { Ingest, Read }`, `KeyScopes`
    (non-empty set), `KeyName` (trimmed, 1..=64 bytes), entity `ApiKey`
    (id, project, name, prefix, scopes, created_at, created_by, last_used_at;
    revoked keys are not listed), `ApiSecret` (the full secret, same
    `ft_live_` format, redacted in Debug), port `ApiKeys` (list, issue,
    revoke, grant_for), service `KeyRing` (members list, admins issue and
    revoke), `ReadGrant`. A project-bound read key asking for another project,
    or for another organisation, gets the same refusal a non-member gets on
    that route today (404 on explore, 403 on issues), never an empty result.
    Read endpoints take a `Reader` (`Session(SessionToken)` or
    `Key(ApiSecret)`): a session reads through its membership as today, a
    key through its grant. The ingest directory only resolves keys whose
    scopes contain `ingest`.
    HTTP: `GET/POST /api/orgs/{org}/keys`, `DELETE /api/orgs/{org}/keys/{id}`;
    `Authorization: Bearer <key>` accepted on the six read routes above.
    Web: Settings > Project > API keys wired to those routes.
    CLI: `issue-key` keeps its output and issues `{ingest}`.
  2 Audit log
    Postgres `audit_entries` (id, org_id, actor_kind, actor_id, action,
    target_kind, target_id, detail jsonb, at). Application writes one entry
    per control plane use case that changes state, inside its transaction,
    through an `AuditTrail` repository. `GET /api/orgs/{org}/audit` for
    owners and admins, filtered by actor and action, newest first, paged.
    Web: Settings > Organisation > Audit log.
  3 Retention
    ClickHouse: `spans` and `span_events` gain `retention_days UInt16`, set
    at ingest from the organisation; TTL becomes
    `start + toIntervalDay(retention_days)`. Postgres `orgs.retention_days`
    (NULL = deployment default), capped by the plan's `retention_days`.
    Owners set it in Settings > Organisation; the deployment default stays
    `spans.retention_days`.
  4 Export
    `GET /api/orgs/{org}/export` for owners: a streamed `tar.gz` holding
    `control-plane.json` (org, projects, teams, members, keys without
    secrets, issue decisions, audit entries) and `spans.ndjson` (one OTLP
    JSON span per line), with no temporary storage.
  5 Helm chart
    `charts/ferristrace` modelled on `/opt/ferrislabs/ferriskey/ferriskey/
    charts/ferriskey`: Deployment, Service, optional HTTPRoute, probes,
    securityContext, PodDisruptionBudget; database, ClickHouse and Redis
    URLs from Secrets; an ArgoCD Application example under
    `deploy/argocd/`; `helm lint` and `kubeconform` in CI; OCI publish on a
    release tag.

Acceptance
  @spec-l0-1  A key created with scopes {read} is refused by POST /v1/traces
              with 401, and a key with {ingest} is refused by
              GET /api/orgs/{org}/traces with 401.
  @spec-l0-2  A read key bound to project A gets only project A's traces and
              issues, even when the query names project B.
  @spec-l0-3  Creating a key returns its secret once; listing keys never does.
  @spec-l0-4  A member lists keys; only an admin or owner creates or revokes
              one; a revoked key is refused at once.
  @spec-l0-5  Renaming an organisation writes one audit entry naming the
              account; a refused rename writes none.
  @spec-l0-6  An organisation with retention 7 days gets spans whose TTL is
              7 days, while another keeps the deployment default.
  @spec-l0-7  The export of org A contains A's spans and none of org B's.
  @spec-l0-8  `helm template` with the example values renders and passes
              kubeconform; the Deployment runs as non-root with a read-only
              root filesystem.

Simulation
  No: no new concurrency or timing beyond what the existing tests cover.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck && pnpm build
  helm lint charts/ferristrace   (part 5)
