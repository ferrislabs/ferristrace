# Second factor

Goal
  An account can keep a second factor, and a password alone no longer opens a
  session for an account that keeps one.

Context
  `SignIn::begin` resolves an identity and issues a session in one step
  (`crates/core/src/signin.rs`). Nothing stands between a correct password and
  a session cookie, and the settings screen `web/src/pages/settings/Security.tsx`
  offers two factors and ten recovery codes that exist only in React state.

  The nearest prior art is the password reset (`crates/core/src/reset.rs`,
  `crates/storage/src/resets.rs`): a one-shot secret minted by the route,
  stored as a sha256 digest, spent by a `DELETE ... RETURNING`, with a window
  the adapter owns. The second factor follows that shape wherever it can.

  Decisions recorded in ADR 0019 (one row per method, verified behind a
  challenge) and ADR 0020 (a totp secret is sealed at rest).

Glossary
  factor         a way to prove the account beyond its password
  authenticator  the factor an app on a phone holds, RFC 6238
  challenge      the short-lived grant that stands between a correct password
                 and a session, spent by one answer
  recovery code  a one-shot secret that answers a challenge when no factor can

Invariants
  - A correct password for an account holding a confirmed factor yields a
    challenge, never a session.                                        [type]
  - A factor is offered at sign-in only once confirmed by a code its owner
    read from it.                                                      [test]
  - A challenge is spent once, within five minutes, after at most five
    answers.                                                           [test]
  - A recovery code is spent once.                                     [test]
  - A totp secret never leaves the process in clear, never reaches a log, and
    never reaches the database in clear.                          [type+test]
  - Recovery codes exist exactly while a factor does: minted with the first,
    dropped with the last.                                             [test]
  - A code is admitted for the current step and the one before it, and never
    for a step already answered.                                       [test]

Out of scope
  - The email factor. PR2: `FactorKind::EmailCode`, a digest beside the
    challenge, the existing SMTP relay. The `kind` column is text so it costs
    no migration.
  - Federated sign-in. An identity provider owns its own second step, and
    `FederatedSignIn` keeps issuing sessions directly.
  - Organisation-wide enforcement. An admin cannot require a factor yet; the
    column it would need belongs to `orgs`, not here.
  - A command-line way out of a lockout (`ferristrace clear-second-factor`).
    Recovery codes cover the owner; an operator still has SQL. PR3, behind
    `ControlPlane` as ADR 0017 requires.
  - Wiring `Security.tsx` and `SignIn.tsx` to these routes. The front end
    keeps its mock state until the routes land.
  - Asking for the password again before forgetting a factor. A live session
    is already the account; the dialog on the screen is the only guard today.
    PR2, beside the email factor, since both need `AuthProvider` in the
    settings use case.

Design
  Domain
    `crates/core/src/totp.rs`
      `TotpSecret`      20 bytes, zeroizing, no `Debug`, base32 for the uri
      `TotpCode`        six digits, `TryFrom<&str>`
      `Step`            the 30-second step a moment falls in
    `crates/core/src/second_factor.rs`
      `FactorKind`      `Authenticator` today, a second variant in PR2
      `RecoveryCode`    two groups of five from 31 unambiguous symbols,
                        zeroizing, sha256 at rest, read however it is typed
      `ChallengeToken`  `ft_2f_` + 64 hex, zeroizing, sha256 at rest
      `Standing`        what the settings screen reads: live kinds, codes left
      `Enrolment`       what the enrolment screen shows: secret, otpauth uri
      `Answer`          `Code(TotpCode)` or `Recovery(RecoveryCode)`
      `SecondFactors`   the settings use case: enrol, confirm, drop, regenerate
      `SecondStep`      the sign-in use case: verify one answer, issue a session
    `crates/core/src/signin.rs`
      `Opened`          `Session(SessionToken)` or
                        `SecondStep { challenge, offered }`

  Ports
    `Gate` (second_factor.rs, static dispatch)
      what `SignIn` needs and nothing more: `offered`, `open`.
    `FactorStore` (second_factor.rs, static dispatch)
      the settings and verify side: `live`, `keep`, `kept`, `confirm`,
      `forget`, `spent_step`, `keep_recovery`, `spend_recovery`,
      `recovery_left`, `answered`, `close`.
    Two ports, one adapter. `SignIn` gains one generic rather than ten methods
    it would never call.

  Adapters
    `crates/storage/src/seal.rs`    `SecretSeal`, chacha20poly1305, the server
                                    key from config, nonce prepended
    `crates/storage/src/factors.rs` `PostgresFactors<C>` implements both ports
                                    over `auth_factors`, `recovery_codes`,
                                    `second_step_challenges`

  Errors
    `FactorError` in the domain: `NoSession`, `NoChallenge`, `Refused`,
    `Exhausted`, `NoFactor`, `AlreadyKept`, `NotOffered`, `StoreUnreachable`.
    `SecondFactorRefused` at the http boundary, mapping to
    400/401/409/429/501/503.
    `SignInError` gains `Factor(FactorError)`, because the gate is reached
    while admitting. The challenge itself is an outcome, not an error.

  Routes (`crates/api/src/second_factor_routes.rs`)
    `GET    /auth/2fa`             standing for the session
    `POST   /auth/2fa/app`         begin enrolment, answers secret and uri
    `POST   /auth/2fa/app/confirm` confirm with a code, answers the recovery
                                   codes when it is the first factor
    `DELETE /auth/2fa/app`         forget the factor
    `POST   /auth/2fa/recovery`    mint ten fresh codes, drop the old
    `POST   /auth/2fa/verify`      spend the challenge cookie, open a session
    `POST /auth/sign-in` keeps answering 204 with a session cookie when no
    factor is live, and answers 200 `{"second_step":["app"]}` with a
    `ft_second_step` cookie when one is.

Acceptance
  1. A password for an account with no factor still answers 204 and a session
     cookie.
  2. A password for an account with a confirmed factor answers 200, names the
     offered kinds, sets a challenge cookie and no session cookie.
  3. The code the authenticator shows for the current step spends the
     challenge and opens a session.
  4. The code for the step before the current one is admitted; two steps back
     is refused.
  5. A code already spent on a challenge is refused on the next one.
  6. A sixth answer on one challenge is refused whatever it carries, and the
     challenge is closed.
  7. A challenge older than five minutes is refused.
  8. Enrolment stores an unconfirmed factor, which is not offered at sign-in.
  9. Confirming the first factor answers ten recovery codes, once.
 10. A recovery code spends a challenge, and the same code never spends a
     second one.
 11. Regenerating replaces every unspent code with ten fresh ones.
 12. Forgetting the last factor leaves no recovery code behind.
 13. A sealed secret round-trips through the adapter, and the row holds
     neither the twenty bytes nor the base32 text.
 14. `format!("{:?}")` on a secret, a code, a recovery code or a challenge
     renders no part of it.
 15. RFC 6238 test vectors pass for sha1 over the published steps.

Simulation
  No. The flow has no concurrency beyond one row per answer, and the clock is
  already a port: `FixedClock` carries every time-dependent case.

Workstreams
  Solo and sequential, as asked. Order: `totp.rs`, `second_factor.rs`,
  `signin.rs`, migration, `seal.rs`, `factors.rs`, routes, wiring, docs.

Frozen contracts
  ```rust
  pub enum Opened {
      Session(SessionToken),
      SecondStep { challenge: ChallengeToken, offered: Vec<FactorKind> },
  }

  pub trait Gate {
      fn offered(
          &self,
          account: AccountId,
      ) -> impl Future<Output = Result<Vec<FactorKind>, FactorError>> + Send;

      fn open(
          &self,
          account: AccountId,
          digest: SecretHash,
      ) -> impl Future<Output = Result<(), FactorError>> + Send;
  }

  pub enum Answer {
      Code(TotpCode),
      Recovery(RecoveryCode),
  }
  ```

Risks / open questions
  - A lost `secrets.at_rest_key` costs every enrolled account a re-enrolment.
    Named in ADR 0020; the deploy README says it before it can be lost.
  - `chacha20poly1305` sits in `crates/storage/Cargo.toml` used by nothing
    today. This spec is its first caller.
  - An email factor sharing the inbox that receives reset links is a weaker
    factor than an app. PR2 says so on the screen rather than pretending
    otherwise.

Verify (exit)
  cargo fmt --all
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  DATABASE_URL=... REQUIRE_DATABASE_URL=1 cargo test -p ferristrace-storage

Verify (loop)
  cargo test -p ferristrace-core totp::
  cargo test -p ferristrace-core second_factor::

Amended: 2026-10-26 recovery codes are typed by a human, so they are two
groups of five unambiguous symbols rather than a prefixed hex token; the gate
is reached inside `SignIn::begin`, so `SignInError` carries `FactorError`; a
deployment with no key at rest needs `FactorError::NotOffered`; forgetting a
factor without re-proving the password is named out of scope rather than left
unsaid.
