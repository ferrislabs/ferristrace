# 0036. ClickHouse keeps a week local and the rest in object storage

Status: accepted
Date: 2026-11-12

## Context
Every signal lives in ClickHouse MergeTree tables on the server's own
volume, and rows leave only by `TTL expire_at DELETE`
([0011](0011-the-server-migrates-what-it-owns-on-start.md), lot 0
retention). The volume holds the whole retention window, up to 90 days on
the cloud plan and as long as the owner wants when self-hosted.

The interface reads the last hours and days far more often than the rest.
Old parts still sit on the fast disk, still take part in merges, and the
volume grows with retention, not with use. Storage and compute grow
together on the same machine.

The deployment is one Scaleway Dedibox running a single-node k3s with
`local-path` volumes, so every volume shares one disk today. The Ferrislabs
stacks use RustFS as their S3-compatible store, inside the cluster, never
a managed service.

Parquet files written to S3 and an Iceberg catalogue were weighed. Both
give an open format, but the interface could no longer read old data
without a second read path, and Iceberg adds a catalogue, compaction and
snapshot expiry to run. Neither was chosen for this goal. Parquet stays the
right format for the organisation export, which is a separate change.

## Decision
ClickHouse keeps the last **7 days** on its local volume and moves older
parts to an S3 disk on a **RustFS dedicated to FerrisTrace**. Parts stay in
the MergeTree format, so every query reads hot and cold parts the same way
and nothing in the read path changes.

The operator declares a storage policy named `tiered` with two volumes:
`hot` (the local disk) and `cold` (the S3 disk, behind a local cache). The
server does not create disks or policies. It only asks for the policy.

The server owns the TTL. With `spans.cold_after_days` set, it sets on each
large signal table, at start and after migrations:

    storage_policy = 'tiered'
    TTL <time> + INTERVAL <n> DAY TO VOLUME 'cold', expire_at DELETE

Without the key, the TTL stays `expire_at DELETE` and the policy is left
alone, so a self-hosted stack without object storage is unchanged.

The tables that move are the ones that grow with traffic: `spans`,
`span_events`, `logs`, `metric_points`, `metric_histograms`,
`metric_exemplars`, `llm_calls`. Catalogues (`metric_catalogue`,
`releases`, `infra_resources`) stay local: they are small and read on
every page.

If the key is set and the server finds no `tiered` policy with a `cold`
volume, it refuses to start and names the missing policy. Silently keeping
everything local would fill the disk the operator thinks is relieved.

## Consequences
- On the single Dedibox, RustFS writes to the same disk, so total capacity
  does not grow yet. What changes now is that merges and the fast volume
  only carry a week. Capacity grows the day RustFS moves to a node with
  larger or cheaper disks, without touching ClickHouse.
- An S3 disk keeps its metadata on the local volume. The ClickHouse volume
  and the RustFS bucket are one backup: losing either loses the cold data.
- If RustFS is down, queries that touch cold parts fail and moves wait.
  Ingest and queries on the last week keep working.
- RustFS is still a beta and runs as one replica. That is accepted for the
  start of the project and is the first thing to revisit before a paying
  customer depends on long retention.
- `storage_policy` cannot go back to `default` once tables use `tiered`.
  Unsetting the key stops new moves; parts already on `cold` stay there
  until their `expire_at`.
- The TTL is changed with `materialize_ttl_after_modify = 0`, so a restart
  does not rewrite existing parts. They move as background moves reach
  them.
