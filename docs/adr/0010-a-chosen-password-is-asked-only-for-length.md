# 0010 — A chosen password is asked only for length

- Status: superseded by 0028
- Date: 2026-10-24

## Context

`Password::new` validated nothing and `/auth/reset/confirm` accepted
whatever it was given, so a single space was a valid password. Nothing
anywhere stated a rule.

A password also reaches argon2, which costs 19 MiB and two passes per
attempt. A caller sending a ten megabyte password makes us do work
proportional to it, and the hash it produces is no better than one from
twenty characters.

## Decision

`PasswordPolicy::admits` is the only rule, and it asks for length alone:
at least 12 characters, at most 128 bytes, and not only spaces.

No composition rules — no required digit, no required symbol, no mixed
case. NIST SP 800-63B withdrew that advice: composition rules push people
towards `Password1!` and towards reuse, and they rule out the long
passphrases that are actually strong. A test states this as behaviour, so
the next person to reach for a regex finds it already answered.

**The floor counts characters, the ceiling counts bytes.** They measure
two different things. The floor is about how much the person chose, and
four glyphs are four choices whether they encode to four bytes or sixteen.
The ceiling is about what we pay to hash, which is bytes. Counting bytes
for the floor would let four emoji pass a rule that says twelve
characters; counting characters for the ceiling would leave the cost
unbounded. The byte check runs first, so a huge password is refused
without walking it.

**The policy is asked before the token is taken.** `confirm` spends the
reset token by deleting it, so checking afterwards would burn somebody's
only link because they typed something short — and they would have to ask
for another one to try again. Order is the whole behaviour here, and a
test holds it.

**Signing in does not apply the policy.** It only applies to choosing.
Accounts provisioned before this rule may hold shorter passwords, and
refusing them at the door would lock out people who did nothing wrong and
would tell an attacker which stored passwords are short. A password that
no longer meets the policy still opens its account; it is replaced the
next time its owner chooses one.

## Consequences

`/auth/reset/confirm` answers 422 with the rule that refused the attempt,
so a front end can show it, and 400 stays what it was: a link that is
spent or was never issued. The two cases no longer look alike.

The policy is a value on `ResetPassword`, set with `with_policy`, so a
deployment that wants a longer floor changes one construction and every
path that chooses a password follows. Nothing reads it from the
configuration file yet, because nothing asked.

Twelve characters is a floor, not a measure of strength. A real measure
means checking the choice against known breached passwords, which needs a
dataset we do not ship. That is the next thing this would gain from, and
it is not in this decision.
