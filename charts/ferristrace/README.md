# ferristrace

![Version: 0.1.0](https://img.shields.io/badge/Version-0.1.0-informational?style=flat-square) ![AppVersion: 0.1.0](https://img.shields.io/badge/AppVersion-0.1.0-informational?style=flat-square)

FerrisTrace as one Deployment. Postgres, ClickHouse and Redis are not part of
this chart: bring your own (Postgres typically through CloudNativePG). The
server applies both databases' migrations itself on start.

## Installation

```sh
helm install ferristrace oci://ghcr.io/ferrislabs/charts/ferristrace \
  --version 0.1.0 -n ferristrace --create-namespace -f my-values.yaml
```

An ArgoCD example is in `deploy/argocd/ferristrace-application.yaml`.

## Secrets

Nothing secret goes in values. Create the Secrets out of band (External
Secrets, OpenBao, sealed secrets or by hand) in the release namespace and name
them under `secrets.*`. Each entry is `{name, key}`.

| Value | Env var | Needed |
|-------|---------|--------|
| `secrets.databaseUrl` | `FERRISTRACE_DATABASE_URL` | always (default: CloudNativePG `ferristrace-app`, key `uri`) |
| `secrets.redisUrl` | `FERRISTRACE_REDIS_URL` | always |
| `secrets.clickhousePassword` | `FERRISTRACE_SPANS__PASSWORD` | when `name` is set |
| `secrets.atRestKey` | `FERRISTRACE_SECRETS__AT_REST_KEY` | when `name` is set; without it no second factor can be enrolled. Mint with `openssl rand -hex 32` and keep it: losing it forces every enrolled account to re-enrol |
| `secrets.oidcClientSecret` | `FERRISTRACE_OIDC__CLIENT_SECRET` | `config.oidc.enabled` |
| `secrets.smtpPassword` | `FERRISTRACE_MAIL__PASSWORD` | `config.mail.enabled` |
| `secrets.objectsAccessKeyId`, `secrets.objectsSecretAccessKey` | `FERRISTRACE_OBJECTS__ACCESS_KEY_ID`, `..._SECRET_ACCESS_KEY` | `config.objects.enabled` |

## Editions

`edition: community` runs `ghcr.io/ferrislabs/ferristrace:X.Y.Z`; `edition: ee`
appends `-ee`. `image.tag` overrides the version (default: chart appVersion).

## Values

| Key | Default | Description |
|-----|---------|-------------|
| `image.repository` | `ghcr.io/ferrislabs/ferristrace` | Image repository |
| `image.tag` | `""` | Tag, empty means appVersion |
| `image.pullPolicy` | `IfNotPresent` | Pull policy |
| `edition` | `community` | `community` or `ee` |
| `replicaCount` | `1` | Replicas; the PDB exists above 1 |
| `terminationGracePeriodSeconds` | `60` | Time to flush spans on SIGTERM |
| `resources` | 200m / 256Mi requests, 1Gi memory limit | Container resources |
| `podSecurityContext`, `securityContext` | non-root 65532, read-only root, no capabilities, RuntimeDefault seccomp | Hardening |
| `nodeSelector`, `tolerations`, `affinity`, `topologySpreadConstraints` | empty | Scheduling |
| `podAnnotations`, `podLabels`, `extraEnv`, `imagePullSecrets` | empty | Pass-through |
| `serviceAccount.*` | create, no token mounted | Service account |
| `service.*` | ClusterIP, 4318 http, 4317 grpc | Service |
| `httpRoute.*` | disabled | Gateway API route to the HTTP port |
| `podDisruptionBudget.*` | enabled, `minAvailable: 1` | Rendered only when replicas > 1 |
| `networkPolicy.*` | disabled | Ingress on 4317/4318, optional `ingressFrom` peers |
| `config.logLevel`, `config.cookieSecure`, `config.localPassword`, `config.afterSignIn`, `config.webRoot` | `info`, `true`, `true`, `/`, `""` | Server settings |
| `config.spans.*` | `http://clickhouse:8123`, `ferristrace`, `ferristrace`, 30 days | ClickHouse |
| `config.oidc.*`, `config.mail.*`, `config.objects.*` | disabled | Optional integrations |
| `secrets.*` | see above | Existing Secrets, by name and key |

gRPC (4317) is exposed by the Service only; route it with your own
`GRPCRoute` or a LoadBalancer Service if clients are outside the cluster.

## Validation

```sh
helm lint charts/ferristrace -f charts/ferristrace/ci/full-values.yaml
helm template t charts/ferristrace -f charts/ferristrace/ci/full-values.yaml \
  | kubeconform -strict -summary -ignore-missing-schemas
```
