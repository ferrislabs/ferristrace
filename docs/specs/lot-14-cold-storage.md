# Lot 14: cold storage

Goal
  ClickHouse keeps the last 7 days of signal on its local volume and
  moves older parts to a RustFS dedicated to FerrisTrace, so storage can
  grow apart from compute. Queries do not change.
  Decision: [ADR 0036](../adr/0036-clickhouse-keeps-a-week-local-and-the-rest-in-object-storage.md).

Context
  Every signal table ends with `TTL expire_at DELETE`. The server applies
  ClickHouse migrations at start (ADR 0011). Production is one Dedibox,
  single-node k3s, `local-path` volumes, ClickHouse 24.8 with a 50Gi claim
  (`ferrislabs/infra`, `apps/ferristrace/clickhouse.yaml`). RustFS already
  runs for mestier (`apps/mestier/rustfs.yaml`).

Decisions
  D1  Seven days hot by default in production. The value is
      `spans.cold_after_days`, unset by default, so self-host is unchanged.
  D2  One RustFS for FerrisTrace, not shared with other stacks.
  D3  The operator owns disks and the `tiered` policy (volumes `default`,
      `cold`). The server owns the TTL and asks for the policy by name.
      The local volume keeps the name `default`: ClickHouse refuses a new
      policy that drops a volume of the old one.
  D4  Moved tables: spans, span_events, logs, metric_points,
      metric_histograms, metric_exemplars, llm_calls. Catalogues stay hot.
  D5  Not a numbered migration: tiering depends on the deployment, and a
      migration that names a missing policy would break every upgrade.

Invariants
  - Without `cold_after_days`, no table changes policy or gains a move.
  - With it, a missing `tiered` policy or `cold` volume stops the start.
  - A restart never rewrites existing parts (`materialize_ttl_after_modify
    = 0`).
  - No row is deleted earlier than its `expire_at`.

Design (14a, server)
  Config: `spans.cold_after_days: Option<NonZeroU16>`.
  Store: after `ClickhouseMigrator::up`, a tiering step reads
  `system.storage_policies`, then per moved table runs
    ALTER TABLE t MODIFY SETTING storage_policy = 'tiered'   (when set)
    ALTER TABLE t MODIFY TTL <time> + INTERVAL n DAY TO VOLUME 'cold',
                             expire_at DELETE
  or `MODIFY TTL expire_at DELETE` when unset. `<time>` is `start_time`
  for spans and `time` elsewhere.
  CI: the `clickhouse` job copies a `tiered` policy whose `cold` volume is
  a second local path into the service container and restarts it.
  Docs: self-host page on enabling cold storage with an S3 disk.

Design (14b, ferrislabs/infra)
  `apps/ferristrace/rustfs.yaml` on the mestier model: claim, Service,
  Deployment, credentials secret created by hand, bucket
  `ferristrace-cold` created by hand, both in the README.
  ClickHouse ConfigMap: S3 disk on `http://ferristrace-rustfs:9000`, a
  cache disk, the `tiered` policy. Credentials from the secret.
  Server values: `spans.cold_after_days = 7`.
  Order: 14b infra first (policy exists), then the release with 14a.

Out of scope
  - Parquet export of an organisation (separate issue).
  - Iceberg, a lake catalogue, querying cold data from outside.
  - Moving RustFS to another node, replication, erasure coding.
  - Per-organisation hot windows.

Acceptance (14a)
  @spec-l14-1  Without `cold_after_days`, every moved table keeps its
               policy and the TTL `expire_at DELETE`.
  @spec-l14-2  With 7 and a `tiered` policy, the seven tables use
               `tiered` and a TTL that moves to `cold` after 7 days and
               deletes at `expire_at`; catalogues are untouched.
  @spec-l14-3  A row 8 days old ends on the `cold` volume after
               `MATERIALIZE TTL`, a row 1 day old stays local, and one
               query returns both.
  @spec-l14-4  With the key set and no `tiered` policy, the start fails
               with an error naming the policy.
  @spec-l14-5  Two starts in a row leave the same policy and TTL.

Acceptance (14b, manual)
  A6  After deploy, `system.parts` shows parts on the S3 disk for days
      older than 7, and the `ferristrace-cold` bucket grows.

Verify (exit)
  cargo test -p ferristrace-clickhouse      (CI job `clickhouse`)
  cargo test -p ferristrace-server
  infra: kubeconform on apps/ferristrace, then A6 on the cluster.
