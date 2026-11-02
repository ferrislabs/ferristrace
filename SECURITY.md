# Security policy

## Reporting a vulnerability

Do not open a public issue. Report it privately:

**[Open a private report](https://github.com/ferrislabs/ferristrace/security/advisories/new)**

That creates a draft advisory visible only to you and the maintainers, with a
private thread to work in and, once there is a fix, a CVE and a published
advisory. If GitHub is not an option for you, email
thibault.pointurier.pro@gmail.com instead.

A report is easiest to act on when it carries:

- the version or commit, and whether it is the community or the `-ee` image
- what an attacker can reach, and what they get out of it
- the smallest sequence of steps that shows it
- anything that would narrow the blast radius in the meantime

A proof of concept helps. Please do not include real data belonging to anyone
else.

## What to expect

| Stage | Target |
| --- | --- |
| Acknowledgement that the report arrived | 3 working days |
| First assessment: is it real, how bad, who is affected | 10 working days |
| Fix released | critical within 14 days, otherwise with the next release |
| Advisory published | once a fix is out, or 90 days after the report |

Those are targets, not guarantees. FerrisTrace is maintained by one person, and
a report that lands mid-holiday will take the long end of each window. You
will hear why rather than hear nothing.

If you plan to disclose publicly before the fix, say so in the report and we
will agree a date rather than discover it.

## Supported versions

FerrisTrace is pre-1.0. Only the most recent release carries fixes, and there
are no backports to earlier ones.

| Version | Supported |
| --- | --- |
| The latest `0.x` release | Yes |
| Anything earlier | No, upgrade |
| `main` and `develop` | Yes, report what you find there |

## Scope

In scope, for both the community and the `-ee` builds:

- the server: the OTLP endpoints on 4317 and 4318, the HTTP API, the web
  application it serves
- authentication and authorisation: sessions, OIDC flows and the adoption of
  an account by a verified address, ingest keys, and the isolation of one organisation's traces from
  another's
- the published images on `ghcr.io/ferrislabs/ferristrace`
- the self-hosted stack in `deploy/`, run as documented

Out of scope:

- the default `ferristrace` passwords in the compose stack. They exist so
  `docker compose up` works on a laptop, and `deploy/README.md` says to change
  them before the stack faces anything real
- anything that needs an already-compromised host, an already-valid session, or
  an ingest key the attacker was given
- a dependency's vulnerability with no path to it through FerrisTrace. Report
  that upstream; tell us if we expose it
- scanner output with no demonstrated impact, missing hardening headers on
  their own, and reports about software versions
- social engineering, physical access, and anything aimed at a person rather
  than at the code

## Testing

Test against a deployment you own. Do not test against someone else's
FerrisTrace, do not run load or denial-of-service tests against a shared host,
and do not read, keep or publish data that is not yours. Research done that
way, reported privately, is welcome and will not be met with a lawyer.
