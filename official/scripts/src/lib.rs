//! `deckpilot.scripts`, spike S1 (DeckPilot `design-projects-scripts.md`
//! §4.1): QuickJS-NG through rquickjs 0.14, built into this plugin's
//! `wasm32-unknown-unknown` module, with no import but the host ABI's.
//!
//! The spike's one action, `selftest`, starts a QuickJS runtime and
//! evaluates a constant expression; it takes no script. The runtime, the
//! API and the bridge follow in the next commit.
#![deny(unsafe_code)]

mod clock;

use deckpilot_plugin_sdk::prelude::*;

/// The plugin's id and version, as in `manifest.json`.
pub const ID: &str = "deckpilot.scripts";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn plugin() -> Plugin {
    Plugin::new(ID, VERSION).action(Action::new("selftest", selftest))
}

deckpilot_plugin_sdk::plugin!(plugin);

fn selftest(_ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    let fail = |e: rquickjs::Error| PluginError::new("runtime", e.to_string());
    let rt = rquickjs::Runtime::new().map_err(fail)?;
    rt.set_memory_limit(32 << 20);
    let ctx = rquickjs::Context::full(&rt).map_err(fail)?;
    clock::set(0.0);
    let n: i32 = ctx.with(|ctx| ctx.eval("[1, 2, 3].reduce((a, b) => a + b, 0)")).map_err(fail)?;
    Ok(Output::message(format!("QuickJS evaluated {n}")))
}
