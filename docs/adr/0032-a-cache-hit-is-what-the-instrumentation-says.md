# 0032. A cache hit is what the instrumentation says

Status: accepted
Date: 2026-11-07

## Context
The Backend > Cache page needs a hit rate. OpenTelemetry has no stable
semantic convention for caches: a Redis or Memcached client is traced as
a database client, with `db.system.name` and `db.operation.name`, and
nothing in that span says whether the key was there. The page's mock
claimed a `GET` returning nothing counts as a miss, which no attribute of
a current SDK carries.

Two readings were available. Derive the verdict — treat a read whose
response returned no rows as a miss — which depends on
`db.response.returned_rows`, an attribute few instrumentations fill, and
silently miscounts every cache that does not. Or read a verdict the
instrumentation states, as Sentry's SDKs already do with `cache.hit`.

Grouping the table had the same shape of choice. `cache.key` is high
cardinality and often holds user data: grouped raw it fills the table
with rows of one call each and prints real identifiers in the UI, so it
is only usable once reduced to a pattern.

## Decision
A span is a cache operation when it is a client span and either carries
`cache.hit`, or names `redis`, `memcached` or `valkey` in
`db.system.name` (with the older `db.system` as a fallback).

`cache.hit = true` is a hit, `cache.hit = false` is a miss, and a span
carrying neither is counted in the calls and in no verdict. The hit rate
is `hits / (hits + misses)` and is **absent**, not zero, when the window
holds no verdict at all.

The table groups by span name and store, by store alone, or by key
pattern: `cache.key` with every run of digits, and every hexadecimal run
of eight characters or more, replaced by `*`. So `cart:4812` reads
`cart:*`, `session:a3f9b2c1d4e5` reads `session:*`, and `user:42:prefs`
keeps its shape as `user:*:prefs`. A call carrying no key reads
`unknown`. The raw key is never shown and never derived into anything
else.

## Consequences
- A stack whose cache client sets no `cache.hit` sees its calls, its
  latency and its operations, and an empty hit rate. The page must say
  what to instrument rather than draw nought per cent.
- The operation column is the span name, so it is only as readable as the
  instrumentation makes it: `GET` from `ioredis`, `cache.get` from an
  application wrapper.
- The pattern is computed at read time, so the rule can change without a
  migration, and it costs two `replaceRegexpAll` per row read. A key
  whose identifiers are plain words (`cart:winter-sale`) is not reduced,
  and a word that is all hexadecimal letters and long enough
  (`deadbeef`) is reduced although it names nothing.
- A Redis call that is not caching (a queue pop, a lock) counts as a
  cache operation. Grouping by operation keeps it visible rather than
  folded into a single number.
- Adding a store is one list in `read.rs`, not a migration.
