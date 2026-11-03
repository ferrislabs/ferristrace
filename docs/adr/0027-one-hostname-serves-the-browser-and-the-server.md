# 0027. One hostname serves the browser and the server

Status: accepted
Date: 2026-11-02

## Context
FerrisKey builds every url in its discovery document, `issuer` included,
from the request's `Host` header. Measured by hand against a standing
FerrisKey 0.10.1: asked as `localhost:3333` it answers
`"issuer":"http://localhost:3333/realms/ferristrace"`; asked as
`id.example.com` it answers `"issuer":"http://id.example.com/realms/ferristrace"`
instead. There is no public base url setting to pin it to one name.

The adapter discovers the issuer's metadata at startup
(`CoreProviderMetadata::discover_async`, `crates/infra/src/oidc.rs:46`)
and verifies each id token against it with the discovered
`id_token_verifier()` (`crates/infra/src/oidc.rs:128`), which checks the
token's `iss` against the issuer it discovered. If the browser reaches
FerrisKey under one name and the server discovers it under another, the
two `iss` values never match and every sign-in fails, no matter how
correct the rest of the flow is.

On a developer's machine this is a real conflict, measured candidate by
candidate against the same stack:
  - `extra_hosts: ["ferriskey:host-gateway"]` on the `server` service lets
    the server reach the published port as `ferriskey`; it then discovers
    `http://ferriskey:3333/realms/ferristrace` and logs
    `federated sign in accepted`. A browser running on the host, outside
    any container, cannot resolve `ferriskey` on its own.
  - `localhost:host-gateway` does not work at all: inside the container,
    `localhost` still names the container itself.
  - `host.docker.internal` resolves inside a container but is absent from
    the Windows host's own hosts file, so a browser on the host cannot use
    it either.
No single name among these resolves on both sides unaided.

## Decision
The shared name is `ferriskey`. `deploy/docker-compose.yml` gives the
`ferriskey-api` service the network alias `ferriskey`, so the server, the
MCP gateway and `ferriskey-init` reach FerrisKey under that name on port
3333 and discover its issuer as `http://ferriskey:3333`. The operator adds one line to their own hosts file —
`127.0.0.1 ferriskey` — so the browser resolves the same name to the same
published port. Both sides now agree on one `iss`, and the product itself
carries no proxy, gateway or extra service to make that agreement happen.

The reasoning that settles it: this is a laptop problem and nothing else.
In production FerrisKey answers on a real, DNS-resolvable name that the
browser and the server both reach the same way, so the name-resolution
difficulty above is absent there — nothing in a production deployment
needs `extra_hosts` or a hosts file edit. A gateway service added to
`deploy/docker-compose.yml` to solve this would sit in the product's own
compose file for the life of the product, maintained, upgraded and
debugged, to work around a limitation of one development environment that
production never has.

## Alternatives rejected
- A reverse proxy service fronting both FerrisTrace and FerrisKey on one
  origin. It would make the local-only problem disappear, but it adds
  permanent complexity to the shipped stack and one more hop to debug
  every time sign-in fails, in exchange for avoiding a single line in a
  file.
- Teach FerrisTrace to discover FerrisKey over an internal url while
  serving the browser a different, public-facing one. FerrisKey has no
  public base url setting to make that split honest on its side, so
  faking it in FerrisTrace would mean no longer validating the token's
  `iss` against what the adapter actually discovered — which is the
  check that makes the flow safe in the first place.

## Consequences
- Gains: one `docker compose up` and one hosts file line give a working
  sign-in on a laptop, with the exact `iss` validation production relies
  on exercised in development too.
- Costs: a new contributor who skips the one documented hosts file line
  gets a sign-in failure whose cause — a name mismatch between what the
  browser resolved and what the server discovered — is not visible from
  the browser's error screen.
- What becomes harder: nothing in production, where the shared name is
  already real. Locally, anyone proxying or renaming the FerrisKey
  service in `docker-compose.yml` has to remember to keep the hosts file
  entry and the network alias naming the same host.

## Amendment
The first version gave `server` `extra_hosts: ["ferriskey:host-gateway"]`
to reach the host's published port. FerrisKey publishes 3333 on the host's
loopback only, so its default admin never faces a network, and on Linux a
container reaching the host through the bridge cannot connect to a port
bound to the host's loopback: the server failed discovery on a clean Linux
machine while it worked on macOS. A network alias on `ferriskey-api` gives
the containers the same name on the same port without going through the
host, and keeps the published port on the loopback.
