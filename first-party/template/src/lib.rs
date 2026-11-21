//! Template of a DeckPilot first-party plugin.
//!
//! Two actions on one counter. It shows the parts most plugins need and
//! nothing else: a setting read on every run, a value kept in the plugin's
//! storage, an own variable, a declared output and a typed error. It asks
//! for no permission: storage, settings, own variables and the log are
//! always allowed for an enabled plugin.
//!
//! It also shows plugin API 1.3: `count` declares the controls it can be
//! bound to (a key press and a clockwise dial turn, each adding `step`),
//! while `reset` declares none and so works on keys only, the default. The
//! manifest adds a toolbar entry (`contributes.navigation`) that opens a
//! host-rendered view of the plugin's variables; the plugin ships no UI.
//!
//! The module has no timers and no access to keys or devices; see
//! `docs/plugin-factory.md`.
#![deny(unsafe_code)]

use deckpilot_plugin_sdk::ApiLevel;
use deckpilot_plugin_sdk::prelude::*;

/// Must equal `manifest.json` (`id`, `version`).
pub const ID: &str = "deckpilot.template";
pub const VERSION: &str = "0.1.0";

/// The storage key of the counter.
const COUNT_KEY: &str = "count";
/// The plugin's own variable, read as `{{ plugins.deckpilot.template.count }}`.
const COUNT_VARIABLE: &str = "count";
/// Error codes of this plugin, each declared in `contributes.errorCodes`.
pub const COUNTER_FULL: &str = "counter-full";
pub const PLUGIN_CODES: &[&str] = &[COUNTER_FULL];

/// The plugin API level the plugin needs: 1.3, for `controls`. Must equal
/// `engines.pluginApi` (`^1.3`) in `manifest.json`.
pub const API: ApiLevel = ApiLevel { major: 1, minor: 3 };

/// The plugin definition. The action ids, their order, their parameters
/// and their controls must equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    Plugin::new(ID, VERSION)
        .requires_api(API)
        .action(
            Action::new("count", count)
                .param(Param::integer("step").min(1.0).max(100.0).default(1))
                // A key press or one clockwise detent of a dial adds `step`.
                // Nothing counts down, so no counter-clockwise turn.
                .controls(
                    Controls::new()
                        .key([Input::Press])
                        .encoder([Input::RotateClockwise]),
                ),
        )
        // No controls: keys only (press, release, click, double click, long
        // press), never a dial or a touch zone.
        .action(Action::new("reset", reset))
}

deckpilot_plugin_sdk::plugin!(plugin);

fn count(ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let step: i64 = args.get("step")?;
    // Capability calls first, so a cancelled run ends early.
    let settings = ctx.settings()?;
    let label = settings.text("label").unwrap_or("Count").to_owned();
    let current: i64 = ctx.storage_get_as(COUNT_KEY)?.unwrap_or(0);
    let next = current
        .checked_add(step)
        .ok_or_else(|| PluginError::new(COUNTER_FULL, "the counter reached its largest value"))?;
    ctx.storage_set(COUNT_KEY, next)?;
    ctx.publish_variable(COUNT_VARIABLE, next)?;
    Ok(Output::message(format!("{label}: {next}")).with("count", next))
}

fn reset(ctx: &mut Context<'_>, _args: &Args<'_>) -> ActionResult {
    ctx.storage_delete(COUNT_KEY)?;
    ctx.publish_variable(COUNT_VARIABLE, 0_i64)?;
    Ok(Output::message("reset"))
}

#[cfg(test)]
mod tests;
