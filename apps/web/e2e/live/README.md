# WB-002 live end-to-end

`wb002.live.ts` proves the browser client against real components on this machine:

| piece  | what runs                                                                                                                                 |
| ------ | ----------------------------------------------------------------------------------------------------------------------------------------- |
| server | `target/debug/scrin-server` — rendezvous + relay + gateway, `--tls none` on TCP, self-signed WebTransport certificate on UDP              |
| host   | `target/debug/examples/controller_cli host` (`scrin-engine --features win`): DXGI capture, Media Foundation / openh264 H.264, `SendInput` |
| client | the built SPA (`apps/web/dist`) in Google Chrome (Playwright, headless)                                                                   |

The test connects with the host's 9-digit ID and one-time code, checks that five SAS emoji are
shown, confirms them, waits for the host's anti-scam delay, checks that the canvas shows a decoded
frame (not blank), then presses Scroll Lock in the page and reads the Windows Scroll Lock state
to prove the key was injected on the host (pressed twice, so the state is restored).

`harness.ts` puts a small same-origin front door in front of the server: it serves `dist/` and
proxies `/v1/*` (HTTP and the `/v1/ws` upgrade) on TCP port _P_, and the server listens for
WebTransport on UDP port _P_. scrin-server sends no CORS headers, so the SPA and API must share an
origin — exactly the production layout. The front door counts WebSocket upgrades; the test
records which transport carried the session as a `transport` annotation.

## Run

```powershell
# shared clone: through the build queue
pwsh -NoProfile -File "$env:USERPROFILE\.copilot\hooks\run-build.ps1" -Purpose 'WB-002 live' `
  -Command 'pwsh -NoProfile -File apps/web/e2e/live/run-live.ps1'
# force the WebSocket fallback
pwsh -NoProfile -File apps/web/e2e/live/run-live.ps1 -Transport ws -SkipBuild
```

Windows only (the host captures this desktop and injects into it). Not part of `gates.ps1`.
