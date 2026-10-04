---
name: device-verify
description: Prove a scrin change works on real hardware — Windows↔Windows across two networks (this PC ↔ ssh dragos-vivobook), Samsung Galaxy A51 (adb R58N94BMLJY) as client and as attended host, and the browser client through the gateway — and record VERIFIED vs EXPECTED evidence in docs/tracker.csv. Use after any change to capture, encode, transport, input, pairing, permissions or Android code, and before marking a device-facing row done.
---

# Verify on devices

Host gates never exercise DXGI, Media Foundation, SendInput, MediaProjection, AccessibilityService,
NAT traversal or WebTransport. A claim about them is **EXPECTED** until seen on the device. Scratch
output goes in `.copilot-tmp/device/<stamp>/`.

## 1. Reach the devices

```powershell
tailscale status                                   # FIRST if ssh times out (tailscale may be stopped)
ssh dragos-vivobook "ver"                          # Windows, user vladu, default shell cmd
adb devices                                        # expect R58N94BMLJY  device
```

- vivobook runs **cmd** by default: send PowerShell as `powershell -EncodedCommand <base64 UTF-16LE>`.
- GUI apps (scrin desktop) must start in the user session through an **Interactive scheduled task**
  (`schtasks /create /tn scrin-verify /tr "<exe>" /sc once /st 00:00 /it /f` then `schtasks /run /tn scrin-verify`);
  starting them over ssh runs in session 0 with no desktop.
- Copy builds with `scp` or, if ssh fails, `tailscale file cp <file> dragos-vivobook:`.

## 2. Windows ↔ Windows over two networks

1. Build both sides from the same commit; note the short sha.
2. Prove the networks differ: on each side `curl.exe -s https://ifconfig.me` (different public IPs)
   — or put the laptop on a phone hotspot. Same LAN is not a two-network test.
3. Host on vivobook shows ID + code; connect from this PC. Record:
   - SAS identical on both screens (screenshot both);
   - path: direct vs relay (from the session stats panel / `tracing` line), RTT, fps, bitrate;
   - wrong code → rejected; reused code → rejected.
4. Exercise what changed: input (keyboard incl. Ctrl+Alt+Del, mouse), clipboard both ways, file both
   ways, audio, UAC prompt and login screen (S7, service build), permission revoke mid-session.
5. Screenshot on the laptop via an Interactive task running a capture script, pull it back.

## 3. Android A51 (adb -s R58N94BMLJY)

```powershell
$s = 'R58N94BMLJY'
adb -s $s install -r android/app/build/outputs/apk/gms/debug/app-gms-debug.apk
adb -s $s shell dumpsys package ro.dragoscatalin.scrin | Select-String 'lastUpdateTime|versionName'
adb -s $s shell monkey -p ro.dragoscatalin.scrin -c android.intent.category.LAUNCHER 1
Start-Sleep 5; $p = (adb -s $s shell pidof -s ro.dragoscatalin.scrin).Trim()   # empty = died
adb -s $s logcat -d --pid=$p | Select-String 'FATAL|panicked|AndroidRuntime|ANR'
adb -s $s shell input keyevent KEYCODE_WAKEUP
adb -s $s exec-out screencap -p > .copilot-tmp/device/a51.png
```

- `lastUpdateTime` must be now — otherwise you are testing a stale APK.
- Zero crash-scan lines is the pass condition; re-scan after each interaction.
- A tiny all-black PNG = screen dozing; wake and retake. Look at the image before describing it.
- **Client**: A51 controls the Windows PC — touch → mouse, keyboard, decode latency in stats.
- **Attended host**: Windows controls the A51 — MediaProjection consent shown **this session**,
  FGS notification present (`adb -s $s shell dumpsys activity services ro.dragoscatalin.scrin`),
  accessibility prominent disclosure shown before enabling, gestures land, revoke stops input.
- Test both flavours when Gradle or dependencies changed (`gms` and `foss`).

## 4. Browser client

- Through the gateway (`scrin-server`), Chrome and Firefox (Safari 26 when available) on this PC
  controlling the vivobook host. Use the built-in browser tools for screenshots.
- Check: WebTransport used (or WebSocket fallback logged), WebCodecs decoder config, SAS shown,
  input works, no console errors (`read_page` + console), axe clean on the session screen.

## 5. Record evidence

In `docs/tracker.csv`, `evidence` for the row (quote it if it contains commas):

- `VERIFIED <date> <sha>: PC(<ISP A>)->vivobook(<ISP B>) relay, SAS equal, 60fps 18ms RTT; shots .copilot-tmp/device/<stamp>/`
- `EXPECTED <date>: compiled + unit tests; not run on A51 (adb offline)` — the row stays `doing`.

Only VERIFIED evidence moves a device-facing row to `done`. Then:

```powershell
pwsh -NoProfile -File scripts/check-tracker.ps1
```

## Done means

- [ ] Same commit sha on every device; fresh install confirmed (`lastUpdateTime`, version)
- [ ] Two different public IPs for the Windows↔Windows test
- [ ] SAS matched; wrong/reused code rejected
- [ ] Changed behaviour exercised on each affected surface (Windows, A51 client/host, browser)
- [ ] Zero crash-scan lines on A51; screenshots viewed and described
- [ ] Tracker evidence written as VERIFIED or EXPECTED; `check-tracker.ps1` passes
