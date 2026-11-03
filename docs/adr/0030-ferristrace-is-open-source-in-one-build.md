# 0030. FerrisTrace is open source, in one build

Status: accepted
Date: 2026-11-03

## Context

[ADR 0014](0014-a-release-is-two-images-on-a-tag.md) published two server
images on every tag: `X.Y.Z`, and `X.Y.Z-ee` built with an `ee` feature that
pulled in a `ferristrace-ee` crate meant for code reserved to the hosted
service. The crate never held a line of code, so the two images did the same
thing, while the second one doubled the release builds, carried an `edition`
value in the chart and a label in the contribution guide.

The first release showed the cost: the publish job's artifact pattern for
`server` also matched `server-ee`, and `ferristrace:0.0.1` came out as one
index holding both builds.

## Decision

FerrisTrace is open source, all of it. There is one server build, and the
hosted service runs the same image as anyone who self-hosts. The `ee` crate,
the `ee` feature, the `-ee` images and the chart's `edition` value are gone.

A release publishes `ferristrace`, `ferristrace-mcp` and
`ferristrace-mcp-gateway` at `X.Y.Z` for linux/amd64 and linux/arm64, and the
chart at the same version.

## Consequences

Everything in ADR 0014 except the second image still holds. Nothing in the
code is gated on an edition, so a feature the hosted service needs is written
here, in the open, and ships to everyone.
