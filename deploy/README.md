# Self-hosting scrin-server

One container runs all three server roles of scrin:

- **rendezvous** — the scrin ID registry (`/v1/register`, `/v1/presence`, `/v1/resolve`, reports);
- **relay** — an embedded [iroh relay](https://docs.rs/iroh-relay) at `/relay`, used when two
  devices cannot reach each other directly; only devices registered on *your* server may use it;
- **gateway** — the browser bridge: WebTransport at `/v1/gw` (UDP 443) and a WebSocket
  fallback at `/v1/ws` (TCP 443). Contract: [`crates/scrin-server/GATEWAY.md`](../crates/scrin-server/GATEWAY.md).

The server never sees session contents: relays forward QUIC ciphertext and the gateway forwards
the end-to-end encrypted inner channel (ADR-0004). The one-time code never reaches it.

## 1. Requirements

- A Linux host with a public IPv4 (IPv6 optional), Docker Engine 24+ with Compose v2.
- 1 vCPU / 1 GB RAM handles hundreds of relayed sessions; **egress bandwidth** is the real cost
  (a 1080p session is 2–8 Mbit/s while relayed).
- A DNS name you control, e.g. `relay.example.org`.

## 2. DNS

Create an `A` record (and `AAAA` if you have IPv6) for your name pointing at the host.
Let's Encrypt validates over TLS-ALPN-01 on port 443, so the record must resolve before the
first start.

## 3. Firewall

| Port | Proto | Purpose | Required |
|---|---|---|---|
| 443 | TCP | HTTPS API, iroh relay, WebSocket gateway, ACME (TLS-ALPN-01) | yes |
| 443 | UDP | WebTransport gateway (HTTP/3) | for browsers (fallback: WebSocket) |
| 80 | TCP | captive-portal probe `/generate_204` | recommended |
| 7842 | UDP | iroh QUIC address discovery (helps hole punching) | recommended |

Example (ufw): `ufw allow 443/tcp; ufw allow 443/udp; ufw allow 80/tcp; ufw allow 7842/udp`.
On a cloud VM also open them in the provider's firewall / security group.

## 4. Start

```sh
git clone https://github.com/scrin-app/scrin && cd scrin
cp deploy/.env.example deploy/.env      # set SCRIN_DOMAIN and SCRIN_ACME_CONTACT
docker compose -f deploy/docker-compose.yml --env-file deploy/.env up -d --build
docker compose -f deploy/docker-compose.yml logs -f scrin-server
```

On first start the server requests a certificate from Let's Encrypt (`acme` lines in the log).
While experimenting set `SCRIN_ACME_STAGING=true` to avoid production rate limits (staging
certificates are not trusted by clients). Certificates and the ACME account are cached in the
`scrin-data` volume together with the SQLite database (`/data/scrin.db`).

Check it:

```sh
curl https://relay.example.org/health     # ok
curl https://relay.example.org/ready      # ready
curl https://relay.example.org/v1/info    # relay URLs, gateway, version
curl https://relay.example.org/metrics    # Prometheus text
```

`/metrics` is public by default; restrict it at your reverse proxy or firewall if that matters to you.

### TLS without ACME

Use your own certificate instead: set `SCRIN_TLS=manual`, `SCRIN_TLS_CERT=/data/fullchain.pem`,
`SCRIN_TLS_KEY=/data/privkey.pem` and copy both files into the volume. Reload means restart.

## 5. Point clients at your server

Clients accept a custom server URL (ADR-0008); enter `https://relay.example.org` where the
client asks for its server. The client reads `/v1/info` and uses:

- your server for ID registration and lookup (IDs are per server: an ID registered on your
  server is unknown on the official one and vice versa);
- `relay_urls` from `/v1/info` as its iroh relay list (no n0 public relays);
- `/v1/gw` + `/v1/ws` for browser sessions.

> The client-side setting is being built in the desktop/Android/web apps; this section is the
> contract they implement (`GET /v1/info` → relay list + gateway).

## 6. Configuration reference

All flags have a `SCRIN_*` environment variable; `scrin-server --help` prints them all.

| Variable | Default | Meaning |
|---|---|---|
| `SCRIN_ROLES` | `rendezvous,relay,gateway` | roles to run |
| `SCRIN_LISTEN` / `SCRIN_WT_LISTEN` | `0.0.0.0:443` | TCP / UDP listeners |
| `SCRIN_TLS` | `acme` | `acme`, `manual`, `self-signed`, `none` |
| `SCRIN_DB` | — (memory) | SQLite path; without it all state is lost on restart |
| `SCRIN_RELAY_URLS` | — | public relay URL(s) advertised at `/v1/info` |
| `SCRIN_RELAY_OPEN` | `false` | let unregistered devices use the relay (private networks only) |
| `SCRIN_RELAY_BPS` | `0` | per-client relay receive limit, bytes/s |
| `SCRIN_PRESENCE_TTL` | `60` | seconds a heartbeat keeps a device online |
| `SCRIN_ABUSE_BLOCK_THRESHOLD` | `3` | distinct reporters that block a device key |
| `SCRIN_GW_MAX_SECS` | `3600` | max browser session (anonymous cap, ADR-0009) |
| `SCRIN_GW_IDLE_SECS` | `120` | idle timeout of a browser session |
| `SCRIN_GW_MAX_BPS` | `3000000` | per-session gateway bandwidth cap |
| `SCRIN_TRUST_FORWARDED` | `false` | take client IPs from `X-Forwarded-For` (only behind a trusted proxy) |

## 7. Development

```sh
cargo run -p scrin-server -- --dev
```

Dev mode listens on `127.0.0.1:4433` (TCP + UDP) with a 13-day self-signed ECDSA certificate and
prints its SHA-256 (`cert-sha256: …`). Browsers accept it through
`new WebTransport(url, { serverCertificateHashes: [{ algorithm: 'sha-256', value }] })`; for
`fetch`/WebSocket either trust the certificate or run with `--tls none` (plain HTTP on TCP,
WebTransport still self-signed). Dev mode also opens the relay to unregistered devices.

## 8. Backups and upgrades

- Back up the `scrin-data` volume (SQLite + ACME cache). SQLite runs in WAL mode; copy with
  `sqlite3 /data/scrin.db ".backup /data/backup.db"` or stop the container first.
- Upgrade: `git pull && docker compose -f deploy/docker-compose.yml up -d --build`. Migrations
  are embedded and applied on start; they are additive.
