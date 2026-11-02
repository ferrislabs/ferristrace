# Issues

Goal
  Exceptions that are the same exception are shown once, with a count, and a
  human can say what has been done about one.

Context
  Every exception already arrives and is already kept. It arrives as a
  `span_events` row named `exception` whose attributes hold `exception.type`,
  `exception.message` and `exception.stacktrace`
  (`crates/otlp/src/traces.rs`, decoded and tested). Nothing reads them:
  `Explore` answers services, a trace list and one trace, and
  `web/src/pages/Issues.tsx` reads `web/src/mocks/issues.ts`.

  So the work is not collection. It is grouping — deciding that two
  exceptions are one issue — and keeping the one thing ClickHouse must not
  keep: what a person decided. Spans expire on a TTL; "Inès resolved this"
  must not.

  The nearest prior art is `Explore` (`crates/core/src/explore.rs`): a use
  case that composes `Whoami` over Postgres with `SpanReader` over
  ClickHouse, and authorises every read through the membership (ADR 0015).
  Issues follow that shape, with one more port for the part a person writes.

Glossary
  issue        a group of exceptions that share a fingerprint, in one project
  fingerprint  16 bytes that decide whether two exceptions are the same one
  occurrence   one exception event that belongs to an issue
  decision     what a person decided about an issue: unresolved, resolved,
               ignored, and who owns it
  regression   an issue seen again after it was resolved

Invariants
  - Two exceptions share an issue when, and only when, they share a
    fingerprint and a project.                                        [type]
  - A fingerprint is a pure function of the occurrence. The same occurrence
    fingerprints the same way in any process, on any day.        [type+test]
  - A fingerprint ignores what varies between two instances of one error:
    digits, uuids, hex blobs, quoted strings, paths before the last segment.
                                                                       [test]
  - An issue nobody has touched is unresolved. No row is written to say so.
                                                                       [test]
  - A decision outlives every occurrence that produced it.             [test]
  - An issue is read and written only through a membership that authorises
    the organisation it belongs to.                                    [test]
  - Resolving an issue that is seen again marks it regressed, and does not
    silently reopen it.                                                [test]
  - Counting is scoped to a window. An issue with no occurrence in the window
    is not in the answer, whatever its decision.                       [test]

Out of scope
  - An error span that carries no exception event. A service that reports
    failure through `status = error` and never records an exception has no
    issue. Adding it means a second fingerprint input, which means a second
    column by the decision below, so it is a migration and not an
    afterthought. PR2.
  - A stack trace with source code, like the mock draws. We hold
    `exception.stacktrace` as the string the sdk sent and nothing else. No
    source, no source maps, no in-app detection. The screen renders the
    string; the file-and-code panel goes away.
  - Affected users. There is no user attribute convention in the data today.
    PR2, once `user.id` is documented, as `uniq()` over it.
  - A short name per project (`STOREFRONT-142`). That is a Postgres sequence
    per project and a migration; the fingerprint's first 8 hex are the handle
    until then.
  - Comments and an activity feed. PR3. The decision table carries who
    changed it and when, which is the half that matters for an audit.
  - Alerting on a new issue. That is the Alerts chantier and it reads this
    one; `first_seen` is the hook it will use.
  - Grouping across projects. A fingerprint is scoped to a project on
    purpose: the same library throwing in two services of two projects is two
    issues, because two different people own them.
  - Merging and splitting issues by hand. It needs a fingerprint alias table.
    PR3.
  - Filtering the list by status on the server. The status lives in Postgres
    and the LIMIT is applied in ClickHouse, so filtering after the join would
    answer three rows for a page of fifty. The screen filters the page it
    holds, and the tab counts are counts of that page. A server-side filter
    needs the decision table to be the thing paged, which is a different
    query. PR2.

Design
  Domain
    `crates/core/src/issue.rs`
      `Fingerprint`  16 bytes, hex in and out, `TryFrom<&str>`
      `Occurrence`   what an exception looks like before it is grouped:
                     service, type, message, trace and span id, time
      `Counted`      what ClickHouse answers: fingerprint, type, message,
                     service, count, first_seen, last_seen
      `Status`       `Unresolved`, `Resolved`, `Ignored`
      `Decision`     status, owner, decided_at, decided_by
      `Issue`        a `Counted` and its `Decision`, plus `regressed`
      `Recorded`     one `Decision` with the project and fingerprint it is for
      `IssueFilter`  window, project, status, service, text, limit
      `Issues`       the use case: list, one, occurrences, decide, assign

    Fingerprinting is a free function in the domain, not SQL:

      fn fingerprint(of: &Occurrence) -> Fingerprint

    sha256 over the service name, the exception type and the normalised
    message, truncated to 16 bytes. Normalisation replaces every run of
    digits with `0`, every uuid and hex run over 8 characters with `x`, every
    single- or double-quoted run with `''`, and keeps only the last segment
    of anything that looks like a path. The rules are in one function with a
    test per rule, because they are the feature: get them wrong and every
    exception is its own issue, or all of them are one.

  Ports
    `IssueReader` (issue.rs, static dispatch)
      over ClickHouse, counting and sampling: `issues`, `occurrences`.
    `IssueDecisions` (issue.rs, static dispatch)
      over Postgres, what a person wrote: `read_many`, `decide`, `assign`.
    Two ports because they are two databases with two lifetimes. The use case
    is the only thing that knows an issue is both.

  Adapters
    `crates/storage/src/issues.rs`     `ClickhouseIssues`, over `span_events`
                                       where `name = 'exception'`
    `crates/storage/src/decisions.rs`  `PostgresDecisions`

    Migration `clickhouse/0006_event_fingerprint`: a `fingerprint`
    `FixedString(16)` column on `span_events` only, written by the sink, with
    a skip index on it. The spans table is untouched, so a span that never
    threw pays nothing.

    Migration `postgres/..._issue_decisions`: `issue_decisions`, keyed by
    `(project_id, fingerprint)`, carrying status, owner, decided_at,
    decided_by.

  Errors
    `IssueError` in the domain: `NoSession`, `NotYours`, `NoSuchIssue`,
    `Unavailable`. `IssueRefused` at the http boundary mapping to
    401/403/404/503.

  Routes (`crates/api/src/issue_routes.rs`)
    `GET   /api/orgs/{org}/issues`                      the list, filtered
    `GET   /api/orgs/{org}/issues/{print}`              one issue
    `GET   /api/orgs/{org}/issues/{print}/occurrences`  the last N, with
                                                        trace ids to jump to
    `PATCH /api/orgs/{org}/issues/{print}`              status and owner

Acceptance
  1. Two exceptions with the same type and a message differing only in digits
     are one issue with a count of two.
  2. Two exceptions with the same message and different types are two issues.
  3. The same exception in two services is two issues.
  4. The same exception in two projects is two issues.
  5. An issue never decided reads as unresolved, and `issue_decisions` holds
     no row for it.
  6. Resolving an issue writes the status, the moment and the account, and
     the list shows it resolved.
  7. An occurrence after a resolution reads as regressed, and the status
     stays resolved until somebody changes it.
  8. A member of another organisation reading the list gets 403, and the
     answer names no issue.
  9. An issue whose last occurrence is outside the window is absent from the
     list even when it is resolved.
 10. The occurrence list answers trace ids that open in the trace view.
 11. Every normalisation rule has a test where one pair groups and one pair
     does not.
 12. A fingerprint round-trips through hex and refuses anything that is not
     32 hex characters.
 13. Spans expiring on their TTL leave the decision in place, and the issue
     then falls out of the list by rule 9.
 14. An event that is not an exception never becomes an issue.

Simulation
  No. The read path has no concurrency and the clock is a port. Two people
  deciding the same issue at once is a last-write-wins `UPDATE` on one row,
  which is the behaviour we want and needs no simulation to assert.

Workstreams
  Solo and sequential. Order: `issue.rs` and the fingerprint with its tests,
  the clickhouse migration and the sink column, `ClickhouseIssues`, the
  postgres migration and `PostgresDecisions`, the use case, the routes, the
  front end, the ADR.

Frozen contracts
  ```rust
  pub struct Fingerprint([u8; 16]);

  pub fn fingerprint(of: &Occurrence) -> Fingerprint;

  pub trait IssueReader {
      fn issues(
          &self,
          tenant: TenantId,
          filter: &IssueFilter,
      ) -> impl Future<Output = Result<Vec<Counted>, ReadError>> + Send;
  }

  pub trait IssueDecisions {
      fn decide(
          &self,
          project: ProjectId,
          print: Fingerprint,
          status: Status,
          by: AccountId,
      ) -> impl Future<Output = Result<(), ReadError>> + Send;
  }
  ```

Risks / open questions
  - The fingerprint is written at ingest, so changing a normalisation rule
    regroups nothing already stored. That is the price of grouping in
    ClickHouse instead of in Rust over every row. The alternative was read
    time grouping, which cannot use an index and reads a day of exceptions
    into the server to count them. A rule change means a new column, and the
    ADR says so.
  - Normalising digits away merges `HTTP 500` with `HTTP 404` when the status
    sits in the message. The exception type is in the fingerprint precisely
    so that this stays rare, but it will happen.
  - `span_events` is ordered by `(tenant_id, service_name, hour, name,
    trace_id)`, so counting one fingerprint in a window still scans the
    `exception` range of the hours it covers. The skip index on fingerprint
    helps one issue, not the list. If the list gets slow, the answer is an
    aggregating materialised view, not a wider order by.
  - An issue list sorted by last_seen needs no new index. Sorted by count it
    reads the window whole. The list is last_seen first until that hurts.

Verify (exit)
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-storage
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-storage
  cd web && pnpm typecheck

Verify (loop)
  cargo test -p ferristrace-core issue::
