# Lot 6: FerrisKey is the only way in

Goal
  A person reaches FerrisTrace through FerrisKey and nothing else. No
  password is stored, chosen, mailed or checked anywhere in the product,
  and the self-hosted stack still comes up usable in one command.

Context
  Two ways in stand side by side today. Local passwords are the default:
  `local_password` is `true` (apps/server/src/config.rs:42), argon2 hashes
  them (crates/infra/src/hasher.rs), `/auth/sign-in` takes an address and
  a password (crates/api/src/sign_in.rs:36), `/auth/reset/*` mails a
  single-use token (crates/api/src/reset_routes.rs:35), and a TOTP second
  factor guards the result (crates/api/src/second_factor_routes.rs:55).
  OIDC stands beside it: discovery, PKCE and id token validation are
  complete (crates/infra/src/oidc.rs), `/auth/oidc/start` and
  `/auth/oidc/callback` are mounted when the block is set
  (apps/server/src/routes.rs:79), and the sign-in screen already renders
  a working provider button on its own (web/src/pages/auth/SignIn.tsx:83).
  An OIDC-only deployment is already documented
  (deploy/README.md:252-268).

  So the way in exists. What this lot removes is the alternative, the
  code that only the alternative needs, and the trap that the two of them
  leave behind: `ProvisionPolicy::default()` is hardcoded with
  `link_by_email: false` (apps/server/src/main.rs:249), so running
  `bootstrap` before the first federated sign-in — the order the quick
  start teaches (README.md:14) — produces a second account that owns
  nothing.

  Agents already trust FerrisKey (lot 5): `JwksVerifier`
  (crates/infra/src/jwks.rs:272) verifies an access token against the
  issuer's JWKS. This lot does not touch that path.

  One pull request per part:
    6A  FerrisKey is the only way in        config, provisioning, stack
    6B  The password and the second factor  domain to web
        leave the code
    6C  The schema drops what nobody writes migration, ADRs, docs

  6B and the second factor were two parts until implementation showed
  they are one. Only `sign_in.rs:56` ever consults the `Gate`:
  `federated_sign_in.rs` names neither `Gate` nor `offered`, so the
  second factor is enforced by the password flow and by nothing else. It
  becomes unreachable the moment that flow goes, which makes its removal
  a consequence of 6B rather than a part beside it. Shipping them apart
  would leave a release whose settings screen invites somebody to enrol
  a factor nothing will ever ask for — which is, incidentally, what an
  OIDC-only deployment of `main` does today, hidden only by the front
  end gating the card on `local_password`.

Glossary
  Way in       the one flow that turns a person into a session: a
               FerrisKey authorization code exchanged at
               `/auth/oidc/callback`.
  Linked       an account an `identity_links` row ties to an issuer and
               a subject.
  Adopted      an account `bootstrap` created from an address, that a
               first sign-in links to its FerrisKey subject.
  Seal         the chacha20poly1305 key under
               `FERRISTRACE_SECRETS__AT_REST_KEY`. It stays: webhook
               secrets are sealed with it (crates/postgres/src/alerts.rs:346).

Invariants
  - No route, port, adapter, column or command accepts, stores, hashes,
    mails or checks a password. `PasswordHasher`, `Argon2Hasher`,
    `Issuer::Local` and the argon2 dependency are gone.  [test]
  - A person signs in only by exchanging a FerrisKey authorization code.
    `/auth/sign-in`, `/auth/password`, `/auth/reset/*` and `/auth/2fa/*`
    do not exist, at any configuration.  [test]
  - A FerrisKey identity whose subject is already linked resolves to that
    account and to no other.  [test]
  - An unlinked identity is adopted by the account holding the same
    address only when the token says that address is verified.  [test]
  - An unlinked identity carrying an address another account already
    holds, unverified, is refused. It gets no account at all: `accounts.
    email` is `NOT NULL UNIQUE` (20261024000001_control_plane.up.sql:25),
    so there is no second account to give it, and dropping that
    constraint would make every address-keyed command ambiguous —
    `bootstrap --email`, `grant-role --member`, `standing_of`. The
    refusal is `AccountError::Taken`, the same one `create` already
    raises on that constraint, never a raw database error. The person
    verifies their address in FerrisKey, or an administrator links
    them.  [test]
  - That refusal reaches the caller as a refusal, not as an outage: it
    answers 401, and it is logged at `warn` with the address. A
    deterministic refusal answering 503 from the `ProviderUnreachable`
    arm, logged at `error`, would be indistinguishable from FerrisKey
    being down and would wake somebody for a working deployment. It
    maps to the existing `AuthError::Rejected` rather than earning a new
    variant, because 6B reshapes `AuthError` when the local arm goes and
    a variant added now would be rewritten before it was ever read.
    [test]
  - A missing `email_verified` is never read as verified. FerrisKey
    sends it on the id token today; an identity arriving without it is
    refused adoption and gets its own account.  [test]
  - `bootstrap --email` works before that person has ever signed in, and
    their first sign-in lands on the account it created, owning the
    organisation it founded.  [test]
  - With no FerrisKey configured the server still starts, still ingests
    telemetry, still answers `/api` to keys and agent tokens; only the
    way in is unavailable, and `/auth/methods` plus the sign-in screen
    say so.  [test]
  - Sessions keep their current behaviour: public id, idle and hard
    expiry, listing, revoking one and revoking the others.  [test]
  - API keys and agent tokens keep their current behaviour.  [test]
  - The seal and `FERRISTRACE_SECRETS__AT_REST_KEY` survive the removal
    of TOTP, and a webhook secret written before this lot still opens
    after it.  [test]

Out of scope
  - Registering the FerrisKey client from FerrisTrace. An administrator
    creates it in FerrisKey and configures the four `OIDC` settings, as
    for agents today.
  - Roles and memberships. `grant-role` and the roster are untouched.
  - Agent tokens, API keys, ingestion, and anything the `access` module
    owns.
  - Carrying an existing deployment's passwords over to FerrisKey. There
    is no import: a person signs in through FerrisKey or not at all.
  - Group or role claims from FerrisKey deciding a FerrisTrace role.

Design
  6A FerrisKey is the only way in
    Config: `local_password` stays untouched here, flag and default
    both. It guards the routes 6B deletes, so removing it first would
    mount `/auth/sign-in` unconditionally and leave an intermediate
    release where local passwords are *more* reachable than before. The
    flag and the code it guards leave together, in 6B. `oidc` stays an
    `Option` throughout, because a server with no way in still ingests
    and still answers keys — the house behaviour for mail, objects and
    agents; its absence keeps logging one warning at startup.
    So 6A adds and fixes only: it never removes.

    Provisioning: `ProvisionPolicy` keeps both flags, and
    apps/server/src/main.rs:249 sets `link_by_email: true`. Linking by
    address is only safe behind a verified address, so
    `crates/postgres/src/accounts.rs:102` gains that condition: the
    lookup runs only when `identity.email_verified`. An unverified
    address provisions a new account rather than adopting one.
    ADR: a FerrisKey identity is adopted only by a verified address.

    Bootstrap: `bootstrap --email` keeps founding the organisation and
    creating the locked account, prints no token, and prints how that
    person signs in. `create-account` and `reset-password` go in 6B.

    Stack: `deploy/docker-compose.yml` gains FerrisKey, its database and
    its migration job — `ghcr.io/ferriskey/ferriskey-api` and
    `ghcr.io/ferriskey/ferriskey-webapp`, pinned, the api on 3333 and the
    console on 5555, admin credentials from `ADMIN_USERNAME` /
    `ADMIN_PASSWORD` / `ADMIN_EMAIL` before first start. The issuer is
    `<base>/realms/<realm>`, discovery at
    `<base>/realms/<realm>/.well-known/openid-configuration`. The client
    is confidential and PKCE S256 is required, which the adapter already
    sends; `openid`, `profile` and `email` must be default scopes on it.
    The four `FERRISTRACE_OIDC__*` settings get defaults pointing at that
    service, overridable in `.env` for a FerrisKey elsewhere.

    Provisioning goes through FerrisKey's admin API, not through
    `ferris-ctl`, which creates clients with the grant off and has no
    flag to change it. An init service holding an admin token — the
    `admin-cli` client takes a password grant on `master` — runs:
      POST   /realms                                    the realm
      POST   /realms/{r}/clients                        confidential,
                                                        require_pkce
      GET    /realms/{r}/clients/{uuid}/client-secret   reads it back
      POST   /realms/{r}/clients/{uuid}/redirects       the callback
    and writes the secret it read into an env file the `server` service
    takes as a second `env_file`. Nothing is copied by a reader, so
    @spec-l6-8's fallback clause does not apply.

    Measured on a standing FerrisKey 0.10.1, so none of this is assumed:
      - A new confidential client already carries `email`, `openid`,
        `profile`, `organization` and `roles` as DEFAULT scopes, and
        every mapper on them is `id.token.claim=true`. Nothing to wire.
      - An id token from that client carries `email` and
        `email_verified`, beside `at_hash`, `azp`, `sid`, `auth_time`
        and `aud` as a string. So the adapter reads both claims exactly
        where it already looks (crates/infra/src/oidc.rs:148) and no
        userinfo call is needed.
      - `/auth/oidc/start` against that realm answers a 303 to the
        authorization endpoint with `code_challenge_method=S256` and
        `scope=openid email`. The adapter does not ask for `profile`;
        the claims arrive anyway because the scope is default on the
        client. 6A asks for it explicitly, so an account gets a name
        from a client whose defaults somebody narrowed.
      - A password must hold an uppercase letter. Only the seeded demo
        user cares; FerrisTrace never chooses one.

    One hostname, and it is the whole difficulty. FerrisKey builds every
    url in its discovery document, `issuer` included, from the request's
    `Host` header: asked as `localhost:3333` it answers
    `"issuer":"http://localhost:3333/realms/ferristrace"`, asked as
    `id.example.com` it answers that instead. There is no public base
    url to configure, so the browser and the server have to use the same
    name or the `iss` the adapter discovered will not match the `iss`
    the token carries. Measured:
      - `extra_hosts: ["ferriskey:host-gateway"]` lets the server reach
        the published port under the name `ferriskey`, and the server
        then discovers `http://ferriskey:3333/realms/ferristrace`
        successfully — the log reads `federated sign in accepted`.
      - `localhost:host-gateway` does not work: `localhost` stays the
        container inside the container.
      - `host.docker.internal` resolves inside a container but not on
        the Windows host, so it cannot be the shared name either.
    Decided: one documented `127.0.0.1 ferriskey` line in the reader's
    hosts file, and no gateway. A gateway would carry a proxy in the
    product's own compose file for the life of the product to work
    around a name resolution limit that exists on a laptop and nowhere
    else — in production FerrisKey answers on a real name that the
    browser and the server both resolve, so the difficulty is absent
    there. The laptop pays one documented line; the product carries
    nothing. ADR 0027.

  6B The password and the second factor leave the code
    Domain: `Password`, `PasswordPolicy`, `PasswordCheck`, `PasswordHash`,
    `PasswordHasher`, `Issuer::Local`, `SignIn`, `ChangePassword`,
    `ResetPassword`, `ResetStore`, `Mailer::send_password_reset`,
    `AuthSources`' local arm, and `SignInProof`'s local arm.
    `Issuer` becomes a newtype over the issuer url, so
    `accounts.rs:91`'s `is_local()` branch and `established()` go with it.
    Application: the use cases and registry entries that name them.
    Postgres: `local.rs`, `resets.rs`, and `Resets` in the registry.
    Infra: `hasher.rs` and the argon2 dependency.
    Api: `sign_in.rs`, `password_routes.rs`, `reset_routes.rs`.
    Server: the `local_password` branches in `routes.rs:71,87`, the
    `Hasher` adapter, `create-account` and `reset-password` in `cli.rs`.
    Web: `ForgotPassword.tsx`, `ResetPassword.tsx`, the `/forgot-password`
    and `/reset` routes, the password form and the password section of
    `Security.tsx`.
    The mail relay stays: alerts, pages and status pages mail. Only the
    reset link leaves, so the startup warning loses its reset clause.

    The guides move with the code, in this part rather than in 6C.
    `deploy/README.md`'s "Make it usable" tells an operator to spend a
    printed token at `/auth/reset/confirm`, which this part makes answer
    404 — it is the first command a new operator runs, so a release that
    breaks it and leaves the instruction standing is not finished. The
    second factor section, the "signing in with an identity provider
    instead" framing, the `FERRISTRACE_LOCAL_PASSWORD` row and the reset
    half of "Adding somebody" go with it, and the at-rest key's row
    stops explaining itself by a second factor and starts explaining
    itself by webhook secrets. 6C keeps only what the schema change
    itself touches, and the ADR supersessions.

    Inviting a member goes with the reset token, and 6A is what makes
    that safe. `Roster::invite` (crates/domain/src/organisation/roster.rs:92)
    creates a locked account for an address nobody holds and mints a
    `ResetToken` for the invitee to spend at `/auth/reset/confirm`. With
    no password to choose, the token has nothing to buy: the invitation
    creates the account and grants the role, and the invitee's first
    FerrisKey sign-in adopts it by its verified address — exactly the
    path 6A turned on. So `Invitation.way_in`, the `invite_token` field
    of the roster DTO, `Roll::open_way_in`, `ControlPlane::invite` and
    `ProvisioningService::invite` all go. `create_locked` stays, because
    the account has to exist before the person ever signs in so a role
    can be granted to it; what "locked" meant was a password hash of
    random bytes (ADR 0009), and with that column gone in 6C it is
    simply an account nobody has signed in to yet. Rename it if the name
    now lies.

    The second factor, which the password flow alone enforced:
    Domain: `SecondFactors`, `SecondStep`, `Gate`, `FactorStore`,
    `NoSecondFactor`, `FactorKind`, `TotpCode`, `TotpSecret`,
    `RecoveryCode`, `ChallengeToken`, `Pending`, `Opened`'s second-step
    arm, `CHALLENGE_PREFIX`, `RECOVERY_CODES`, `DRIFT_STEPS`,
    `STEP_SECONDS`.
    Postgres: `factors.rs` and its registry entry; `unit_of_work.rs`
    keeps `with_seal`, which alerts need.
    Api: `second_factor_routes.rs` and the challenge arm of the sign-in
    response.
    Web: the TOTP and recovery sections of `Security.tsx`, which keeps
    the session roll.
    FerrisKey owns the second factor from here. `deploy/README.md`'s
    second factor section says so and points at the realm.

  6C The schema drops what nobody writes
    Two things 6B leaves pointing at the column, both found by sweeping
    for it and both deliberate: the test
    `an_account_may_now_exist_before_any_way_in_is_linked`
    (crates/postgres/tests/control_plane.rs) inserts `password_hash =
    NULL` to prove the cross-table CHECK is gone, and goes when the
    column does; and `accounts_carry_a_way_in` needs no dropping,
    because 20261024000003_identity_links.up.sql already dropped it
    three migrations in — the live schema carries only
    `accounts_name_is_not_blank` and `accounts_photo_is_whole`. So the
    accounts half of this part is one `DROP COLUMN` with no constraint
    to remove first, and `20261024000003_identity_links.down.sql` stops
    being able to reinstate a CHECK on a column that is gone: its own
    reversal needs reading alongside.

    One migration, after 6B has stopped writing: drop
    `password_resets`, `auth_factors`, `recovery_codes`,
    `second_step_challenges`, and the `accounts.password_hash` column
    with the constraint that mentions it. Its `.down.sql` recreates them
    empty, as the house convention requires, and cannot recover rows.
    docs/specs/second-factor.md is a frozen record of a lot, like an
    ADR, so it keeps its text and gains one superseded line rather than
    being rewritten or deleted — hexagonal-crates.md still names
    crates/core and crates/storage, renamed lots ago, which is the
    convention stated by practice.

    ADRs: supersede 0009 (an account is created locked by a reset token),
    0010 (a chosen password is asked only for length), 0013 (a local
    identity is its own account), 0019 (a second factor is answered
    behind a challenge) and 0020 (a TOTP secret is sealed at rest).
    Narrow 0006, whose session half stands: session tokens stay SHA-256,
    the argon2 half is superseded. 0021 (a session is named by a public
    id) stands untouched.
    Docs: `deploy/README.md` loses "Make it usable"'s reset token, the
    second factor section and the reset half of "Adding somebody", and
    gains the FerrisKey quick start; `README.md`'s quick start; the
    settings table; `.env.example`; `CONTRIBUTING.md` if it names a
    password.
    Breaking, so the release notes say it and the version takes a minor
    bump (ADR 0018).

Acceptance
  @spec-l6-1  A person signs in through FerrisKey and gets a session;
              `/auth/sign-in`, `/auth/password`, `/auth/reset/request`,
              `/auth/reset/confirm` and every `/auth/2fa/*` answer 404
              with FerrisKey configured and without it.
  @spec-l6-2  A subject already linked resolves to its account; a second
              sign-in with the same subject issues a second session on
              the same account.
  @spec-l6-3  An unlinked identity carrying a verified address adopts the
              account holding that address; carrying an unverified one it
              is refused with `Taken` and the held account keeps its id,
              its address and its rows. An unverified address nobody
              holds still provisions its own account.
  @spec-l6-4  `bootstrap --email a@b` then a first FerrisKey sign-in as
              `a@b` yields one account, owning the organisation, and
              `bootstrap` printed no token.
  @spec-l6-5  With no OIDC block the server starts, logs the warning,
              `/auth/methods` answers `{"oidc":false}`, OTLP still
              ingests and `/api` still answers a read key.
  @spec-l6-6  A webhook secret written before the migration still opens
              after it, and an alert still delivers to it.
  @spec-l6-7  The migration leaves no table or column holding a password
              or a TOTP secret, and applying it twice changes nothing.
  @spec-l6-8  `docker compose up --build --detach` then
              `ferristrace bootstrap --email …` yields a stack whose
              sign-in screen reaches FerrisKey and returns a session,
              with no realm, client or redirect uri created by hand. The
              generated client secret reaches the server without the
              reader copying it; if that proves impossible, the one
              documented command that prints it is the only manual step
              and the guide says so.

Simulation
  No. The flows are request-scoped and the token verification already
  has its harness (crates/infra/tests/jwks.rs serves a JWKS from a test
  server); the federated sign-in tests reuse it.

Verify (exit, per PR)
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-postgres
  CLICKHOUSE_URL=... REQUIRE_CLICKHOUSE_URL=1 cargo test -p ferristrace-clickhouse
  cd web && pnpm typecheck && pnpm build
  grep -rin "password" crates apps web/src --include=*.rs --include=*.ts --include=*.tsx
    answers nothing outside a comment about what no longer exists
