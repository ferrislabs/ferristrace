# Lot 3: infrastructure

Goal
  Put pods, nodes, workloads and deploys next to the services, traces,
  logs and alerts they explain, from what the OpenTelemetry Collector
  already sends.

Context
  Milestone "3 · Infrastructure": #125, #181, #182, #183. Since lot 1,
  spans, log records and metric points carry their resource attributes
  merged in, so `k8s.*` set by the Collector's `k8sattributes` processor
  and `service.version` are stored on every row. Nothing filters on them
  as first-class fields, the Infrastructure and Releases pages read mocks,
  Overview's deploy markers return nothing, and incident timelines and
  postmortem drafts have no deploy entries (lot 2 amendments). The
  Collector's `k8s_cluster` receiver emits pod phase, container restarts
  and node conditions as metrics; `kubeletstats` emits pod CPU and memory.

  One pull request per part:
    3A  Kubernetes attributes as filters             #181
    3B  Cluster view from Collector metrics          #182
    3C  Releases and deploy markers                  #125
    3D  Alerts name the pods and version             #183

Glossary
  Workload   a Deployment, StatefulSet, DaemonSet or Job, from
             `k8s.deployment.name`, `k8s.statefulset.name`,
             `k8s.daemonset.name`, `k8s.job.name`.
  Release    one `service.version` of one service: first seen, last seen,
             environments, counts.
  Deploy     the moment a new release of a service starts receiving
             traffic: its first span, or an explicit deploy event.

Invariants
  - Every new read filters on the organisation and narrows project-bound
    read keys, like lot 0 and lot 1.  [test]
  - The cluster view derives only from what the organisation sent; there
    is no Kubernetes API access from FerrisTrace.  [review]
  - A deploy is recorded once per service, version and environment, even
    when spans with that version arrive from many pods.  [test]
  - Alert notifications never grow without bound: at most 5 pods named,
    the rest counted.  [test]

Out of scope
  - Docker hosts and containers (the mock "Docker" tab): no receiver
    contract yet; the tab is removed until a later issue.
  - Kubernetes events (`k8s_events` receiver) beyond what logs already
    store; they show as logs.
  - Cost, rightsizing, autoscaling advice.

Design
  3A Filters
    A shared `Placement` filter (namespace, workload kind and name, pod,
    node, cluster from `k8s.cluster.name`) accepted by the traces, services,
    operations, series, logs and metric series queries and their HTTP
    routes (`namespace`, `workload`, `pod`, `node`, `cluster` query params),
    read from the attribute maps; ClickHouse gets materialized columns
    `k8s_namespace`, `k8s_pod`, `k8s_node`, `k8s_workload` on spans, logs,
    metric_points and metric_histograms (`MATERIALIZED attributes['…']`,
    plus bloom filters) so filters do not scan maps. A placement picker in
    the web query bars, and pod/node/workload chips on span and log rows.
  3B Cluster view
    A `ClusterReader` over the metric tables: latest value per pod of
    `k8s.pod.phase` (1 Pending … 5 Unknown), `k8s.container.restarts`
    summed per pod, `k8s.pod.cpu.utilization` or `k8s.pod.cpu.usage`,
    `k8s.pod.memory.working_set`, `k8s.node.condition_ready`,
    `k8s.node.cpu.utilization`, `k8s.node.memory.working_set`, within the
    last 10 minutes; pods grouped by node, namespace and workload; a pod is
    unhealthy when not Running/Succeeded, restarted in the window, or its
    node is not ready. HTTP: `GET /api/orgs/{org}/cluster?cluster&namespace`
    → `{"clusters":[..],"nodes":[{"name","ready","cpu","memory","pods"}],
    "workloads":[{"kind","name","namespace","desired"?,"ready","pods"}],
    "pods":[{"name","namespace","workload","node","phase","restarts",
    "cpu","memory","services":[..],"unhealthy","reason"}],"seen_ms"}`;
    pod rows link to traces and logs filtered by pod (3A). Web:
    Infrastructure page wired, Docker tab removed, a "Kubernetes not
    reporting" empty state with the Collector configuration to add.
  3C Releases and deploys
    ClickHouse: `releases` (tenant, project, service, version, environment,
    first_seen, last_seen) as an AggregatingMergeTree fed by a
    materialized view over spans where `service.version` is set; deploy
    events accepted as log records with `event.name = "deploy"` (or
    `deployment.event`) carrying service, version and environment, which
    also set first_seen. Reads: release list per service (first and last
    seen, environments, request/error counts, new issues = fingerprints
    first seen in that release), release detail (error rate in the hour
    before and after the deploy, issues introduced), and deploy markers in
    a window. Incidents: a deploy of a service named in an open incident's
    alert, or of any service when the incident has no alert, appends a
    `deploy` timeline entry; postmortem drafts gain a "Deploys" section.
    HTTP: `GET /api/orgs/{org}/releases?service&from&to`,
    `GET /api/orgs/{org}/releases/{service}/{version}`,
    `GET /api/orgs/{org}/deploys?from&to&service`. Web: Releases page and
    detail wired, deploy markers on Overview, Services and Metrics charts
    linking to the release.
  3D Alerts name what fired
    When a rule fires, the probe also returns the placement of what
    crossed the threshold: for trace and log signals the distinct pods,
    nodes and `service.version` of the matching rows in the window (top 5
    by count), for metric signals the series labels; the alert event,
    the notification (email and webhook `context` field) and the incident
    timeline entry carry them.

Acceptance
  @spec-l3-1  Traces, logs and metric series filtered by pod, node or
              workload return only rows from that placement, set by the
              Collector on the resource.
  @spec-l3-2  Pods reported by the Collector receivers appear by node with
              phase, restarts, CPU and memory; a crash-looping pod is
              marked unhealthy with its reason.
  @spec-l3-3  Spans with a new `service.version` create one release and
              one deploy marker; a deploy log record does too; the release
              shows the issues first seen in it and the error rate before
              and after.
  @spec-l3-4  A deploy during an open incident appears in its timeline and
              in the postmortem draft.
  @spec-l3-5  A firing alert's email and webhook name up to 5 pods, the
              node and the version behind the value.

Simulation
  No: reads over stored data; the deploy detection is a materialized view
  tested against a real ClickHouse.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck && pnpm build

Amended: 2026-11-02 3A as built: a bare workload name matches the workload that owns the pod (a CronJob's pod matches the CronJob; `job:<name>` reaches its Job), so it stays on the indexed column; readers always filter on the materialized columns, which ClickHouse computes from the maps for parts written before the migration, so old rows are found without an index; a trace matches when any of its spans in the window ran in the placement; exemplars match through spans that started inside the window; the alert editor gains no placement field yet, the API accepts one.
Amended: 2026-11-02 3B as built: `GET /api/orgs/{org}/cluster` also accepts `project` (narrowed for project-bound read keys) and returns namespaces, node memory available, pod ready and restarts total; workloads are keyed by cluster, namespace, kind and name, so "All clusters" never merges two clusters; Job and CronJob desired and ready counts are not read yet.
Amended: 2026-11-02 3C as built: `releases` is keyed by project too and backfilled from existing spans; a release in the API is a service and version with its environments, a deploy marker is per environment; `from` and `to` are required on `/releases` and `/deploys`; an issue belongs to a release until the next version of that service appears; error rates compare the service one hour before and after first seen; the incident `deploy` kind is new, exactly once per incident, service, version and environment through a partial unique index; a duty every 60 s reads deploys from a Postgres watermark minus 5 minutes and appends them to incidents open at that time whose alert names the service, or names no service, or that have no alert; deploy logs carry `event.name` as an attribute since the OTLP event_name field is not decoded yet; request counts sum at insert, so a retried batch counts twice.
