# Lot 8: the web signs in, the API verifies a bearer

Goal
  The web app is a public OIDC client of FerrisKey and holds the tokens.
  The API keeps no session and no client secret: every call carries an
  access token that the API verifies against the issuer's JWKS. Same
  shape as mestier and autharie.

Context
  Today the server is a confidential client (`OidcConfig`,
  apps/server/src/config.rs:208). It runs the code flow
  (crates/api/src/oidc_routes.rs, crates/infra/src/oidc.rs), keeps the
  pending flow in `oidc_flows`, mints an opaque `ft_ss_` token stored
  hashed in `sessions` (crates/postgres/src/sessions.rs) and sets it as a
  cookie (crates/api/src/session_cookie.rs). Every route reads that
  cookie through `carried` (crates/api/src/carried.rs:57), guarded by an
  `x-requested-with` header against cross-site requests. The active
  organisation lives on the session row (`sessions.org_id`).

  A deployment therefore needs the client secret in a Secret; the
  Terraform provider stores it masked (`***`), which broke the first
  production sign-in on 2026-11-04.

  Agents already present FerrisKey access tokens, verified by
  `JwksVerifier` (crates/infra/src/jwks.rs). This lot reuses it for
  people.

Decisions (2026-11-04, owner)
  D1  The active organisation moves from the session to the account
      (`accounts.active_org_id`). The web keeps calling
      `POST /api/workspace/switch`.
  D2  An access token maps to an account by (issuer, subject), through
      the existing `AccountDirectory::provision` and `ProvisionPolicy`.
      Not by email.
  D3  A token is a person's when its `azp` is the configured web client
      and it carries no `client_id` claim. The API checks `iss`, `exp`,
      `nbf`, the signature (RS256, ES256) and `typ` when present. No
      audience is required of people's tokens.

Design
  Domain
    `SessionToken` keeps its name and its place in every use case, but
    it is no longer a secret string: it holds a `ResolvedIdentity` and
    the token expiry, and only a verifier (or a test) builds one.
    `SessionStore` keeps `resolve` alone: the Postgres store provisions
    or finds the account by (issuer, subject) and reads its standing,
    `active_org_id` included. `SessionSwitch` writes
    `accounts.active_org_id`.
    New port `UserTokens::verify(&str) -> Result<SessionToken,
    UserRefusal>`; `UserRefusal` is `Off`, `NotAPerson`, `Invalid`,
    `Unavailable`.
    Removed: `issue`, `revoke`, `SessionRoll`, `Sessions`,
    `LiveSession`, `SessionId`, `RollError`, `SessionLifetime`,
    `FederatedSignIn`, `OidcFlows`, `Authorizer`, `AuthProvider`,
    `SignInProof`, `PendingFlow`, `CodeVerifier`, `FlowError`,
    `FederatedError`, the session user agent.

  Adapters
    `JwksVerifier` gains a people mode (issuer, web client id) and
    implements `UserTokens`. `crates/infra/src/oidc.rs` and the
    `openidconnect` dependency leave. `PostgresFlows` leaves.

  API
    `carried` becomes async: it reads `Authorization: Bearer`, asks the
    application to verify, and answers 401 otherwise. `reading` sends a
    three-part bearer to the people verifier first and to the agent
    verifier on `NotAPerson`; any other bearer stays an API key.
    `x-requested-with` is no longer required: a bearer is never sent by
    the browser on its own.
    Removed routes: `/auth/oidc/start`, `/auth/oidc/callback`,
    `/auth/sign-out`, `/auth/sessions`, `/auth/sessions/{id}`.
    `GET /auth/methods` answers `{oidc, issuer, client_id}` so the web
    reads its runtime config from the server that serves it.

  Schema
    Migration adds `accounts.active_org_id` (FK to `orgs`, on delete set
    null), copies the latest live session's org per account, drops
    `sessions` and `oidc_flows`. The down migration recreates both
    tables empty and drops the column.

  Web
    `oidc-client-ts` and `react-oidc-context`, user in `localStorage`,
    code flow with PKCE S256, `redirect_uri` the app origin. Renewal by
    refresh token, one tab at a time under a Web Lock, 60 s before
    expiry; a failed renewal sends the person to FerrisKey again.
    `src/auth/client.ts` attaches the bearer and drops cookies and
    `x-requested-with`. Avatars and the organisation export are fetched
    with the bearer, not linked. The sessions screen leaves; sign-out is
    `signoutRedirect`.
    There is no sign-in page: a visitor without a session, or whose call
    answers 401, is sent to FerrisKey at once and brought back to the page
    they asked for. Public status pages never redirect. A second redirect
    within 30 s stops on a "sign-in did not finish" screen instead of
    looping; a server without `[oidc]` shows "sign-in is unavailable".
    After a deliberate sign-out the tab shows "you are signed out" with
    a button, instead of sending the person straight back to FerrisKey.

  Configuration
    `[oidc]` keeps `issuer` and `client_id` (now the public web client).
    `client_secret` and `redirect_uri` leave, as do
    `FERRISTRACE_COOKIE_SECURE` and the chart's `secrets.oidcClientSecret`.
    `deploy/ferriskey-init.sh` creates a public client and stops writing
    a secret.

Out of scope
  The FerrisKey realm in ferrislabs/infra (public client, web origin):
  done after this lot merges. MCP gateway and agents: unchanged.

Acceptance
  A1  A call with a valid person's bearer reaches the use case as that
      person's account (api tests, every route group).
  A2  A call with no bearer, a bad signature, another issuer, an expired
      token or another `azp` answers 401.
  A3  A first call from an unknown (issuer, subject) provisions an
      account under `ProvisionPolicy`; a second call reuses it.
  A4  An agent token (other `azp`, agent audience) still reaches the
      agent path; an API key still reaches the key path.
  A5  Switching organisation persists across tokens: a new token for
      the same person lands in the last organisation chosen.
  A6  The removed routes answer 404.
  A7  `/auth/methods` names the issuer and the web client id.
  A8  The web signs in, renews, calls the API with the bearer, shows
      avatars, exports, and signs out (manual check against FerrisKey).

Verify (exit)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  Postgres suite (crates/postgres, DATABASE_URL set)
  pnpm -C web lint && pnpm -C web build
  helm lint charts/ferristrace -f charts/ferristrace/ci/full-values.yaml
