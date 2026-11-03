# Cutting a release

What the number means is [ADR 0018](adr/0018-a-version-is-cut-from-a-tag-that-matches-the-manifest.md). What a tag publishes is [ADR 0014](adr/0014-a-release-is-two-images-on-a-tag.md). This page is the runbook.

## Before

1. `develop` is green. Every job, not most of them.
2. Decide the number. Pre-1.0: a breaking change bumps the minor, everything else the patch. ADR 0018 lists what counts as breaking, and the list is about config keys, CLI flags, API fields, OTLP endpoints and migrations, not about how much code moved.
3. Read what the changelog will say. Every pull request prints it in its job summary, under *What the next release would say*. A release whose notes read badly is usually a commit subject that reads badly.

## Cut it

The version bump is its own pull request, because the release refuses to publish a tag that disagrees with the manifest or with the chart.

```sh
# 1. bump workspace.package.version in Cargo.toml, and version and
#    appVersion in charts/ferristrace/Chart.yaml to the same number
git switch -c chore/release-0.2.0
git commit -s -am "chore(release): 0.2.0"
gh pr create --base develop --fill
```

Merge it once CI is green, then move `main` and tag it:

```sh
git switch main
git merge --ff-only develop
git push
git tag -a v0.2.0 -m "v0.2.0"
git push origin v0.2.0
```

The tag is annotated, which is what `git-cliff --latest` reads to find the range.

## What happens then

The `release` workflow runs four jobs in order:

1. `check` — the tag matches the manifest and the chart, then fmt, clippy, the tests and the doc tests. Nothing is published before this passes.
2. `images` — builds and pushes `X.Y.Z` and `X.Y.Z-ee` for linux/amd64 and linux/arm64, with provenance and an SBOM, and attests them. A stable community release also moves `latest`.
3. `chart` — lints the chart and pushes it to `oci://ghcr.io/ferrislabs/charts/ferristrace` at the tag's version, with the same appVersion. It runs after the images, so a published chart never names an image that is not there.
4. `release` — generates the changelog from the commits since the previous tag, measures the published images, and creates the GitHub Release.

## After

```sh
docker pull ghcr.io/ferrislabs/ferristrace:0.2.0
docker run --rm ghcr.io/ferrislabs/ferristrace:0.2.0 --help
helm show chart oci://ghcr.io/ferrislabs/charts/ferristrace --version 0.2.0
```

Then read the release body: the image table carries a line per image and per architecture, and the changelog underneath should name every change an operator would notice. Check that `latest` moved if this was a stable release, and that it did not if this was a candidate.

## A release candidate

Same steps, with the manifest at `0.2.0` and the tag at `v0.2.0-rc.1`. The workflow strips the `-rc.N` before comparing it to the manifest, publishes `0.2.0-rc.1`, `0.2.0-rc.1-ee` and the chart at `0.2.0-rc.1`, marks the release as a prerelease, and moves nothing. Cut one when the risk is in a migration or in the image rather than in the code, because that is the part CI cannot prove before a tag exists.

## When it goes wrong

An image tag that is already on ghcr is public and should be treated as permanent: cut the next patch rather than retag. Deleting a GitHub Release is only right when the release is wrong about itself, and the tag stays either way.

If `check` fails on the tag, nothing was published. Delete the tag, fix the tree, tag again:

```sh
git push origin :refs/tags/v0.2.0
git tag -d v0.2.0
```
