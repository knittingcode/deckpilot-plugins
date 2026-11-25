//! What a control key shows, from `call.state`: states only, never text of
//! the call (no title, no name), so a face says the same in every language
//! and in a shared room.
//!
//! - No call: nothing (the key shows its own title).
//! - A control the call app does not allow now (Teams: every control): a
//!   cross on a grey key. Pressing it says why.
//! - A command waiting for its confirmation: "running", never "done".
//! - Not confirmed in time: a warning, for a moment.
//! - The microphone and the camera: on or off only as the call app
//!   reported it; unknown stays unknown (Teams may hold the microphone
//!   while muted).

use deckpilot_plugin_sdk::{Accent, Glyph, Indicator, KeyState};

use crate::host::CallState;

/// The face of the control action `id` for `state`.
pub fn face(id: &str, state: &CallState) -> KeyState {
    if !state.is_live() {
        return KeyState::new();
    }
    let supported = state.controls.get(id).is_some_and(|s| s.supported);
    if !supported {
        return KeyState::new()
            .indicator(Indicator::Unknown)
            .glyph(Glyph::Cross)
            .accent(Accent::Grey);
    }
    let mut face = KeyState::new();
    if state.pending {
        face = face.indicator(Indicator::Running);
    } else if state.unconfirmed {
        face = face.indicator(Indicator::Warning);
    } else if !state.connected {
        face = face.indicator(Indicator::Error);
    }
    match id {
        "mic-toggle" | "mic-mute" | "mic-unmute" => {
            if let Some(muted) = state.mic_muted {
                face = face.toggle(!muted);
            }
        }
        "camera-toggle" | "camera-on" | "camera-off" => {
            if let Some(on) = state.camera_on {
                face = face.toggle(on);
            }
        }
        "answer" if state.state == "incoming" => face = face.accent(Accent::Green),
        "reject" | "end" | "cancel-outgoing" => face = face.accent(Accent::Red),
        _ => {}
    }
    face
}
