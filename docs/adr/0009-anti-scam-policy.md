# ADR-0009: Anti-scam policy for quick connect

Date: 2026-10-04 · Status: Accepted · Tracker: D19, D05, D14

## Context

Remote-desktop tools are the main instrument of tech-support and bank scams: the victim is talked
into installing the tool, reading out the code, and the attacker empties accounts or installs
persistent access. AnyDesk and TeamViewer added countermeasures only after years of abuse. The
cryptography (ADR-0004) prevents impersonation by third parties but not a victim who willingly
accepts a scammer. Anti-scam is explicitly in scope (D19). Accounts are optional (D05), so most
scam sessions will be anonymous.

## Decision

- **Pre-accept interstitial** on the host before the first connection of a quick session: plain
  language "Nobody from a bank, Microsoft, police or a delivery company will ask you to install
  this", with a short delay before Accept is enabled.
- **Verified / unverified badge**: the host sees whether the controller is a verified account
  (verified email + org, optional branding) or an anonymous device. Unverified is shown
  prominently, never as an error-coloured but ignorable footnote.
- **Anonymous-session caps** (controller without a verified account): max **60 min**, **no file
  transfer**, **no unattended setup** (cannot add itself to the trust list or set a permanent
  password), **no privacy mode** (host screen cannot be blanked), no block-input.
- **No silent unattended install from a quick session**: turning a quick session into unattended
  access always needs a separate, explicit confirmation on the host with its own warning.
- **Sensitive-app blur**: known banking/password-manager windows (and Android FLAG_SECURE content)
  are blurred for the controller unless the host explicitly reveals them.
- **Stop & report**: always-visible button on the host; ends the session, revokes trust created in
  it, and submits a report (controller key, ID, timestamps) to the rendezvous.
- **Rendezvous protections**: abuse blocklist of reported device keys/IDs, rate limits on lookups
  and pairing attempts per key/IP, heuristics for keys that open many first-contact sessions.

## Consequences

- Legitimate IT support has more friction: anonymous helpers hit the 60-minute and no-files caps.
  The answer is a free verified account (or an org), which removes the caps; this is the intended
  trade-off.
- Reports create a moderation workload and a false-report risk; blocklisting acts on keys, and
  appeals go through account verification.
- Blur lists need maintenance per platform; they are data, not code.
- Self-hosted instances can tune caps by policy; the official instance keeps the defaults.

## Alternatives considered

- **No countermeasures** (early AnyDesk/TeamViewer) — maximal convenience, and the tool becomes a
  scam vector, harming users and the project's reputation and store standing. Rejected.
- **KYC for every controller** — strongest deterrent, but kills quick connect without an account
  (D05), costs money per check and excludes privacy-minded users. Rejected; verified accounts are
  the lighter middle ground.
