---
title: "Credentials"
description: "Credential slots and the handles-only model: the plugin names a slot, the agent injects the secret, and plugin code never sees it."
group: capabilities
order: 9
---

Many plugins need an API token. In DeckPilot a plugin never receives one. It declares a credential slot, the user stores a secret in that slot, and when the plugin sends a request it names the slot. The agent injects the secret into that one request. The secret never enters the module's memory, its logs or its results.

## The handles-only model

A `SecretHandle` is a slot key, nothing more. The capability request the module sends contains the key and the scheme; the agent's executor adds the secret after the permission broker allowed the request.

| Step | Where | What happens |
| --- | --- | --- |
| Declare | `manifest.json` | A slot with its hosts and schemes, and a `credentials` permission that references it |
| Store | The app | The user enters the secret; it is kept in the agent's encrypted plugin database |
| Name | Plugin code | `SecretHandle::slot("apiToken")` and `Request::bearer(&handle)` |
| Inject | The agent | The broker checks slot, host and scheme; the executor adds the secret over `https` |
| Redact | The agent | Every form of the secret is replaced by `[redacted]` in what comes back |

There is no way to read a secret back. Never ask a user to paste a secret into a parameter or a setting.

## Declaring a slot

A slot is an entry in `contributes.credentials`:

```json
"credentials": [
  {
    "key": "apiToken",
    "label": "Acme API token",
    "description": "A read-only token of the Acme status API.",
    "hosts": ["status.example.com"],
    "schemes": ["bearer"],
    "setupUrl": "https://status.example.com/account/tokens"
  }
]
```

| Field | Rule |
| --- | --- |
| `key` | camelCase, unique; the name used in code |
| `label`, `description` | 1 to 48 characters; at most 300 |
| `hosts` | 1 to 8 exact hosts, each also listed in the `network` permission's `hosts`. The secret goes to these origins and nowhere else |
| `schemes` | A non-empty subset of `bearer`, `basic`, `header`, `query` |
| `headerName` | Required with `header` |
| `queryName` | Required with `query` |
| `setupUrl` | An https URL where the user creates the secret |
| `userOrigin` | Optional: the setting of a user-entered origin; see [Permissions](../permissions/#user-entered-origins) |

A plugin declares at most 8 slots. Each slot must be referenced by the `credentials` permission, and every reference must be a slot:

```json
{
  "kind": "credentials",
  "refs": ["apiToken"],
  "justification": "Sends your Acme API token to status.example.com to authenticate the status request."
}
```

## Using a slot

```rust
/// One authenticated GET. The plugin names the slot; the agent adds the
/// secret to the request. The secret never enters this module.
fn status_get(ctx: &mut Context<'_>, service: &str) -> Result<Response, PluginError> {
    let token = SecretHandle::slot(TOKEN_SLOT)?;
    if !ctx.has_credential(&token)? {
        return Err(PluginError::new(
            codes::CREDENTIAL_MISSING,
            "store an Acme API token in the plugin's credentials",
        ));
    }
    let request = Request::get(format!("{API}/v1/services/{service}"))
        .header("Accept", "application/json")
        .bearer(&token);
    let response = ctx.fetch(request)?;
```

### `SecretHandle::slot`

`SecretHandle::slot(key)` creates a handle from a slot key. It only checks that the key is well formed (camelCase, at most 32 characters) and returns a `plugin-error` otherwise. It does not contact the agent.

### `has_credential`

`Context::has_credential(&handle)` returns whether the user stored a secret for the slot: `true` or `false`, never the secret. It needs the slot's grant.

Use it to decide before sending. A plugin whose API works without a token can go without, as the Developer Tools example does for public GitHub repositories. A plugin that needs the token fails with the shared code `credential-missing`, as above.

Without the check, a request that names a slot with no stored secret fails with the kind `Unavailable`.

### `Request::bearer` and `Request::credential`

`Request::bearer(&handle)` attaches the slot as a bearer token. `Request::credential(&handle, scheme)` selects any declared scheme:

| `AuthScheme` | How the agent sends the secret |
| --- | --- |
| `Bearer` | `Authorization: Bearer …` |
| `Basic` | `Authorization: Basic …` |
| `Header` | In the header named by the slot's `headerName` |
| `Query` | In the query parameter named by the slot's `queryName` |

A plugin cannot set `Authorization` itself. The broker refuses that header, along with `Cookie` and `Proxy-Authorization`, so slots are the only way to authenticate.

## What the broker enforces

A request with a credential is allowed only when all of these hold:

- the slot is granted (`credentials:<key>`);
- the request goes to one of the slot's own hosts, and that host has a current `network` grant;
- the scheme is one the slot declares;
- the request uses `https`.

A credential never follows a redirect to another origin.

Revoking the network grant of a host withholds that host from every slot that names it. A slot left without hosts is not granted at all: `has_credential` and every request using it fail with `not-permitted`. The slot works again when the network grant is back.

## Redaction

Before the module sees a response to a request that carried a credential, the agent replaces every form of the secret (as is, form-encoded and base64) with `[redacted]` in the response headers, the body and error messages. An API that echoes a key in a pagination link, for example, arrives with the key redacted.

Redaction is a safety net, not a licence to be careless. Do not copy response bodies wholesale into messages or outputs.

## What the user sees

The user stores, replaces and removes the secret in the app. A secret is 1 byte to 16 KiB without line breaks. The app can show whether a slot is set, never its value. Uninstalling a plugin deletes its stored credentials by default; an uninstall can instead keep settings and credentials for a later install of the same plugin id.

## Testing

The mock host records the request, so a test can prove that the plugin sent a handle and no secret:

```rust
    assert_eq!(fetch.url, "https://status.example.com/v1/services/api");
    // The request carries the slot's key, never a secret.
    assert_eq!(fetch.auth.as_ref().unwrap().credential, "apiToken");
    assert!(fetch.headers.iter().all(|h| !h.name.eq_ignore_ascii_case("authorization")));
```

See [Testing](../testing/#scripting-the-agent) for the mock host that answers `credentials.has`.

## Canonical reference

The Credentials section of [docs/marketplace/sdk/capabilities.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/sdk/capabilities.md), and "Plugin data" in [docs/marketplace/agent-integration.md](https://github.com/knittingcode/deckpilot/blob/main/docs/marketplace/agent-integration.md).
