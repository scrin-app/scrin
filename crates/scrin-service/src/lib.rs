//! Windows service for scrin (W-007).
//!
//! The device owner installs it explicitly. It runs as `LocalSystem` and keeps
//! one user-facing host agent alive in the active console session, launched
//! with a SYSTEM token moved into that session so a session the owner already
//! accepted keeps working on the lock screen and elevation prompts. It also
//! sends Ctrl+Alt+Del (the secure attention sequence) when the agent it
//! launched asks for it — a remote keystroke cannot.
//!
//! - [`supervisor`]: which session, when to (re)start — pure, tested.
//! - [`ipc`]: agent ↔ service pipe messages and who may send them — pure, tested.
//! - `win`: the Win32 side (service control, `CreateProcessAsUserW`, pipe, `SendSAS`).

pub mod ipc;
pub mod supervisor;
#[cfg(windows)]
pub mod win;
