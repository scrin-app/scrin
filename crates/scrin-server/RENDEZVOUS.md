# scrin rendezvous HTTP API

The rendezvous role maps 9-digit scrin IDs (and short-lived passphrase locators, D24) to a
device public key and an address hint. Schemas live in `crates/scrin-server/src/api.rs`, the
signature layout in `crates/scrin-server/src/auth.rs`. The browser gateway is described in
[`GATEWAY.md`](GATEWAY.md).

## Signed requests

Every mutating request carries `device_pub` (or `controller_pub`), `timestamp` (Unix seconds) and
`signature` (hex Ed25519) over:

```text
label (ASCII) || 0x00 || device_pub (32) || timestamp (u64 BE) || body_len (u32 BE) || body
```

Timestamps more than 300 s from the server clock are rejected (`401 expired`). The label is per
operation, so a signature for one endpoint is never valid for another.

| Endpoint | Label | Signed `body` |
|---|---|---|
| `POST /v1/register` | `scrin rendezvous register v1` | `relay_url \n direct_addr \n …` (`AddrHint::canonical`) |
| `POST /v1/presence` | `scrin rendezvous presence v1` | same as register |
| `POST /v1/resolve` | `scrin rendezvous resolve v1` | the 9-digit `id` |
| `POST /v1/report-failure` | `scrin rendezvous report-failure v1` | `controller_pub` or empty |
| `POST /v1/abuse` | `scrin rendezvous abuse v1` | `subject_pub \n subject_id \n reason` |
| `POST /v1/locator` | `scrin rendezvous locator v1` | empty |
| `POST /v1/locator/release` | `scrin rendezvous locator-release v1` | empty |

## Endpoints

| Endpoint | Auth | Response |
|---|---|---|
| `POST /v1/register` | host key | `{ id, created, presence_ttl }` |
| `POST /v1/presence` | host key (registered) | `{ id, expires_in }` |
| `GET /v1/resolve/{id}` | anonymous | `ResolveResp` |
| `POST /v1/resolve` | controller key | `ResolveResp` |
| `POST /v1/report-failure` | host key (registered) | `{ locked }` |
| `POST /v1/abuse` | reporter key (registered) | `{ reports, blocked }` |
| `POST /v1/locator` | host key (registered) | `{ locator, expires_in }` |
| `POST /v1/locator/release` | host key (registered) | `{ released }` |
| `GET /v1/locator/{n}` | anonymous | `ResolveResp` |

`ResolveResp` = `{ "id": "123456789", "device_pub": "<hex32>", "addr_hint": { "relay_url": "…",
"direct_addrs": ["…"] }, "expires_in": 42 }` (`expires_in` = seconds of presence left).

Errors are `{ "error": "<code>", "message": "…" }`:

| Status | Codes |
|---|---|
| 400 | `bad_request` |
| 401 | `bad_signature`, `expired` |
| 403 | `blocked` |
| 404 | `not_registered`, `offline`, `unknown_locator` |
| 429 | `rate_limited`, `locked_out` (with `Retry-After: 60`) |
| 503 | `exhausted` (with `Retry-After: 60`) |

## Passphrase locators (D24)

A host that wants to be reachable by two words instead of its 9-digit ID asks the server for a
**locator**: a random number in `0..=1_048_575` (20 bits) that is unique among active locators
and maps to the host's registered ID. The client turns the locator into the two words; the
secret used for pairing is separate and client-side (a PAKE password), so the server never sees
it and the locator alone grants nothing beyond what resolving the ID does.

### `POST /v1/locator`

```json
{ "device_pub": "<hex32>", "timestamp": 1800000000, "signature": "<hex64>" }
```

Signed with label `scrin rendezvous locator v1` and an **empty** body. Requirements: valid
signature, key registered (`404 not_registered`), key not blocked (`403 blocked`); shares the
per-IP write limit with register/presence. The server draws a uniform random locator from the OS
CSPRNG, retrying on collision with an active locator up to 64 times (then `503 exhausted`).
A host holds at most one locator: a new request **replaces** the old one, which stops resolving
immediately.

```json
{ "locator": 731045, "expires_in": 600 }
```

`expires_in` is the locator lifetime (`--locator-ttl` / `SCRIN_LOCATOR_TTL`, default 600 s).
A host that wants to stay findable longer requests a new locator (new words) before it expires.

### `POST /v1/locator/release`

Same body, label `scrin rendezvous locator-release v1`, same checks. Frees the host's locator;
idempotent: `{ "released": true }` if one was freed, `{ "released": false }` otherwise.

### `GET /v1/locator/{n}`

Anonymous. `n` is decimal, at most 7 digits, `<= 1048575` (else `400 bad_request`). In order:

1. per-IP limit (own bucket, 10 burst, 12/min) → `429 rate_limited`;
2. per-locator limit across all IPs (10 burst, 12/min) → `429 rate_limited`;
3. unknown or expired locator → `404 unknown_locator`;
4. pairing-failure lockout of the mapped ID (same as `GET /v1/resolve/{id}`) → `429 locked_out`;
5. host presence expired → `404 offline`;
6. `200` with the same `ResolveResp` as resolving the mapped ID.

Metrics: `scrin_locator_allocations_total`, `scrin_locator_lookups_total`,
`scrin_locator_misses_total`.

Storage: table `locators(locator INTEGER PRIMARY KEY, device_id INTEGER NOT NULL UNIQUE,
expires_at INTEGER NOT NULL)` (SQLite schema version 2). Expired rows are ignored by lookups and
purged on the next allocation.
