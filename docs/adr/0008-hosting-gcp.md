# ADR-0008: Hosting — GCP europe for the official instance, docker compose for self-host

Date: 2026-10-04 · Status: Accepted · Tracker: D04, D23

## Context

The official instance needs: the accounts API (stateless HTTP), PostgreSQL, the rendezvous
service, iroh relays and the WebTransport/WebSocket gateway. Relay and gateway are long-lived
UDP/QUIC services with high egress; the API is ordinary request/response. Self-hosting must be a
first-class option for an AGPL product (ADR-0001). The user chose GCP over Hetzner (D04) and the
domain `scrin.dragoscatalin.ro` for now (D23).

## Decision

- **Region**: GCP europe (europe-west primary); EU data residency for accounts data.
- **API**: Cloud Run (`apps/api` container), min instances 0–1, autoscaling.
- **Database**: Cloud SQL for PostgreSQL 17, private IP, automated backups, PITR.
- **Relay + gateway + rendezvous**: `scrin-server` on **GCE VMs** (container-optimised OS or
  Debian + systemd), static external IPs, UDP and TCP 443 open, TLS via ACME. Cloud Run is not
  usable here: no inbound UDP/QUIC, request timeouts and scale-to-zero break long-lived sessions.
- **IaC**: Terraform (google provider) in `infra/terraform`; no console-only resources.
- **DNS**: `scrin.dragoscatalin.ro` and subdomains (`api.`, `relay-<region>.`, `gw.`);
  bundle ids `ro.dragoscatalin.scrin`. Changing the domain later is a config change, not a code
  change.
- **Self-host**: `deploy/docker-compose.yml` runs `scrin-server`, optional `apps/api` +
  Postgres, with the same images the official instance uses. It is an equal citizen: every release
  is verified against it (done criterion 4).
- Deploys follow clean-tree, commit → push → deploy, verified by an authenticated live request.

## Consequences

- **Egress cost is the main risk**: relayed and gateway sessions pay GCP internet egress (much
  higher than Hetzner). Tracked as SV-007; mitigations are maximising direct P2P, per-session
  bitrate caps on relayed paths, and adding cheaper relay regions/providers later — iroh relay
  lists make that a config change.
- All infrastructure in one cloud and one Terraform state; Secret Manager, Artifact Registry and
  Cloud Logging come for free.
- GCE VMs need patching and monitoring (no serverless for the data plane).
- Self-hosters never depend on our infrastructure; clients accept a custom server URL.

## Alternatives considered

- **Hetzner** — far cheaper egress and bare-metal UDP; rejected by the user in favour of a single
  GCP footprint (D04). Remains the first candidate for extra relay capacity if SV-007 costs bite.
- **Fly.io** — UDP support and global anycast, but another vendor, less control over egress
  pricing and Postgres. Rejected.
- **Cloudflare (Workers/Spectrum)** — no raw QUIC/UDP server with our own TLS/ALPN at reasonable
  cost; Workers cannot host iroh relays. Rejected.
