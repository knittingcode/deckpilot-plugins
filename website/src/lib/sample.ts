/**
 * The smallest plugin, as shown on the homepage. It is the Quick start
 * listing of DeckPilot's docs/marketplace/sdk/README.md, abridged from
 * plugins/examples/hello-world/src/lib.rs, which DeckPilot's CI compiles
 * and tests. Keep it identical to that listing.
 */
export const HELLO_WORLD_RS = `use deckpilot_plugin_sdk::prelude::*;

pub fn plugin() -> Plugin {
    Plugin::new("acme.hello-world", "0.1.0").action(
        Action::new("greet", greet).param(Param::text("name").required().max_length(64)),
    )
}

deckpilot_plugin_sdk::plugin!(plugin);

fn greet(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let name: String = args.get("name")?;
    let greeting = format!("Hello, {}!", name.trim());
    Ok(Output::message(greeting.clone()).with("greeting", greeting))
}
`;
