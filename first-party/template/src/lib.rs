//! Template of a DeckPilot first-party plugin.
//!
//! Two actions on one counter. It shows the parts most plugins need and
//! nothing else: a setting read on every run, a value kept in the plugin's
//! storage, an own variable, a declared output and a typed error. It asks
//! for no permission: storage, settings, own variables and the log are
//! always allowed for an enabled plugin.
//!
//! The module has no clock, no random source, no timers and no access to
//! keys or devices (plugin API 1.0); see `docs/plugin-factory.md`.
#![deny(unsafe_code)]

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

/// The plugin definition. The action ids, their order and their parameters
/// must equal `manifest.json`; a unit test compares them.
pub fn plugin() -> Plugin {
    Plugin::new(ID, VERSION)
        .action(
            Action::new("count", count)
                .param(Param::integer("step").min(1.0).max(100.0).default(1)),
        )
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
