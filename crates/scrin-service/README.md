# scrin-service

Optional Windows service (tracker W-007). The device owner installs it on purpose (the installer's
"service" option, or the command below). Without it, scrin still works for everything except the
lock screen, elevation (UAC) prompts and Ctrl+Alt+Del.

## What it does, and why each privilege is needed

| Job | Why it needs the service |
|---|---|
| Keep one scrin agent (the desktop app, `--agent`) running in the **active console session** and restart it with backoff (1 s → 60 s) after a crash; follow logon, fast user switching and RDP takeover. | A process started by the user cannot outlive logoff or appear before logon. |
| Start that agent with a **copy of the service's SYSTEM token moved into the console session** (`DuplicateTokenEx` + `SetTokenInformation(TokenSessionId)` + `CreateProcessAsUserW` on `winsta0\default`). | Only a SYSTEM process in the user's session may switch to the secure desktop, so a session the owner **already accepted** keeps showing the screen on the lock screen and UAC prompts. Accepting sessions, permissions and anti-scam rules are unchanged: they live in the agent. |
| Send **Ctrl+Alt+Del** with `SendSAS` when the agent asks. | Windows ignores injected Ctrl+Alt+Del. `SendSAS` from a service requires the policy `SoftwareSASGeneration` (bit 1 = services), which `install` sets and `uninstall` restores. |

The agent asks over the pipe `\\.\pipe\scrin-service`. Its ACL allows only `LocalSystem`
(`D:P(A;;GA;;;SY)`), remote clients are rejected, and the service acts only on requests from the
exact process it launched (the client PID comes from `GetNamedPipeClientProcessId`, never from the
message) after that process sent `Hello{pid}`. Logic is in `src/supervisor.rs` and `src/ipc.rs`
(unit tested); Win32 calls are in `src/win.rs`.

## Commands (elevated)

```powershell
scrin-service install "C:\Program Files\scrin\scrin.exe"   # register (auto start, restart on failure), set policy, start
scrin-service status
scrin-service uninstall                                    # stop, delete, restore the previous policy value
```

## Manual verification (needs administrator; not run in CI)

1. Build: `cargo build -p scrin-service --release` and the desktop app.
2. Elevated PowerShell: `scrin-service install <path to scrin.exe>`; `scrin-service status` prints `Running`.
3. Task Manager → Details: `scrin.exe` runs as `SYSTEM` in your session id (not 0).
4. From another device, connect and accept the session. Press **Win+L** on the host: the controller
   still sees the lock screen. Trigger a UAC prompt: the controller sees it.
5. Controller toolbar → **Ctrl+Alt+Del**: the security screen opens on the host.
6. Kill `scrin.exe`: it comes back within ~1 s; kill it again: ~2 s.
7. `reg query HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\Policies\System /v SoftwareSASGeneration`
   shows `0x1` (or your old value with bit 1 set); `scrin-service uninstall` restores the old value
   (or deletes it if it did not exist).
