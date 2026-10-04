# ADR-0007: Accounts backend — Hono, Drizzle, PostgreSQL 17, better-auth

Date: 2026-10-04 · Status: Accepted · Tracker: D05, D15, D06

## Context

Accounts are optional (D05): quick connect must work with no account at all. An account adds the
address book, device list, organisations (teams/MSP), policies, audit, Pro billing and
integrations. Every capability must be reachable by humans (console), scripts (CLI), integrators
(SDK) and agents (MCP) without hand-written duplicate clients.

## Decision

- **API**: `apps/api`, Hono 4 on Node 24 with `@hono/zod-openapi` (Zod 4 schemas are the single
  source for validation and the OpenAPI document) and Scalar API reference. pino logs.
- **Data**: Drizzle ORM 0.45 (relations with `defineRelations`), PostgreSQL 17 (Cloud SQL on the
  official instance, plain Postgres in the self-host bundle), identity columns, `timestamptz`.
- **Auth**: better-auth with email + passkey, TOTP second factor, OIDC/SSO for organisations.
- **Entities**: user, org, membership/role, device, device_group, address_book_entry, policy,
  session_log, audit_event, jit_grant, webhook, branding.
- **Clients**: OpenAPI → `packages/sdk` (typed TS client) → `packages/cli` and `packages/mcp`.
  The console in `apps/web` uses the SDK as well.
- **Email and billing through brivio** (D06): transactional mail and invoices/payments use brivio's
  API; no new email or billing vendor.
- **Separation from the data plane**: rendezvous (ID ↔ key registry), relay and gateway live in the
  Rust `scrin-server`, not in the accounts API. The API can be down and quick connect still works.
  Device ↔ account linking is a signed statement by the device key that the API stores.

## Consequences

- One schema change flows: Drizzle migration → Zod → OpenAPI → SDK → CLI/MCP; a gate checks the
  generated SDK is up to date.
- The accounts service holds no session keys and cannot open sessions; compromise exposes
  metadata (address books, logs), not machines. Audit records are signed by devices, so the API
  cannot forge them.
- Two server languages (Rust data plane, TS control plane) — accepted, each fits its job.
- Self-hosters can skip `apps/api` entirely and run only `scrin-server`.
- brivio becomes a runtime dependency of the official instance's billing/email; self-hosters
  configure SMTP or disable email.

## Alternatives considered

- **Rust axum for everything** — one language for the server side, but no zod-openapi/better-auth
  equivalent; passkeys, OIDC and SDK generation would be rebuilt by hand. Rejected.
- **Auth.js v5** — still beta, weaker passkey/TOTP/org support than better-auth; legacy in this
  stack. Rejected.
- **Supabase** — fast start, but couples auth and data to a vendor and complicates the
  self-host bundle. Rejected.
- **Firebase** — proprietary, not self-hostable, conflicts with AGPL self-host parity. Rejected.
