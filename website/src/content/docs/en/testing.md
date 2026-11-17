---
title: "Testing"
description: "Unit-test a plugin natively with MockHost and manifest_mismatches, then try it against a real agent and a local throwaway registry."
group: ship
order: 12
---

A plugin can be tested at two levels. Unit tests run natively with `cargo test`: the SDK's mock host stands in for the agent, so no WebAssembly and no DeckPilot are involved. For an end-to-end check, a locally built agent installs the plugin from a local registry signed with throwaway keys.

## Native unit tests

Because the crate also has the `rlib` crate type, its tests are ordinary Rust tests:

```sh
cargo test --locked
```

Inside the DeckPilot checkout, run it in `plugins/runtime` with `-p <your-plugin>`.

The `deckpilot_plugin_sdk::testing` module provides two tools: `MockHost` and `manifest_mismatches`.

## Code and manifest agree

`manifest_mismatches(&plugin, &manifest)` lists every difference between the definition in code and `manifest.json`: id, version, `engines.pluginApi`, `engines.sdk`, action ids and their order, and each parameter's `required`, kind, bounds, options and default. Assert that the list is empty.

```rust
const MANIFEST: &str = include_str!("../manifest.json");

#[test]
fn definition_is_valid_and_matches_the_manifest() {
    plugin().validate().unwrap();
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    assert_eq!(manifest_mismatches(&plugin(), &manifest), Vec::<String>::new());
}
```

A second test keeps the error codes in step:

```rust
#[test]
fn every_own_code_is_declared() {
    let manifest: Value = serde_json::from_str(MANIFEST).unwrap();
    let declared = manifest["contributes"]["errorCodes"].as_object().unwrap();
    for code in PLUGIN_CODES {
        assert!(declared.contains_key(*code), "{code} is not declared");
        assert!(!codes::is_shared(code), "{code} is a shared code");
    }
}
```

### Validating against the schema

`manifest_mismatches` compares code and manifest; it does not validate the manifest itself. The example plugins validate theirs in a unit test against [plugins/schemas/plugin-manifest.schema.json](https://github.com/knittingcode/deckpilot/blob/main/plugins/schemas/plugin-manifest.schema.json) with the `jsonschema` crate as a dev-dependency (the test `manifest_is_valid_against_the_schema`). Any JSON Schema draft 2020-12 validator works. The complete validation, including cross-field rules, runs when you [package](../packaging/#the-package-command) the plugin.

## MockHost

A `MockHost` answers capability requests like the agent would, and records them.

| Constructor | Behaviour |
| --- | --- |
| `MockHost::allow_all()` | Every request succeeds with `null`: an empty HTTP 200 for fetches, a run started from the app for the run context |
| `MockHost::deny_all()` | Every request is denied like an ungranted permission |
| `MockHost::new(closure)` | The closure receives each `CapabilityRequest` and returns a `CallResult` |

| Method | Does |
| --- | --- |
| `host.invoke(&plugin, action, json_params)` | Runs one invocation as the host would: init, parameter checks, handler, result checks. Returns an `ActionOutcome` |
| `host.requests()` | The requests made so far, in order |
| `host.context()` | A `Context` whose calls go to this mock, for testing a helper function directly |

### A pure action

An action that needs no capability must work with everything denied. This test is from the Hello World example:

```rust
    #[test]
    fn greet_asks_for_nothing() {
        // Every capability denied: greet must still work.
        let mut host = MockHost::deny_all();
        let out = host.invoke(&plugin(), "greet", json!({"name": "  Ada "}));
        let ActionOutcome::Ok { message, data } = out else {
            panic!("{out:?}")
        };
        assert_eq!(message.as_deref(), Some("Hello, Ada!"));
        assert_eq!(data["greeting"], ParamValue::String("Hello, Ada!".into()));
        assert!(host.requests().is_empty());
    }
```

### Scripting the agent

For actions that use capabilities, script the answers. This host has one setting, a stored credential and a canned HTTP answer:

```rust
/// A scripted agent: a stored token, one setting and a canned API answer.
fn status_host(status: u16, body: Value) -> MockHost {
    MockHost::new(move |request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(json!({ "defaultService": "api" })),
        CapabilityRequest::CredentialHas(_) => CallResult::Ok(json!(true)),
        CapabilityRequest::HttpFetch(_) => CallResult::Ok(json!({
            "status": status,
            "headers": [{ "name": "date", "value": "Mon, 16 Nov 2026 10:00:00 GMT" }],
            "body": body.to_string(),
        })),
        _ => CallResult::Ok(Value::Null),
    })
}
```

Then assert the outcome and what the plugin asked for:

```rust
#[test]
fn a_missing_service_is_a_typed_error() {
    let mut host = status_host(404, json!({}));
    let out = host.invoke(&plugin(), "check", json!({ "service": "nope" }));
    assert!(
        matches!(&out, ActionOutcome::Failed { code, .. } if code == "service-not-found"),
        "{out:?}"
    );
}
```

### Denials

Test that a denied capability becomes a clean error, as it will after the user revokes a permission:

```rust
#[test]
fn a_denied_request_is_not_permitted() {
    // Settings and the credential answer; the fetch is denied like a revoked grant.
    let mut host = MockHost::new(|request| match request {
        CapabilityRequest::SettingsGet(_) => CallResult::Ok(json!({})),
        CapabilityRequest::CredentialHas(_) => CallResult::Ok(json!(true)),
        _ => CallResult::error(CallErrorCode::NotPermitted, "not granted"),
    });
    let out = host.invoke(&plugin(), "check", json!({ "service": "api" }));
    assert!(
        matches!(&out, ActionOutcome::Failed { code, .. } if code == codes::NOT_PERMITTED),
        "{out:?}"
    );
}
```

### What the mock does not check

The mock host is not the broker. It does not know your manifest, so it does not refuse an undeclared host, an undeclared variable id or an output the manifest does not list, and it applies no rate limits. Those checks happen in the agent. Parameter kinds that exist only in the SDK (`Param::number`, `Param::secret`) also work under the mock but not through the agent.

## Against a real agent

The DeckPilot repository documents a manual end-to-end check on macOS or Linux in [docs/marketplace/local-testing.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/local-testing.md). In outline:

1. **Build** an agent with the `plugins` feature, the registry tool, the plugin host and the module:

   ```sh
   cargo build -p deckpilot-agent --features plugins
   cargo build -p deckpilot-plugin-registry --features publish --bin deckpilot-registry
   (cd plugins/runtime &&
     cargo build --locked -p deckpilot-plugin-host --bin deckpilot-plugin-host &&
     cargo build --locked -p hello-weather --target wasm32-unknown-unknown --profile guest)
   ```

2. **Create a local registry** with throwaway keys. The guide's script generates root, index and timestamp keys with `deckpilot-registry keys generate`, initializes and signs a key set, adds a publisher, packages the plugin, and then runs `add-version`, `build-index`, `sign-index`, `timestamp` and `verify-site`.
3. **Start the agent**, pointing at the host, the local registry and its roots. An agent built with the `plugins` feature has plugins on unless it is started with `--disable-plugins` or `DECKPILOT_ENABLE_PLUGINS=0`:

   ```sh
   target/debug/deckpilot-agent --headless --no-usb \
     --plugin-host "$DECKPILOT_PLUGIN_HOST" \
     --plugin-registry "$REG" --plugin-registry-roots "$KEYS/roots.json"
   ```

   The host path must be absolute. The app shows its Plugins page only when started with `VITE_DECKPILOT_PLUGINS=1`.
4. **Walk through** install, consent, enable, run, revoke, update with an added permission, rollback and uninstall in the app.

To test your own plugin this way, replace the sample's directory and module with yours. The keys made here are for local use only; they have nothing to do with the public registry.

The same flow is automated for the example plugins in `apps/agent/tests/plugins_examples.rs`, with canned HTTP responses:

```sh
cargo test -p deckpilot-agent --features plugins --test plugins_examples --locked -- --nocapture
```

## A checklist before packaging

- `manifest_mismatches` is empty and the manifest validates.
- Every action has a test under `deny_all()` or a scripted host, including a denial.
- Every own error code is declared.
- Every output key the code sets is declared in `node.outputs`.
- No input can cause a panic.

## Canonical reference

The Testing section of [docs/marketplace/sdk/building.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/building.md) and [docs/marketplace/local-testing.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/local-testing.md).
