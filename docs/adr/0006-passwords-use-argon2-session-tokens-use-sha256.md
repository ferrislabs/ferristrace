# 0006. A password is hashed with argon2id, a session token with SHA-256

Status: accepted, narrowed by 0028
Date: 2026-10-24

Narrowed by [0028](0028-local-passwords-and-the-second-factor-leave-the-product.md): the argon2id half is superseded because no password is stored any more. The session half stands: a session token is stored as its SHA-256 and compared in constant time.

## Context
Two secrets reach the control plane and both are stored as digests, but they are not the same kind of secret. A password is chosen by a person, so its entropy is low and guessable: an attacker holding the digests will try a dictionary. A session token is thirty-two bytes this service drew at random, so there is no dictionary to try and no smaller space to search than the whole of it. Using one hashing scheme for both would either leave passwords cheap to attack or make every authenticated request pay a key derivation.

## Decision
A password is hashed with argon2id at the OWASP cost of 19 MiB, two passes and one lane, on a blocking thread, and verified against the stored PHC string. A session token is stored as its SHA-256 and compared in constant time. When a stored password hash carries a cost below the current one, a successful sign-in rewrites it at the current cost.

## Alternatives rejected
- Argon2 for session tokens as well: every authenticated request would spend 19 MiB and tens of milliseconds to re-derive a digest of a value that was already random, and the cost would fall on the request path rather than on the attacker.
- SHA-256 for passwords: a stolen table becomes an offline dictionary attack at the speed of the hash, which is the attack argon2 exists to make expensive.
- One scheme chosen by configuration: the two secrets have different threat models, so a single switch would be wrong in one of its positions.
- The clear prefix and argon2 pairing that ADR 0003 gives API keys: a session needs no prefix lookup, because its own digest is the primary key and the token is never user-chosen.

## Consequences
- Gains: a password costs an attacker 19 MiB per guess; a session costs this service one indexed digest read. A sign-in upgrades a hash left behind by an older cost, so raising the cost does not need a migration.
- Costs: two hashing paths to understand, and a sign-in that blocks a thread for tens of milliseconds, which is why it runs through spawn_blocking rather than on the runtime.
- What becomes harder: comparing a password in constant time inside the domain. Argon2 is an adapter here, so the layering keeps the cost parameters out of the core and the core cannot verify a password on its own.
