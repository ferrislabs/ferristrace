# 0025. The product is named Ferristrace

Status: accepted
Date: 2026-11-02

## Context
The repository, the image (`ghcr.io/ferrislabs/ferristrace`) and every crate
(ADR 0023) are named Ferristrace, while the binary, the settings, the
database, the token and cookie prefixes, the front end and the documentation
still said Comettrace. Two names for one product leak into every command an
operator types and every key a client holds.

## Decision
Everything is named Ferristrace: the binary and CLI `ferristrace`, settings
under `FERRISTRACE_*`, the compose project and the Postgres and ClickHouse
database, user and default password `ferristrace`, the `x-requested-with`
value `ferristrace-web`, and the front end and documentation, ADRs included.
The short prefixes follow: `ft_live_` for ingest keys, `ft_ss_` for session
tokens, `ft_pr_` for reset links, `ft_2f_` for second-step challenges, the
cookies `ft_session` and `ft_second_step`, and `ft-` for browser storage
keys. No old name is read as a fallback.

## Alternatives rejected
- Keep the `ct_` prefixes: issued keys keep working, but the old name stays
  in every token and cookie for good.
- Accept `COMETTRACE_*` and `ct_` beside the new names: a compatibility path
  to maintain and remove, for a 0.1.0 with no published release.
- Leave the accepted ADRs as written: the history would keep naming crates,
  commands and settings that no longer exist.

## Consequences
- Gains: one name in every command, setting, key and screen.
- Costs: an existing deployment re-issues its ingest keys and signs everybody
  in again, renames its settings, and starts on new volumes (deploy/README.md).
- What becomes harder: reading an old log or an old key, whose prefix no
  longer says what it is.
