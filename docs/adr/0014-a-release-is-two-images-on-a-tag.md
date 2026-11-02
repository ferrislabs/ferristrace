# 0014 — A release is two images on a tag

- Status: accepted
- Date: 2026-10-24

## Context

The compose file built from a checkout, so self-hosting needed the source
and a Rust toolchain. A published image removes both.

The runtime was `debian:bookworm-slim` with `curl` installed, only so the
compose healthcheck could reach `/health`. That is a package manager, a
shell and an HTTP client in the image, for one GET.

## Decision

A tag matching `vX.Y.Z` or `vX.Y.Z-rc.N` publishes two images to
`ghcr.io/ferrislabs/ferristrace`: `X.Y.Z` without the `ee` feature and
`X.Y.Z-ee` with it. A stable community release also moves `latest`; a
release candidate moves nothing.

The runtime is `gcr.io/distroless/cc-debian12:nonroot`. No shell, no
package manager, no curl, and uid 65532.

`ferristrace health` takes the place of curl: it asks `/health` on the
configured port and exits non-zero if the answer is not a success. The
compose healthcheck is `["CMD", "ferristrace", "health"]`, the exec form,
because a distroless image has no `/bin/sh` for the shell form to use. It
is intercepted before the control plane pool is opened, so a healthcheck
every five seconds costs no database connection.

Both architectures are **cross-compiled from the runner's own
architecture** rather than built under QEMU. The build stage is pinned to
`$BUILDPLATFORM` and picks the target and the cross linker from
`$TARGETARCH`. Emulating aarch64 to compile Rust with `lto = "thin"` and
`codegen-units = 1` would turn a ten minute release into most of an hour.

## Consequences

Release notes come from GitHub's own generated notes, fetched through the
API and appended under an Images section that carries the pull command and
a size table. A curated changelog driven by conventional commits is #68
and stays there.

Image size is reported in two places: the job summary of every pull
request, which is where a regression gets noticed, and the release body,
per image and per architecture, as the compressed sum of layer sizes.

**The release workflow itself is not exercised by this pull request**,
because it only runs on a tag. What CI does prove is the part most likely
to be wrong: a job builds the image for linux/arm64 and, separately, for
the `ee` feature, so the cross-compile and the feature flag are checked on
every change. What stays unproven until the first tag is the push to
ghcr.io, the provenance attestation, and the release body.

The `cc` distroless variant is needed rather than `static` because the
binary links glibc. Going static would mean `musl` and a different libc
for argon2 and ring to be measured against, which is not a change to make
blind.
