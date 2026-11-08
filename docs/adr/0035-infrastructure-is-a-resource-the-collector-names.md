# 0035. Infrastructure is a resource the Collector names

Status: accepted
Date: 2026-11-08

## Context
The Infrastructure page was built for Kubernetes
([lot 3](../specs/lot-3-infrastructure.md)): clusters, nodes, workloads
and pods, read from the `k8s_cluster` and `kubeletstats` metrics. Hosts
came later on their own page
([0034](0034-a-host-is-what-the-hostmetrics-receiver-reports.md)).
Docker containers, cloud instances and databases have no page.

Organisations run several clusters and also machines, containers and
databases that are not in any cluster. Each new kind of infrastructure
would today mean its own page, its own route and its own idea of what an
instance is, and nothing says which services run on it.

FerrisTrace reads only what an organisation sends
([lot 3](../specs/lot-3-infrastructure.md), invariants): no Kubernetes
API, no cloud API, no agent of its own.

## Decision
Infrastructure is a list of **resources**. A resource has a kind and an
identity, and both come from the resource attributes the Collector
already puts on metrics, spans and logs.

| Kind | Identity | Seen through |
| --- | --- | --- |
| `cluster` | `k8s.cluster.name` | `k8s.*` metrics |
| `host` | `host.id`, else `host.name` | `system.*` metrics |
| `container` | `container.name`, else `container.id`, only without `k8s.pod.name` | `container.*` metrics |
| `database` | the receiver's metric prefix and `server.address` or `service.instance.id` | `postgresql.*`, `redis.*`, and the like |

Cloud is not a kind. `cloud.provider`, `cloud.region`,
`cloud.availability_zone` and `host.type` describe a host, a container
or a cluster, so they are facets of those kinds and a way to group them.

A resource appears when it first reports and goes `silent` when it stops
for five minutes. Silence is shown as silence, never as zero.

A catalogue table, fed by a materialized view over the metric rows like
`releases`, keeps one row per tenant, project, kind and identity with
first and last seen and the few attributes used to group and label it.
Pages list from it instead of scanning attribute maps.

The identity rules live in the materialized views that fill the catalogue,
one per kind, so there is one place that decides what a resource is. A
container is named, not numbered, because `container.id` changes each time it
is recreated and would turn one container into many.

Names, cluster and the cloud facets are aggregated with `max`, so a report
that lacks them never blanks what an earlier one carried.

Kubernetes keeps its own detail view. A cluster is one resource in the
list, and opening it shows what Infra shows today.

## Consequences
- A new kind is one identity rule, one set of health signals and one
  detail view. The list, the filters, the empty state and the links from
  services do not change.
- Nothing is declared in the product. A resource that is not instrumented
  does not exist, and the empty state says which receiver to add.
- Two Collectors that name the same host differently show two resources.
  Fixing names is the sender's job.
- A database receiver that sets neither `server.address` nor
  `service.instance.id` cannot be told apart from another one of the same
  type, and the kind says so instead of guessing.
- The catalogue is derived data. It can be rebuilt from the metric rows,
  and a retention shorter than a resource's silence removes it.
- Cloud managed services that report no metrics through the Collector
  stay out of scope, since reading them would need a cloud API.
