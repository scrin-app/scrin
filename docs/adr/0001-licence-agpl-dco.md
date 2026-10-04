# ADR-0001: Licence AGPL-3.0-only with DCO sign-off

Date: 2026-10-04 · Status: Accepted · Tracker: D01, D07

## Context

scrin is an open-source remote desktop with three server-side components that third parties could
run as a service: the rendezvous/relay/gateway binary (`crates/scrin-server`), the accounts API
(`apps/api`) and the web client (`apps/web`). The market already contains closed commercial forks
of permissively licensed remote-desktop code, and hosted "remote support as a service" is the
obvious way to monetise a fork without giving anything back.

Requirements:

- A company may run scrin, including as a paid service, but must publish its modifications.
- The licence must cover network use, not only binary distribution — the gateway and API are
  never "distributed" to end users, they are reached over the network.
- Contribution friction must stay low: no legal paperwork before a first pull request.
- The project must be able to depend on the normal Rust and npm ecosystem (MIT/Apache/BSD/ISC).

## Decision

- All code in this repository is licensed **AGPL-3.0-only** (SPDX `AGPL-3.0-only`, not
  `-or-later`, so a future GPL version cannot change the terms without a deliberate decision).
- Every `Cargo.toml`, `package.json` and Gradle module declares the SPDX identifier; `LICENSE`
  holds the full text.
- Contributions are accepted under the **Developer Certificate of Origin 1.1**: every commit
  carries `Signed-off-by:` (`git commit -s`). CI rejects commits without it. There is **no CLA**
  and no copyright assignment; contributors keep their copyright.
- Dependency licences are enforced by `cargo-deny` (`deny.toml` allowlist) for Rust and a licence
  check in the js lane of `scripts/gates.ps1`. Allowed: MIT, Apache-2.0, Apache-2.0 WITH
  LLVM-exception, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, LGPL-*, GPL-3.0, AGPL-3.0.
  **GPL-2.0-only is denied** (incompatible with AGPL-3.0).
- The name "scrin", logos and icons are **not** covered by the AGPL; trademark/brand use is a
  separate policy (forks must rename when they ship a modified build publicly).
- The repository lives in a dedicated GitHub org (D07) so secrets, signing keys and branding are
  isolated from personal repos.

## Consequences

- Hosted forks of the server, gateway or API must offer their source to their users (AGPL §13).
  This is the intended effect.
- Because there is no CLA, the project cannot relicense later (e.g. to offer a proprietary
  edition) without the consent of every contributor. Accepted: dual licensing is not a goal.
- Pro/org features are sold as a hosted service, not as closed code; the code stays AGPL.
- Every new dependency is a licence decision. `cargo-deny check licenses` and the js licence
  check fail the gate; an exception needs an entry in `deny.toml` with a reason.
- Some codec/runtime pieces need care: openh264 is BSD-2-Clause (fine) but Cisco's binary
  licence is separate from the source licence — we build from source or document the binary
  terms; Media Foundation, WebView2 and Windows APIs are system libraries (AGPL system library
  exception).
- Apple App Store distribution of AGPL code is legally contested (store terms add restrictions).
  iOS is not in v1 (D02); revisit before an iOS client ships.
- Google Play and F-Droid both accept AGPL apps; the foss flavour (D18) must not pull
  proprietary SDKs.
- Some enterprises have AGPL bans; they can still use the official hosted instance or the
  unmodified self-host bundle. We accept the lost adoption.

## Alternatives considered

- **Apache-2.0 / MIT** — maximal adoption, but permits exactly the closed SaaS fork D01 wants to
  prevent. Rejected.
- **GPL-3.0** — copyleft on distribution only. The gateway and API are used over the network
  and never distributed, so a hosted fork could stay closed. Rejected.
- **MPL-2.0** — file-level copyleft; a fork can wrap the code in new closed files and has no
  network-use clause. Rejected.
- **BSL / FSL (source-available, time-delayed open source)** — not OSI open source, deters
  contributors and distributors (F-Droid refuses non-free licences). Rejected.
- **CLA (Apache ICLA style or copyright assignment)** — would allow relicensing, but adds legal
  friction to every first contribution and signals an intent to close the code later. DCO gives
  the provenance guarantee we need. Rejected.
