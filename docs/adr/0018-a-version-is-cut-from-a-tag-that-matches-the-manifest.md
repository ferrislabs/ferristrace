# 0018. A version is cut from a tag that matches the manifest

Status: accepted
Date: 2026-10-26

## Context
ADR 0014 settled what a tag publishes: two images, provenance, a size table, and `latest` moved only by a stable community release. It left three things open, and said so.

Nothing stated what the number means. The workspace declares `0.1.0` and `release.yaml` derives every version from the tag, so `v0.9.0` would have published a tree that calls itself `0.1.0` without anything objecting.

Nothing stated which bump carries a breaking change, which matters more here than usual: the thing being versioned is not a library but a server, a command line, two images, a compose file and a schema that an operator has already applied to their data.

Release notes came from `releases/generate-notes`, which builds its list from pull request titles. Merges here are rebases, so the titles are not what the history keeps, and ADR 0014 named a changelog driven by conventional commits as issue #68.

## Decision
**SemVer, read for a pre-1.0 product.** A breaking change bumps the minor; everything else bumps the patch. The major stays at zero until the API, the config keys and the CLI stop moving.

Breaking means any of these:

- a config key (`FERRISTRACE_*`) or a compose variable removed, renamed, or given a different meaning
- a CLI subcommand or flag removed or renamed
- an API route or a response field removed or renamed, or a status code a screen depends on
- an OTLP endpoint, port or authentication scheme changed
- a migration that cannot be applied to the previous release's data, or that an operator cannot roll back

Not breaking: adding a route, a field, a key or a flag with a default; a migration that only adds; anything behind the `ee` feature; the web bundle's internals; the shape of a log line.

**One version for everything shipped** — the server, the CLI, both images, the compose file, the web bundle. `workspace.package.version` is the source of truth and the tag is `v` plus it. The release workflow refuses to publish when the two disagree, so the number is bumped in a reviewed commit before the tag exists rather than invented at tag time.

A release candidate is `vX.Y.Z-rc.N` with the manifest already at `X.Y.Z`.

**Conventional commits are enforced on the pull request title and on every commit subject.** The subjects are what a rebase merge puts in the history, and the history is what the changelog reads; checking only the title would leave the changelog generated from unchecked text.

**The changelog is generated from the commits between tags by `git-cliff`, and it is the release body.** `cliff.toml` groups by type and drops `test`, `chore`, `ci`, `build` and `style`. There is no committed `CHANGELOG.md`.

## Alternatives rejected
- A committed `CHANGELOG.md`. It needs a commit made by CI on a protected branch, without a review and without a sign-off, to record what git already holds. The Releases page is already the artifact ADR 0014 chose.
- GitHub's generated notes. They read pull request titles, which a rebase merge discards, and they cannot group by type.
- Enforcing conventional commits on the title alone, as issue #68 asked. With rebase merges the title lands nowhere; the check stays because it is what the pull request list shows, but it cannot be the only one.
- Letting a tool decide the number from the commits, the way `release-plz` does. Which bump a change deserves is a decision about compatibility, and at 0.x, with a schema an operator owns, it is not one to derive from a commit prefix.

## Consequences
- Gains: a tag cannot publish a tree that disagrees with it. The changelog comes from the history rather than from a view of it. The `cliff.toml` template is rendered on every pull request and printed in the job summary, which is the part of the release path ADR 0014 said nothing exercised.
- Costs: one more third-party action, pinned by commit sha, in a workflow that holds `packages: write`. Cutting a release is two merges rather than one, since the version bump is its own reviewed commit.
- What becomes harder: a quick release. That is the intent — the gate exists because the compose file and the schema travel with the number.
- The runbook lives in `docs/releasing.md` and is the thing to update when a step moves.
