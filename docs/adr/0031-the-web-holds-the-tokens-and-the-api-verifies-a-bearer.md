# 0031. The web holds the tokens and the API verifies a bearer

Status: accepted
Date: 2026-11-04
Supersedes: 0021

## Context
The server was FerrisKey's confidential client: it ran the code flow,
stored an opaque session token hashed in Postgres and set it as a cookie
(ADR 0021 named sessions by a public id). Each deployment needed the
client secret in a Secret, and FerrisKey's Terraform provider stores that
secret masked, so the first production sign-in failed with
`invalid_client`. Mestier and Autharie, from the same owner, already let
the web app be a public client and give the API a bearer to verify; an
operator who knows one knows the others.

## Decision
The web app is a public OIDC client (code flow, PKCE S256) and keeps the
tokens. The API is a resource server: every call carries
`Authorization: Bearer <access token>`, verified against the issuer's
JWKS by the verifier agents already use. A token is a person's when its
`azp` is the configured web client and it has no `client_id` claim. It
maps to an account by (issuer, subject). The active organisation is a
column of the account. The server keeps no session, no pending flow and
no client secret.

## Consequences
- No cookie: no CSRF guard, no `x-requested-with`, no `cookie_secure`.
- FerrisKey owns sessions; FerrisTrace no longer lists or ends them.
- Revoking access takes effect when the access token expires (five
  minutes in FerrisKey's default), not at once.
- Anything the browser loads without JavaScript (an `<img>`, a download
  link) must be fetched with the bearer instead.
- The chart and the compose stack lose the client secret.
