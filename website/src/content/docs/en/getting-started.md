---
title: "Getting started"
description: "Set up the toolchain, get the Rust plugin SDK from the DeckPilot repository, then build and test the smallest example plugin."
group: start
order: 1
---

A DeckPilot plugin is a Rust crate compiled to one WebAssembly module, shipped next to a `manifest.json` that describes it. This page takes you from an empty machine to a built and tested example, and shows how your own crate depends on the SDK.

> **Status:** DeckPilot 0.6.0, the current release, does not include plugin support. Plugin support ships with the next DeckPilot release: the Marketplace screen, the sandboxed plugin host in every installer and the registry's root keys built in, with plugins on by default. Until then, an agent built from source with the `plugins` cargo feature has plugins on unless it is started with `--disable-plugins` or `DECKPILOT_ENABLE_PLUGINS=0`. The signed registry is live at `https://knittingcode.github.io/deckpilot-plugins/v1/` and publishes DeckPilot's own plugins; it is not open to other publishers yet. The SDK crate is not published to crates.io. This documentation covers plugin API 1.x, guest ABI 1 and manifest version 1.

## Prerequisites

| Tool | Why |
| --- | --- |
| `git` | The SDK and the examples live in the DeckPilot repository |
| `rustup` | The repository pins its Rust toolchain (1.99.1) in `rust-toolchain.toml`; rustup installs it on first use |
| The `wasm32-unknown-unknown` target | Plugins are built for it, without WASI |

No Node.js is needed. The JavaScript package in the repository contains type declarations only and cannot build a plugin; see [SDK overview](../sdk/#javascript).

## How you get the SDK

The Rust SDK is the crate `deckpilot-plugin-sdk`, version 0.1.0, at `plugins/runtime/guest/plugin-sdk` in [knittingcode/deckpilot](https://github.com/knittingcode/deckpilot). It is marked `publish = false` and is not on crates.io. There are two ways to depend on it.

### Inside a checkout

The example plugins are members of the `plugins/runtime` Cargo workspace and inherit the dependency from it:

```toml
[package]
name = "hello-world"
description = "Example DeckPilot plugin: one pure action, no permissions"
version = "0.1.0"
edition = "2024"
rust-version = "1.90"
license = "Apache-2.0"
publish = false
workspace = "../../runtime"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
deckpilot-plugin-sdk.workspace = true
```

The `cdylib` crate type produces the module. The `rlib` crate type lets the unit tests run natively with plain `cargo test`. A new plugin placed next to the examples must also be added to `members` and `default-members` in `plugins/runtime/Cargo.toml`.

### From your own repository

A plugin submitted to the registry lives in its author's repository, so it cannot use a workspace path. A git dependency pinned to a commit works:

```toml
[dependencies]
deckpilot-plugin-sdk = { git = "https://github.com/knittingcode/deckpilot", rev = "7f614b521a7b858d26b3f018da946e6a515cbcd4" }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[profile.guest]
inherits = "release"
opt-level = "s"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

Cargo finds the crate inside the repository and resolves its workspace-inherited fields. Three things do not carry over from the DeckPilot workspace, so your repository needs its own: a committed `Cargo.lock`, a `rust-toolchain.toml` with an exact version, and the `guest` build profile shown above (it is a copy of the one in `plugins/runtime/Cargo.toml`).

> **Note:** This setup was checked for these pages with a crate outside the repository: native tests, a `wasm32-unknown-unknown` build with toolchain 1.99.1 and packaging all succeeded against the commit above. It has not been run through the registry's CI, whose only example builds inside the DeckPilot workspace.

Always pin `rev`. The SDK is at 0.1.0 and a later commit may change its Rust API.

## Build the example

Clone the repository and build Hello World, the smallest example: one action, one parameter, no permissions.

```sh
git clone https://github.com/knittingcode/deckpilot
cd deckpilot
rustup target add wasm32-unknown-unknown
plugins/examples/build.sh hello-world
```

The script runs `cargo build --locked` in `plugins/runtime` for the `wasm32-unknown-unknown` target with the `guest` profile, copies the module to `plugins/runtime/target/examples/hello_world.wasm` and prints its sha256.

The whole plugin is this definition and one handler, from `plugins/examples/hello-world/src/lib.rs`:

```rust
pub fn plugin() -> Plugin {
    Plugin::new("acme.hello-world", "0.1.0")
        .action(Action::new("greet", greet).param(Param::text("name").required().max_length(64)))
}

// The guest ABI 1 exports (`dp_abi_version`, `dp_alloc`, `dp_invoke`, ...).
deckpilot_plugin_sdk::plugin!(plugin);

/// `greet`: "Hello, <name>!". The SDK has already checked that `name` is
/// present, is text and is at most 64 characters.
fn greet(_ctx: &mut Context<'_>, args: &Args<'_>) -> ActionResult {
    let name: String = args.get("name")?;
    let name = name.trim();
    if name.is_empty() {
        return Err(PluginError::new(
            codes::INVALID_PARAMETERS,
            "parameter `name` must not be blank",
        ));
    }
    let greeting = format!("Hello, {name}!");
    Ok(Output::message(greeting.clone()).with("greeting", greeting))
}
```

Next to it sit a `manifest.json` that declares the same id, version, action and parameter, a `LICENSE` file and a 128 by 128 pixel PNG icon. The [manifest format](../manifest/) page explains every field.

## Run the tests

Plugin unit tests run natively, without WebAssembly and without a DeckPilot agent:

```sh
cd plugins/runtime
cargo test --locked -p hello-world
```

The example's tests check that the code and `manifest.json` agree, validate the manifest against the JSON Schema, and invoke the action through the SDK's mock host. See [Testing](../testing/).

## Make a package

A `.dpplugin` package is built by the registry tool that ships in the same repository. From the repository root:

```sh
cargo run --locked -p deckpilot-plugin-registry --features publish --bin deckpilot-registry -- \
  package --plugin-dir plugins/examples/hello-world \
  --module plugins/runtime/target/examples/hello_world.wasm \
  --out hello-world-0.1.0.dpplugin
```

Packaging is not publishing. It only proves that the manifest, the module and the files fit the format; see [Packaging](../packaging/).

## Where to go next

| Goal | Page |
| --- | --- |
| Understand how a plugin runs and what it can reach | [Architecture](../architecture/) |
| Learn the SDK's types | [SDK overview](../sdk/) |
| Write the manifest | [Manifest format](../manifest/) |
| Declare actions and parameters | [Actions](../actions/), [Parameters](../parameters/) |
| Call the network, store data, use secrets | [Permissions](../permissions/), [Credentials](../credentials/), [Storage](../storage/), [Variables](../variables/) |
| Try the plugin in a real agent | [Testing](../testing/) |
| Prepare a submission | [Submission](../submission/) |

The canonical SDK documentation is in the DeckPilot repository: [docs/marketplace/sdk/README.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/README.md). The other two examples are [Developer Tools](https://github.com/knittingcode/deckpilot/blob/main/plugins/examples/developer-tools/README.md), which uses every capability, and [Hello Weather](https://github.com/knittingcode/deckpilot/blob/main/plugins/examples/hello-weather/README.md), which makes one brokered request. They are examples and are never published.
