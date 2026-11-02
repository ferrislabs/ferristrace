# 0020. A totp secret is sealed at rest

Status: superseded by 0028
Date: 2026-10-26

## Context
A totp secret is not a credential the server checks, it is a key the server
shares with an app. Verification recomputes the code from it, so it cannot be
hashed the way ADR 0006 hashes a password. It has to be readable by the
process that verifies, which leaves only one question: readable to whom else.

Everything else the control plane keeps is either a digest (`password_hash`,
`password_resets.digest`, `ingest_keys.digest`) or not a secret at all. A
clear totp secret would be the first row in Postgres that is worth stealing on
its own: a backup, a replica, a `pg_dump` in a support ticket or a managed
snapshot hands whoever holds it the ability to mint valid codes for every
enrolled account, for ever, silently. The password hashes in the same dump
survive argon2; the second factor would not survive at all, which inverts the
point of having one.

`chacha20poly1305` has been declared in `crates/storage/Cargo.toml` since the
crate was written and is called by nothing.

## Decision
The adapter seals a totp secret before it reaches a row and unseals it after
it leaves one. The domain never knows:

```rust
pub struct SecretSeal { /* chacha20poly1305, key from config */ }

impl SecretSeal {
    pub fn seal(&self, secret: &TotpSecret) -> Vec<u8>;
    pub fn unseal(&self, sealed: &[u8]) -> Option<TotpSecret>;
}
```

`auth_factors.secret` holds a 12-byte nonce followed by the ciphertext and its
tag. The nonce is fresh per write, so two accounts enrolling the same bytes
produce different rows, and the aead tag means a row edited in the database
fails to unseal rather than verifying against something else.

The key is 32 bytes, base64 in configuration as `secrets.at_rest_key`, and the
server refuses to offer enrolment without one rather than falling back to
clear. `TotpSecret` stays a zeroizing newtype with no `Debug`, so the clear
bytes exist only between the seal and the comparison.

Sealing lives in `crates/storage`, not in the domain, because the domain has
no rule about it: `TotpSecret` crosses the port in clear and
`PostgresFactors` is the only thing that ever sees a byte at rest. A
`SecretSealer` port in `crates/core` would have been a trait with one
implementation, hiding nothing the adapter does not already own.

## Alternatives rejected
- Clear `bytea`, with the risk written in this ADR. It was the first proposal.
  It costs nothing to deploy and makes a stolen dump equal to a stolen second
  factor for every enrolled account, which is the one thing the feature exists
  to prevent.
- Postgres `pgcrypto` and a key passed per statement: the key then lands in
  the query log and in `pg_stat_statements`, and the database it protects is
  the thing holding it.
- A key derived from the account id: derivation from a public value is not a
  key, it is an encoding.
- Envelope encryption with a key manager (KMS, Vault): correct for the cloud
  build, wrong as a requirement for a self-hosted stack that must come up with
  one compose file. `SecretSeal` takes 32 bytes, so a managed key can feed it
  later without touching the schema.

## Consequences
- Gains: a Postgres dump no longer carries anyone's second factor. The dead
  dependency becomes load-bearing.
- Costs: one required configuration value, and a lost key costs every enrolled
  account a re-enrolment. `deploy/README.md` says how to mint one and why to
  keep it beside the database credentials, and the server logs the reason it
  refuses enrolment when the key is absent.
- What becomes harder: rotating the key is a read-decrypt-encrypt-write pass
  over `auth_factors`, which nothing automates yet. With no rotation tool, the
  honest answer for a suspected key compromise today is to forget every factor
  and ask for re-enrolment.
- What becomes easier: nothing else in the schema has to change to keep
  another readable secret. The seal takes bytes and gives bytes.
