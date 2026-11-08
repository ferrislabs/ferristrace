# Lot 12: infrastructure beyond one cluster

Goal
  See every cluster and every other piece of infrastructure an
  organisation runs in one place: hosts, Docker containers, databases and
  the cloud they sit in, with Kubernetes clusters as one kind among them.
  Start from the whole estate and go down into one resource.

Decisions (ADR 0035)
  D1  Infrastructure is a list of resources of a kind. The kind and the
      identity are read from resource attributes. Nothing is declared.
  D2  Cloud is a facet, not a kind.
  D3  The overview is multi-cluster first: clusters are the first group,
      other kinds follow.
  D4  The Kubernetes view stays as the detail of a cluster.

Glossary
  Resource   one cluster, host, container or database, by kind and identity.
  Silent     no metric for five minutes. Not the same as unhealthy.
  Facet      cloud provider, region, zone, host type, environment: used to
             group and filter, not to identify.

Invariants
  - Every read filters on the organisation and narrows project-bound
    read keys.  [test]
  - Nothing reads an external API: a resource exists only because the
    organisation sent telemetry for it.  [review]
  - A resource is one row however many pods, cores or series report it.  [test]
  - A pod's container is not also a `container` resource.  [test]
  - A silent resource is marked silent and carries no figures, never zeros.  [test]
  - A hostile attribute value is bound, never spliced.  [test]

Out of scope
  - Reading cloud or Kubernetes APIs, or installing anything on a host.
  - Cost, rightsizing, autoscaling advice.
  - Managed cloud services that send no telemetry.
  - Declaring or renaming resources by hand (later, if the need shows).

Design
  12A Catalogue
    Domain: `Resource`, `ResourceKind`, `InfraFilter` and an `InfraReader`
    port. Silence is settled in the domain from the last report. The
    identity rules of ADR 0035 live in the materialized views, one per kind.
    ClickHouse: `infra_resources`, an AggregatingMergeTree, fed by a
    materialized view over `metric_points`, with a backfill from existing
    rows like `releases`.
    API: `GET /api/orgs/{org}/infrastructure?kind&cluster&environment&
    provider&region&status&project` answering `{resources:[..]}`. Group
    counts are derived by the reader of the list.
  12B Overview
    The Infrastructure page lists resources grouped by kind, clusters
    first, with one card per cluster (nodes ready, unhealthy pods, silent
    or not) and one row per other resource. Filters: kind, cluster,
    provider, region, environment, status. Opening a cluster shows today's
    view; opening a host shows today's System metrics rows. The empty state
    names the receiver each kind needs. System metrics becomes a link into
    the host list.
  12C Docker containers
    Identity and health for `container.*` metrics outside Kubernetes: CPU,
    memory, restarts where reported, image and host. Detail view.
  12D Cloud facets
    `cloud.*` and `host.type` on hosts, containers and clusters as filters
    and as a grouping of the overview.
  12E Databases
    Postgres, Redis and ClickHouse first, each with the three or four
    figures that tell a healthy one from a sick one (connections, memory,
    replication lag, evictions). One detail view per type.
  12F From a service to its infrastructure
    A service page lists the resources its spans ran on in the window, and
    a resource lists the services seen on it, from the resource attributes
    already stored on spans.

Acceptance
  @spec-l12-1  Metrics from two clusters and one host produce two cluster
               resources and one host resource, each listed once.
  @spec-l12-2  A resource that stops reporting is listed as silent after
               five minutes, with no figures.
  @spec-l12-3  A container inside a pod is not listed as a container
               resource.
  @spec-l12-4  The list narrows by kind, cluster, provider, region and
               environment, and by the project of a project-bound key.
  @spec-l12-5  Opening a cluster shows the current Kubernetes view for
               that cluster only.
  @spec-l12-6  An empty organisation sees, for each kind, the receiver to
               add.

Decided
  Q1  A container is identified by `container.name`, else `container.id`.
  Q3  Five minutes of silence for every kind; the interval is not read.
  Open for later parts: database identity (12E) and alerts on a resource.

Delivered with 12A to 12C
  The catalogue, the route, the overview page with kind, status, provider,
  region and environment filters, clusters as cards, hosts and containers
  as tables with CPU and memory for containers. The cloud facets (12D) are
  already stored and filterable, so 12D only has to add what is missing.

Delivered with 12F
  Each resource lists up to 20 services whose spans ran on it in the last
  10 minutes, read from the resource attributes stored on spans. The
  overview shows them, and a service page has a "Runs on" card built from
  the same list, so there is no second route.
