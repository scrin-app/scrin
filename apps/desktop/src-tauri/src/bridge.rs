//! UI ↔ engine bridge.
//!
//! The engine runs on its own multi-threaded tokio runtime on dedicated
//! threads, so a slow webview never stalls media and vice versa. Commands go
//! through [`Bridge::call`] with a 5 s deadline (titi pattern); events are
//! forwarded to the webview as `scrin://event`, except video frames, which go
//! to the native presenter.

use std::sync::Arc;
use std::time::Duration;

use scrin_engine::backend::InputEvent;
use scrin_engine::{Command, EngineConfig, EngineHandle, Event, Quality, Reply};
use scrin_proto::v1;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tracing::{info, warn};

use crate::video::Presenter;

/// Event name the webview listens to.
pub const EVENT: &str = "scrin://event";
const CALL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Bridge {
    rt: tokio::runtime::Runtime,
    handle: EngineHandle,
}

impl Bridge {
    /// Starts the engine and the event pump. Blocks until the endpoint is bound.
    pub fn start(
        app: AppHandle,
        cfg: EngineConfig,
        presenter: Arc<Presenter>,
    ) -> Result<Self, String> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .thread_name("scrin-engine")
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        let (handle, mut events) = rt
            .block_on(scrin_engine::start(cfg))
            .map_err(|e| e.to_string())?;
        rt.spawn(async move {
            while let Some(ev) = events.recv().await {
                match ev {
                    Event::VideoFrame { session, frame } => presenter.submit_for(&session, frame),
                    Event::StateChanged {
                        state: scrin_engine::SessionState::Ended,
                        ..
                    } => {
                        presenter.clear();
                        let _ = app.emit(EVENT, &ev);
                    }
                    other => {
                        if let Err(e) = app.emit(EVENT, &other) {
                            warn!(error = %e, "could not emit to the webview");
                        }
                    }
                }
            }
            info!("engine event stream closed");
        });
        Ok(Self { rt, handle })
    }

    /// Runs a command with the 5 s deadline. Safe from any thread except a
    /// tokio worker of the engine runtime.
    pub fn call(&self, cmd: Command) -> Result<Reply, String> {
        let h = self.handle.clone();
        self.rt
            .block_on(async move { h.call_timeout(cmd, CALL_TIMEOUT).await })
            .map_err(|e| e.to_string())
    }

    /// Async variant for `async` Tauri commands.
    pub async fn call_async(&self, cmd: Command) -> Result<Reply, String> {
        let h = self.handle.clone();
        self.rt
            .spawn(async move { h.call_timeout(cmd, CALL_TIMEOUT).await })
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())
    }

    pub fn shutdown(&self) {
        let h = self.handle.clone();
        let _ = self.rt.block_on(async move {
            tokio::time::timeout(Duration::from_secs(2), h.shutdown()).await
        });
    }
}

/// Input as the webview sends it; mapped onto `scrin.v1` messages.
#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum UiInput {
    Key {
        hid_usage: u32,
        down: bool,
        #[serde(default)]
        modifiers: u32,
        #[serde(default)]
        text: Option<String>,
    },
    MouseMove {
        display_id: u32,
        x: f32,
        y: f32,
    },
    MouseRelative {
        dx: i32,
        dy: i32,
    },
    MouseButton {
        /// 1 left, 2 right, 3 middle, 4 back, 5 forward.
        button: i32,
        down: bool,
    },
    Wheel {
        dx: i32,
        dy: i32,
    },
}

impl From<UiInput> for InputEvent {
    fn from(u: UiInput) -> Self {
        use v1::mouse_move::Motion;
        match u {
            UiInput::Key {
                hid_usage,
                down,
                modifiers,
                text,
            } => Self::Key(v1::KeyEvent {
                hid_usage,
                down,
                modifiers,
                text,
                repeat: false,
            }),
            UiInput::MouseMove { display_id, x, y } => Self::MouseMove(v1::MouseMove {
                motion: Some(Motion::Absolute(v1::AbsolutePosition {
                    display_id,
                    x: x.clamp(0.0, 1.0),
                    y: y.clamp(0.0, 1.0),
                })),
            }),
            UiInput::MouseRelative { dx, dy } => Self::MouseMove(v1::MouseMove {
                motion: Some(Motion::Relative(v1::RelativeMotion { dx, dy })),
            }),
            UiInput::MouseButton { button, down } => {
                Self::MouseButton(v1::MouseButton { button, down })
            }
            UiInput::Wheel { dx, dy } => Self::MouseWheel(v1::MouseWheel {
                delta_x: dx,
                delta_y: dy,
            }),
        }
    }
}

/// The special key combos the UI exposes (`SpecialKey` in packages/ui).
pub fn special_key_events(combo: &str) -> Option<Vec<InputEvent>> {
    // HID usages, keyboard page 0x07.
    const CTRL: u32 = 0xE0;
    const ALT: u32 = 0xE2;
    const GUI: u32 = 0xE3;
    const DEL: u32 = 0x4C;
    const TAB: u32 = 0x2B;
    const PRINT: u32 = 0x46;
    const L: u32 = 0x0F;
    let keys: &[u32] = match combo {
        "ctrl-alt-del" => &[CTRL, ALT, DEL],
        "win" => &[GUI],
        "alt-tab" => &[ALT, TAB],
        "print-screen" => &[PRINT],
        "lock" => &[GUI, L],
        _ => return None,
    };
    let key = |hid_usage, down| {
        InputEvent::Key(v1::KeyEvent {
            hid_usage,
            down,
            modifiers: 0,
            text: None,
            repeat: false,
        })
    };
    let mut out: Vec<InputEvent> = keys.iter().map(|&k| key(k, true)).collect();
    out.extend(keys.iter().rev().map(|&k| key(k, false)));
    Some(out)
}

pub fn parse_quality(q: &str) -> Option<Quality> {
    Some(match q {
        "auto" => Quality::Auto,
        "balanced" => Quality::Balanced,
        "sharp" => Quality::Sharp,
        "speed" => Quality::Speed,
        _ => return None,
    })
}

/// What `scrin_connect` returns.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub session_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_keys_press_then_release_in_reverse() {
        let ev = special_key_events("ctrl-alt-del").expect("known");
        assert_eq!(ev.len(), 6);
        let InputEvent::Key(last) = &ev[5] else {
            panic!("key")
        };
        assert_eq!((last.hid_usage, last.down), (0xE0, false));
        assert!(special_key_events("nope").is_none());
    }

    #[test]
    fn ui_input_maps_to_proto() {
        let json = r#"{"type":"mouseMove","displayId":1,"x":1.5,"y":0.25}"#;
        let u: UiInput = serde_json::from_str(json).expect("parse");
        let InputEvent::MouseMove(m) = InputEvent::from(u) else {
            panic!("move")
        };
        let Some(v1::mouse_move::Motion::Absolute(a)) = m.motion else {
            panic!("abs")
        };
        assert!((a.x - 1.0).abs() < f32::EPSILON);
        assert_eq!(parse_quality("speed"), Some(Quality::Speed));
    }
}
