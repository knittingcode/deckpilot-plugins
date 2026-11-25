//! `deckpilot.calls` ("Calls" / "Rozmowy"): calls and meetings on Stream
//! Deck keys (DeckPilot `design-calls.md`, `docs/calls.md`).
//!
//! **A thin plugin.** DeckPilot's agent holds the call providers, the state
//! engine, the Calls pages (`callOverlay`, laid out from this manifest's
//! `layoutTemplates`) and "Back to work". On the Calls pages the agent
//! runs the keys itself. This module only serves the call keys a user puts
//! on their own pages:
//!
//! - a control action asks the host for one action on the focused call
//!   (`call.control`) and answers only once the call app confirmed it;
//! - its key state reads the focused call (`call.state`) and shows states
//!   only, never text of the call ([`faces`]);
//! - "Back to work" works on the Calls pages only; on another key it says
//!   so (`calls-pages-only`).
//!
//! **Microsoft Teams is detection only.** Microsoft retired the local API
//! for devices on 2026-06-30: DeckPilot can tell that Teams holds the
//! microphone, nothing more, and every control is refused with its reason.
//! Nothing here claims a Teams control.
//!
//! **Privacy.** The module never reads the meeting title (only with the
//! user's opt-in would the host send one), never logs anything of a call,
//! and has no way out of the agent: no network, credentials, opening pages,
//! tools or resources (the manifest rules and the broker refuse them with
//! `calls`).
//!
//! **Held.** No DeckPilot provides plugin API 1.16 yet: the plugin is
//! `held` in `official/plugins.json` (never packaged, signed or released)
//! until its host side, the reviewed policy entry and bundling land.
#![deny(unsafe_code)]

pub mod faces;
pub mod host;

use deckpilot_plugin_sdk::prelude::*;

use host::{CallAction, Refusal, codes};

/// The plugin's id and version, as in `manifest.json`.
pub const ID: &str = "deckpilot.calls";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

impl From<Refusal> for PluginError {
    fn from(r: Refusal) -> Self {
        let error = PluginError::new(r.code, r.message);
        if r.retryable {
            error.retryable()
        } else {
            error
        }
    }
}

/// Asks for `action` on the focused call.
fn run(action: CallAction) -> ActionResult {
    let outcome = host::control(action)?;
    Ok(Output::ok().with("outcome", outcome.as_str()))
}

/// The face of the control action `id`.
fn shown(id: &str) -> Result<KeyState, PluginError> {
    Ok(faces::face(id, &host::state()?))
}

macro_rules! control {
    ($run:ident, $state:ident, $id:literal) => {
        fn $run(_ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
            run(host::action_of($id).expect("a control action"))
        }

        fn $state(_ctx: &mut Context<'_>, _args: &Args<'_>) -> Result<KeyState, PluginError> {
            shown($id)
        }
    };
}

control!(answer, answer_state, "answer");
control!(reject, reject_state, "reject");
control!(cancel_outgoing, cancel_outgoing_state, "cancel-outgoing");
control!(end, end_state, "end");
control!(mic_toggle, mic_toggle_state, "mic-toggle");
control!(mic_mute, mic_mute_state, "mic-mute");
control!(mic_unmute, mic_unmute_state, "mic-unmute");
control!(camera_toggle, camera_toggle_state, "camera-toggle");
control!(camera_on, camera_on_state, "camera-on");
control!(camera_off, camera_off_state, "camera-off");

/// "Back to work" outside the Calls pages: the agent runs it on those
/// pages itself, so here it only says where it works. Its parameters are
/// checked like any action's.
fn return_to_work(_ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    Err(PluginError::new(
        codes::PAGES_ONLY,
        "Back to work works on the Calls pages, during a call",
    ))
}

/// The plugin definition; `deckpilot_plugin_sdk::plugin!` exports it.
pub fn plugin() -> Plugin {
    let after = || ["resume", "stay"];
    Plugin::new(ID, VERSION)
        .action(Action::new("answer", answer).state(answer_state))
        .action(Action::new("reject", reject).state(reject_state))
        .action(Action::new("cancel-outgoing", cancel_outgoing).state(cancel_outgoing_state))
        .action(Action::new("end", end).state(end_state))
        .action(Action::new("mic-toggle", mic_toggle).state(mic_toggle_state))
        .action(Action::new("mic-mute", mic_mute).state(mic_mute_state))
        .action(Action::new("mic-unmute", mic_unmute).state(mic_unmute_state))
        .action(Action::new("camera-toggle", camera_toggle).state(camera_toggle_state))
        .action(Action::new("camera-on", camera_on).state(camera_on_state))
        .action(Action::new("camera-off", camera_off).state(camera_off_state))
        .action(
            Action::new("return-to-work", return_to_work)
                .param(
                    Param::choice("mode", &["timer", "completion", "completionOrTimeout"])
                        .default("completionOrTimeout"),
                )
                .param(
                    Param::integer("seconds")
                        .min(5.0)
                        .max(600.0)
                        .default(30_i64),
                )
                .param(
                    Param::integer("maxSeconds")
                        .min(10.0)
                        .max(1800.0)
                        .default(60_i64),
                )
                .param(Param::choice("onFailure", &after()).default("resume"))
                .param(Param::choice("onCancel", &after()).default("stay"))
                .param(Param::boolean("returnAllDevices").default(false))
                .param(Param::boolean("countNavigation").default(false)),
        )
}

deckpilot_plugin_sdk::plugin!(plugin);

#[cfg(test)]
mod contract;
#[cfg(test)]
mod tests;
