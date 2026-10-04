# Architecture Decision Records

An ADR records one significant decision: the context that forced it, what was decided, what it
costs, and which alternatives were rejected and why. ADRs are immutable once accepted — a changed
decision gets a new ADR that supersedes the old one (the old one's status becomes
`Superseded by ADR-NNNN`). The short decision log lives in `docs/TRACKER.md` § Decisions; every ADR
names the tracker ids (`Dxx`) it expands.

## When an ADR is required

Write one before merging a change to any of:

- the **wire format** (`proto/`, the media header, ALPN, stream kinds);
- **crypto** (identity, pairing, key storage, audit chain, update signing);
- **transport** (iroh, relays, gateway, fallbacks);
- **storage** (database engine, schema strategy, on-device secret storage);
- a **public API** (OpenAPI surface, SDK/CLI/MCP contracts, FFI surface);
- **licences** (project licence, a dependency licence exception in `deny.toml`).

## Template

```markdown
# ADR-NNNN: Title

Date: YYYY-MM-DD · Status: Proposed | Accepted | Superseded by ADR-NNNN · Tracker: Dxx

## Context
What forces the decision; constraints; facts.

## Decision
What we do, concretely (versions, parameters, names).

## Consequences
What gets easier, what gets harder, risks, follow-ups.

## Alternatives considered
- **Option** — why rejected.
```

## Index

| # | Title | Status | Tracker |
|---|---|---|---|
| 0001 | [Licence AGPL-3.0-only with DCO sign-off](0001-licence-agpl-dco.md) | Accepted | D01, D07 |
| 0002 | [One monorepo, shared Rust core, native video on desktop](0002-monorepo-layout.md) | Accepted | D06, D10, D11, D12 |
| 0003 | [Transport — iroh QUIC, WebTransport gateway, WebSocket fallback](0003-transport-iroh-webtransport.md) | Accepted | D04, D09, D13 |
| 0004 | [Security model — device keys, SPAKE2 codes, SAS, trust list](0004-security-model.md) | Accepted | D14, D05, D19 |
| 0005 | [Media pipeline — DXGI capture, hardware encode, datagrams with FEC](0005-media-pipeline.md) | Accepted | D16, D10, D13 |
| 0006 | [Android — native Kotlin/Compose with the Rust core over UniFFI](0006-android-native-uniffi.md) | Accepted | D08, D11, D18, D22 |
| 0007 | [Accounts backend — Hono, Drizzle, PostgreSQL 17, better-auth](0007-accounts-backend.md) | Accepted | D05, D15, D06 |
| 0008 | [Hosting — GCP europe, docker compose for self-host](0008-hosting-gcp.md) | Accepted | D04, D23 |
| 0009 | [Anti-scam policy for quick connect](0009-anti-scam-policy.md) | Accepted | D19, D05, D14 |
| 0010 | [UI system — one React UI package, theme engine, i18n](0010-ui-system.md) | Accepted | D12, D20, D10 |
| 0011 | [Code signing deferred until a legal entity exists](0011-code-signing-deferred.md) | Accepted | D17 |
