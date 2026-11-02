# Self-hosting FerrisTrace

One command brings up the server, its control plane, the span store and the
usage counter:

```sh
cd deploy
docker compose up --build --detach
```

The server waits for all three stores to report healthy, applies the
Postgres and ClickHouse migrations itself, and then listens on 4317 for
OTLP over gRPC and 4318 for everything else: OTLP over HTTP, the API, and
the web application at <http://localhost:4318>.

## Make it usable

A fresh deployment holds no account and no organisation, so nothing can
sign in and no key can authenticate a payload. One command fixes all of
that:

```sh
docker compose exec server ferristrace bootstrap --email you@example.com
```

It creates the account, creates an organisation owned by it, and prints a
single-use token on stdout. The account has no password until you spend
that token:

```sh
curl -X POST http://localhost:4318/auth/reset/confirm \
  -H 'x-requested-with: cli' \
  -H 'content-type: application/json' \
  -d '{"token":"<the printed token>","password":"<at least twelve characters>"}'
```

`bootstrap` does nothing if an organisation already exists, so it is safe
to run again.

## Send a trace

Draw an ingest key for the organisation `bootstrap` named:

```sh
docker compose exec server ferristrace issue-key --org <the printed org id>
```

An owner or an administrator can also create one in the web application,
under Settings > Project > Api keys, with the `ingest` scope. A key with
the `read` scope reads services, traces and issues through `/api` with the
same bearer header, and a key bound to a project reads only that project.

Every change made to an organisation, from the web application or from the
command line, is recorded once in its audit log: who acted (an account, or
`operator` for the command line), what was done, to what, and when. Owners
and administrators read it under Settings > Organisation > Audit log, or
through `GET /api/orgs/<org>/audit`. No entry carries a key secret, a
token or a password, and nothing in FerrisTrace edits or deletes an entry;
deleting the organisation deletes its log.

Then point any OTLP exporter at `http://localhost:4318/v1/traces` or
`localhost:4317`, with the key as a bearer token:

```sh
curl -X POST http://localhost:4318/v1/traces \
  -H "authorization: Bearer <the printed key>" \
  -H 'content-type: application/x-protobuf' \
  --data-binary @payload.pb
```

## Coming from a Comettrace deployment

The product was called Comettrace before. A stack started under that name
does not carry over:

- settings are read from `FERRISTRACE_*`; `COMETTRACE_*` is ignored, so
  rename every line of your `.env`
- the compose project, the Postgres and ClickHouse database, user and
  password are `ferristrace`, so the volumes of a `comettrace` project are
  not reused
- ingest keys (`ft_live_`), sessions and reset links (`ft_pr_`) carry a new
  prefix, so every key issued before has to be issued again and everybody
  signs in again

On a machine whose data can go, bring the old stack down with its volumes
and start this one:

```sh
docker compose -p comettrace down --volumes
docker compose up --build --detach
docker compose exec server ferristrace bootstrap --email you@example.com
```

## Using a published image instead of building

Images land on `ghcr.io/ferrislabs/ferristrace` for every released tag:
`1.2.3` is the community build and `1.2.3-ee` the cloud one, both for
linux/amd64 and linux/arm64.

```sh
export FERRISTRACE_IMAGE=ghcr.io/ferrislabs/ferristrace:1.2.3
docker compose up --detach --no-build --pull always
```

Without `FERRISTRACE_IMAGE` the compose file builds from this checkout,
which is what `--build` above does.

Every published image is signed keyless with cosign, bound to the release
workflow's identity and recorded in the Sigstore transparency log:

```sh
cosign verify ghcr.io/ferrislabs/ferristrace:1.2.3 \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  --certificate-identity-regexp '^https://github.com/ferrislabs/ferristrace/\.github/workflows/docker\.yaml@'
```

## What to change before this faces anything real

- **The passwords.** Postgres, ClickHouse and the server all default to
  `ferristrace` so that `docker compose up` works on a clean machine with
  no preparation. ClickHouse and Redis are reachable only from inside the
  compose network; Postgres answers this machine too, on the loopback, so
  a database client can reach it. Set `POSTGRES_PASSWORD` and
  `CLICKHOUSE_PASSWORD` in a `.env` file beside the compose file for
  anything that is not a laptop — and before widening `POSTGRES_BIND`,
  which is what would put that database on the network.
- **`COOKIE_SECURE`.** It defaults to `false` because this compose file
  serves plain HTTP. Put the server behind TLS and set it to `true`, or
  session cookies travel in the clear.
- **`RETENTION_DAYS`.** Spans carry a TTL of 30 days by default. ClickHouse
  drops them on its own schedule once past it. An owner can set a different
  retention for one organisation in Settings, Organisation, General, between
  1 day and the plan's own limit when it has one; it applies to spans
  ingested afterwards, and `RETENTION_DAYS` stays the value for every
  organisation that sets none.

## Settings

Every field of `ferristrace.toml` can be given as an environment variable:
uppercase, prefixed with `FERRISTRACE_`, with `__` between a section and
its field. `FERRISTRACE_SPANS__RETENTION_DAYS` is `spans.retention_days`.

Put them in a `.env` beside the compose file. `.env.example` lists every
one of them, commented out, so copying it changes nothing:

```sh
cp .env.example .env
```

That file does two jobs, and it is worth knowing which line does which. The
names without the prefix — `POSTGRES_PASSWORD`, `OTLP_HTTP_PORT` — are read
by the compose file, to fill in a password or a port. The ones with it are
handed to the server through `env_file`, which is how a setting the compose
file never mentions still reaches it. The handful the compose file sets
itself win over the file, because they are how the server reaches the three
stores inside its own network rather than something to choose.

| Variable | Default | What it does |
| --- | --- | --- |
| `POSTGRES_PASSWORD` | `ferristrace` | The control plane password |
| `CLICKHOUSE_PASSWORD` | `ferristrace` | The span store password |
| `RETENTION_DAYS` | `30` | How long a span is kept |
| `COOKIE_SECURE` | `false` | Whether a session cookie demands HTTPS |
| `OTLP_GRPC_PORT` | `4317` | Host port for OTLP over gRPC |
| `OTLP_HTTP_PORT` | `4318` | Host port for OTLP over HTTP |
| `RUST_LOG` | `info` | Log filter |
| `POSTGRES_BIND` | `127.0.0.1` | Which address the control plane answers on |
| `POSTGRES_PORT` | `5432` | Host port for the control plane |
| `FERRISTRACE_SECRETS__AT_REST_KEY` | unset | 32 bytes as hex, without which no second factor is offered |
| `FERRISTRACE_WEB_ROOT` | set in the image | Where the built web application lives |
| `FERRISTRACE_IMAGE` | built locally | A published image to run instead |

## Signing in with an identity provider instead

Set the OIDC block in `.env` and local passwords become unnecessary:

```sh
FERRISTRACE_OIDC__ISSUER=https://id.example.com
FERRISTRACE_OIDC__CLIENT_ID=ferristrace
FERRISTRACE_OIDC__CLIENT_SECRET=...
FERRISTRACE_OIDC__REDIRECT_URI=https://ferristrace.example.com/auth/oidc/callback
FERRISTRACE_LOCAL_PASSWORD=false
```

An account is provisioned the first time somebody signs in, so run
`bootstrap` with that person's address afterwards to give them an
organisation to own. It prints no reset token in that case: there is no
password to set, and `/auth/reset/*` is not mounted when local passwords
are off.

## A second factor

An account can keep an authenticator app as a second step after its
password. The server will not offer one without a key to seal the shared
secret with, because the alternative is a totp secret in clear in a column
that lands in every backup:

```sh
FERRISTRACE_SECRETS__AT_REST_KEY=$(openssl rand -hex 32)
```

Keep that key where the database password is kept. It is not derivable and
it is not stored anywhere else: lose it and every enrolled account has to
enrol again, because what is in `auth_factors` no longer opens. Rotating it
is the same operation by hand — there is no rotation command yet.

With the key set, `/auth/2fa/app` enrols, `/auth/2fa/app/confirm` turns the
enrolment on and answers ten recovery codes, and a password for an account
that keeps a confirmed factor answers `200` with a challenge cookie instead
of a session. The sign-in screen drives that second step: the password form
gives way to a field that takes six digits or a recovery code. Recovery codes
are shown once, hashed at rest, and spent one at a time; `ferristrace` has no
command to clear a factor, so an account that loses both its app and its
codes is unlocked with SQL:

```sh
docker compose exec postgres psql -U ferristrace -d ferristrace \
  -c "DELETE FROM auth_factors WHERE account_id = (SELECT id FROM accounts WHERE email = 'person@example.com')"
```

## Profile photos

Without a bucket, everybody is shown under their initials, taken from the
name they wrote or from their address. Give the server one and the profile
screen gets an upload control:

```sh
FERRISTRACE_OBJECTS__ENDPOINT=https://<account id>.r2.cloudflarestorage.com
FERRISTRACE_OBJECTS__BUCKET=ferristrace-avatars
FERRISTRACE_OBJECTS__ACCESS_KEY_ID=...
FERRISTRACE_OBJECTS__SECRET_ACCESS_KEY=...
```

Cloudflare R2 is what this was built against. The protocol is S3, so an S3
bucket answers it, and so does a MinIO; R2 has one region and calls it
`auto`, which is what the server assumes when `FERRISTRACE_OBJECTS__REGION`
is left out. Nothing goes in the compose file: `env_file` hands anything
prefixed `FERRISTRACE_` to the server already.

The bucket needs no public access and should not have any. A browser never
talks to it: the server reads the bytes and serves them from its own origin,
under a url that carries a token which changes every time somebody replaces
their photo. That is what lets those urls be cached for a year without ever
showing a face somebody deleted.

A photo is at most 1 MB and must really be a PNG, a JPEG or a WebP — the
first bytes decide, not the content type the browser announced, because an
avatar is handed back to every reader under this deployment's own origin.
Nothing is resized: 1 MB is small enough that a resizer would be a
dependency rather than a saving.

## Adding somebody

Settings > Organization > Members does it: an owner or an administrator
invites an address, and gets back one link that sets that account's
password. The link lasts 24 hours and can be spent once, and it is shown
once — nothing can read it back, because the control plane keeps only its
digest. Send it through whatever channel you already trust.

An address that already has an account here joins without a link: it signs
in with the password it already chose. Nothing on that screen can set a
password for an account somebody else signs in with.

Only an owner hands out the owner role, changes an owner or removes one,
and an organisation keeps at least one owner. So the sole owner of a fresh
deployment can rename the organisation, invite anybody and change anybody
else, but cannot stop being the owner until it has made somebody else one —
after which it can step down, or leave. An organisation with no owner left
could only be rescued with `grant-role`, which is why that is refused.

Somebody who has lost their password asks for a link on the sign-in
screen, which needs the mail relay. Without one, print a link yourself:

```sh
docker compose exec server ferristrace reset-password --email somebody@example.com
```

`create-account` and `grant-role` still do the same job from the command
line, which is how you add the first person when nobody can sign in yet.

## Projects

Telemetry arrives under a project, and `bootstrap` makes the first one,
`default`. Settings > Organization > Projects makes the rest, renames them,
hands them to a team and deletes them; `create-project` still does the
first two from the command line.

A name is a slug — lowercase letters, digits and dashes — because it is
typed into a configuration file beside an ingest key and read out of a url.
What you type is lowered rather than refused, so `Payments-EU` is kept as
`payments-eu`.

Renaming keeps the keys and the signals: a key carries the project id and
so does every span, and nothing anywhere holds the name. Deleting is not
the same bargain. The api keys go with the project at once, and the spans
already written stay in the span store until their retention expires —
unreachable from the application, because nothing names them any more.

How long a signal is kept is a property of the organisation rather than of
one project: an owner sets it in Settings, Organisation, General, and
`spans.retention_days` is the value for an organisation that sets none.
ClickHouse applies it on its own schedule.

## Export

An owner downloads everything the organisation holds from Settings,
Organisation, General, Export organisation data, or with
`GET /api/orgs/{org}/export` and the session cookie (an API key does not
open it). The answer is a streamed `application/gzip` named
`ferristrace-<org-id>-<YYYYMMDD>.tar.gz`: nothing is written to disk on the
server and no more than one page of spans is held in memory at a time. Each
export is recorded in the audit log as `org.exported`.

The archive holds, in this order:

- `control-plane.json`: `format` (`ferristrace-export`), `version` (`1`),
  `exported_at_ms`, then `org` (`id`, `name`, `plan`, `created_at_ms`,
  `retention_days`, null when the deployment default applies), `projects`,
  `teams` (with their `members` and `projects`), `members` (`account_id`,
  `email`, `name`, `role`, `joined_at_ms`), `keys` (`id`, `name`, `prefix`,
  `project`, `scopes`, `created_at_ms`, `created_by`, `last_used_at_ms`; never
  a secret or a digest, and revoked keys are left out), `issue_decisions`
  (`project_id`, `fingerprint`, `status`, `owner_id`, `decided_at_ms`,
  `decided_by`) and `audit_entries`, newest first.
- `spans/000001.ndjson`, `spans/000002.ndjson`, and so on: one file per page
  of up to 10000 spans, because a tar entry needs its size before its
  content and a page is the most the server buffers. There are none when the
  organisation holds no spans. Spans run in order of service name, start
  hour, trace id and span id, and each appears exactly once.

A line of a spans file is one JSON object:

```json
{"trace_id":"0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a","span_id":"0b0b0b0b0b0b0b0b",
 "parent_span_id":null,"project_id":"6f0f5b4e-0000-0000-0000-000000000000",
 "service_name":"checkout","name":"POST /pay","kind":"server","status":"error",
 "start_unix_nanos":1700000000000000000,"end_unix_nanos":1700000001000000000,
 "attributes":{"http.route":"/pay"},
 "events":[{"time_unix_nanos":1700000000000000500,"name":"exception",
            "attributes":{"exception.type":"Timeout"}}]}
```

Ids are lowercase hex; `parent_span_id` and `project_id` are null for a root
span and for a span written before it had a project. `kind` is one of
`unspecified`, `internal`, `server`, `client`, `producer` or `consumer`;
`status` is `unset`, `ok` or `error`. Times are integers in nanoseconds since
the epoch, which exceed 2^53, so read them with a parser that keeps 64 bits.
Export needs the span store: without ClickHouse the route answers 503.

## Multi-tenancy

Organisations exist and every span carries one, but a self-hosted
deployment is expected to have exactly one. `bootstrap` creates it and
refuses to create a second. Nothing stops you adding more with
`create-org` and `grant-role`.

## Looking at the databases

Postgres answers `localhost:5432` as `ferristrace` / `ferristrace`, so any
client reaches it:

```sh
psql postgres://ferristrace:ferristrace@localhost:5432/ferristrace
```

ClickHouse and Redis are not published. Reach them through the container:

```sh
docker compose exec clickhouse clickhouse-client \
  --user=ferristrace --password=ferristrace --database=ferristrace
docker compose exec redis redis-cli
```

## Changing the front end without rebuilding anything

The container serves the built application, so a change to `web/` would
mean a new image. It does not have to: the dev server proxies every server
path to the container, so run it beside the stack instead.

```sh
docker compose up --detach          # the server, on 4318
cd ../web && pnpm install && pnpm dev
```

Then open <http://localhost:5173>. Every change appears without a reload,
and `/auth`, `/api`, `/health` and `/v1` go to the real server on 4318. A
session cookie is set for `localhost` and ignores the port, so signing in
on either one signs you in on both.

`FERRISTRACE_SERVER` points the proxy somewhere other than
`http://localhost:4318`.

Rebuild the image when you change Rust, or to check what the single
container actually serves.

## Kubernetes

A Helm chart lives in [`charts/ferristrace`](../charts/ferristrace): one
Deployment, with Postgres, ClickHouse and Redis supplied from outside and
every credential read from an existing Secret by name. Published to
`oci://ghcr.io/ferrislabs/charts/ferristrace`. An ArgoCD `Application`
example is in [`argocd/ferristrace-application.yaml`](argocd/ferristrace-application.yaml).
