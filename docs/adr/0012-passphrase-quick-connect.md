# ADR-0012: Passphrase quick connect (D24)

- Status: accepted (2026-10-04)
- Extends: ADR-0004 (security model)

## Context

Quick connect today needs two things read aloud: a 9-digit ID and an 8-symbol one-time code
(`K7QX-M2PA`). Over a phone call, letters get misheard. The user asked for "a few words in the app
language, regenerating, globally unique among active ones, dictated, no ID and no code".

## Decision

Five words from a 1024-word list (10 bits each):

| words | bits | where it comes from | who sees it |
|---|---|---|---|
| 1–2 | 20 | **locator**, allocated by the rendezvous server (`POST /v1/locator`), unique among active locators, 600 s TTL, replaced on request | server, controller |
| 3–5 | 30 | **secret**, drawn on the host (CSPRNG), used as the SPAKE2 password with `Hello.intent = 2` | host, controller (spoken) |

- The controller resolves the locator anonymously (`GET /v1/locator/{n}`), dials, and pairs with
  SPAKE2 over the secret, bound to both endpoint ids exactly like the code (ADR-0004). The server
  never learns the secret, so it cannot pair or offline-attack it.
- **Single use.** The host consumes the secret at the first `PairStart`, right or wrong (same rule as
  the code), and draws three new secret words on the same locator. Wrong guesses count toward the
  host lockout and the server's per-id lockout, which also covers lookups by locator.
- **Strength.** 30 bits, one online guess per attempt, attempts bounded by lockout: the same
  trade-off as the 39-bit code, chosen for dictation. The PAKE password is prefixed
  (`scrin-phrase:`) and never parses as a one-time code, so the two secrets cannot be confused.
- **Lists.** `crates/scrin-crypto/wordlists/{en,ro}.txt`, 1024 words each, original, AGPL-3.0-only.
  Indices carry meaning, words do not: a host showing Romanian can be typed in English. Every word
  is identified by its first four letters after folding case and Romanian diacritics, unique within
  and across lists (`wordlists/check.ps1`). Reordering a list changes what phrases mean: lists are
  append-never, edit-never once released.
- **Privacy.** The phrase is part of `Status` (shown in the UI) and redacted in every `Debug` impl.

## Consequences

- Needs a rendezvous server; LAN-only and ticket connects still use the ID/code.
- A locator expires after 600 s: the host renews it 15 s before, which changes words 1–2.
- 2^20 locators bound concurrent phrase hosts per server to ~1 M; allocation retries 64 times, then
  returns 503 `exhausted`.
