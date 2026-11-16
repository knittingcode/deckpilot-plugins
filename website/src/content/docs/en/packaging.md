---
title: "Packaging"
description: "Build the module for wasm32-unknown-unknown, create a .dpplugin package with deckpilot-registry, size limits, reproducible builds, build.json."
group: ship
order: 13
---

A plugin is distributed as one file, `<id>-<version>.dpplugin`. It contains the manifest, the compiled module, the license and the assets the manifest names. This page covers building the module, creating and checking a package, the size limits, what is known about reproducible builds, and the build recipe the registry uses to rebuild your plugin.

> **Note:** Packaging is not publishing. A package you build yourself is for checking and local testing. Packages reach users only through the registry, which rebuilds them from your tagged source; see [Submission](../submission/).

## Project layout

```text
your-plugin/
├── Cargo.toml          # [lib] crate-type = ["cdylib", "rlib"]
├── Cargo.lock          # committed
├── rust-toolchain.toml # an exact toolchain version
├── manifest.json
├── LICENSE             # required in every package
├── README.md           # packaged only if the manifest names it
├── assets/icon-128.png # icons.128: a 128 by 128 pixel PNG, at most 256 KiB
└── src/lib.rs          # plugin() and deckpilot_plugin_sdk::plugin!(plugin)
```

Dependencies must build for `wasm32-unknown-unknown` without WASI and without JavaScript glue: no `wasm-bindgen`, and no random number backend that expects a browser. The package check refuses a module with any import other than `deckpilot_v1.host_call`.

## Building the module

In your own repository, with the `guest` profile from [Getting started](../getting-started/#from-your-own-repository):

```sh
rustup target add wasm32-unknown-unknown
cargo build --locked --target wasm32-unknown-unknown --profile guest
```

The module is written to `target/wasm32-unknown-unknown/guest/<crate_name>.wasm`.

The `guest` profile inherits from `release` and sets `opt-level = "s"`, `lto = true`, `codegen-units = 1`, `panic = "abort"` and `strip = true`. With it, the Hello World example is about 190 KB and Developer Tools about 300 KB.

For the examples inside the DeckPilot checkout there is a script that also remaps build paths and prints the sha256 of each module:

```sh
plugins/examples/build.sh                  # hello-world and developer-tools
plugins/examples/build.sh developer-tools  # one
```

It accepts `--target-dir DIR` and `--out DIR` and writes to `plugins/runtime/target/examples/<crate>.wasm` by default.

## The package format

A `.dpplugin` file is a gzip-compressed tar archive in a fixed, deterministic form: entries sorted by path, fixed modes and timestamps, no directory entries. It contains exactly:

| File | Required |
| --- | --- |
| `manifest.json` | yes |
| `plugin.wasm` | yes |
| `LICENSE` | yes |
| The icons, key icons, screenshots, `README.md` and `CHANGELOG.md` the manifest names | as named |

Nothing else is allowed in a package.

## The package command

The registry tool `deckpilot-registry` lives in the DeckPilot repository, in the crate `deckpilot-plugin-registry`, behind the cargo feature `publish`. From a DeckPilot checkout:

```sh
cargo run --locked -p deckpilot-plugin-registry --features publish --bin deckpilot-registry -- \
  package --plugin-dir plugins/examples/developer-tools \
  --module plugins/runtime/target/examples/developer_tools.wasm \
  --out developer-tools-0.1.0.dpplugin
```

| Option | Meaning |
| --- | --- |
| `--plugin-dir DIR` | The directory with `manifest.json`, `LICENSE` and the files the manifest names. It may be outside the checkout |
| `--module FILE.wasm` | The built module; it is stored as `plugin.wasm` |
| `--out FILE.dpplugin` | The package to write |

On success it prints the plugin id, the version, the output path, the sha256 and the size of the package.

`package` runs every check a reader of the package would run:

- the complete manifest validation, including the cross-field rules the JSON Schema cannot express;
- the size limits below;
- the static module check: a valid core WebAssembly module, only the `deckpilot_v1.host_call` import, the required exports with their exact types.

The output is deterministic: the same inputs give the same bytes.

## Size limits

| What | Limit |
| --- | --- |
| The package, compressed | 5 MiB |
| Any single file | 5 MiB |
| All files unpacked | 10 MiB |
| `plugin.wasm` | 4 MiB |
| Entries in the archive | 256 |
| `manifest.json` | 256 KiB |
| The 128 pixel icon the agent shows | 256 KiB |

## Reproducible builds

The registry does not take your binary. It rebuilds the module from your source and requires its own build to match the recorded hash, so it matters how reproducible a build is. What has been measured for the example plugins with toolchain 1.99.1:

| Comparison | Result |
| --- | --- |
| Two builds on one machine, each into a new empty target directory | Byte-identical, on Ubuntu, macOS and Windows. CI checks this on every change |
| Two machines with the same operating system | Identical for Hello World, different for Developer Tools; not analysed further |
| Different operating systems | Different for both modules |

Consequences:

- Pin the toolchain to an exact version and commit `Cargo.lock`.
- Do not expect a build on your machine to match the registry's. The registry's reference build runs in CI on `ubuntu-24.04`, x86_64, and its hash is the one that counts.
- Keep the build hermetic: no build script that downloads or executes anything, and no network access beyond Cargo's locked dependencies.

## The build recipe

In the registry repository, each plugin has a `plugins/<id>/build.json` that tells the registry how to build the module. For a plugin whose repository root is the crate:

```json
{
  "id": "acme.status-check",
  "toolchain": "1.99.1",
  "workspace": ".",
  "package": "status-check",
  "profile": "guest",
  "artifact": "status_check.wasm"
}
```

| Field | Meaning |
| --- | --- |
| `id` | The plugin id |
| `toolchain` | An exact Rust version such as `1.99.1`, installed with rustup |
| `workspace` | The directory of the Cargo workspace with its `Cargo.lock`, relative to the repository root; `.` for the root |
| `package` | The Cargo package that builds the module |
| `profile` | The Cargo profile, for example `release` or `guest` |
| `artifact` | The file name of the module under `target/wasm32-unknown-unknown/<profile>/` |

All six fields are required. The recipe allows nothing else: no custom commands, no features, no environment. The registry's build script checks out exactly the recorded commit, runs `cargo build --locked` for the recipe's package, profile and toolchain on the `wasm32-unknown-unknown` target with remapped path prefixes, and then runs `deckpilot-registry package`.

The schema is [schemas/inputs/build-recipe.schema.json](https://github.com/knittingcode/deckpilot-plugins/blob/main/schemas/inputs/build-recipe.schema.json), and the registry's own [example recipe](https://github.com/knittingcode/deckpilot-plugins/blob/main/plugins/examples/hello-weather/build.json) builds the Hello Weather example from the DeckPilot workspace.

## Checklist

- `manifest.json` validates and agrees with the code.
- `LICENSE` and the 128 pixel icon are present.
- The module builds with `--locked` from a clean checkout.
- `deckpilot-registry package` succeeds.
- The package is not committed anywhere. The registry repository refuses `.dpplugin` files, and a package is never attached to a DeckPilot application release.

## Canonical reference

[docs/marketplace/sdk/building.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/building.md), [docs/marketplace/artifact-contract.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/artifact-contract.md) and the package crate's [README](https://github.com/knittingcode/deckpilot/blob/main/crates/deckpilot-plugin-package/README.md).
