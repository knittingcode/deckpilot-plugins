//! Native unit tests: the definition agrees with manifest.json, the
//! manifest is valid against the schema, and both actions behave, with the
//! agent replaced by `MockHost`. The comparison with the built-in module
//! through the real agent is `apps/agent/tests/plugins_notifications.rs`.

use std::cell::RefCell;
use std::rc::Rc;

use deckpilot_plugin_sdk::testing::{MockHost, manifest_mismatches};
use deckpilot_plugin_sdk::{ActionOutcome, CallErrorCode, CallResult, CapabilityRequest, Params};
use serde_json::{Value, json};

use super::*;

const MANIFEST: &str = include_str!("../manifest.json");
const SCHEMA: &str = include_str!("../../../schemas/plugin-manifest.schema.json");

fn manifest() -> Value {
    serde_json::from_str(MANIFEST).unwrap()
}

#[test]
fn the_definition_matches_the_manifest() {
    assert_eq!(
        manifest_mismatches(&plugin(), &manifest()),
        Vec::<String>::new()
    );
    plugin().validate().unwrap();
}

#[test]
fn the_manifest_is_valid() {
    let schema: Value = serde_json::from_str(SCHEMA).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(&manifest())
        .map(|e| format!("{}: {e}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    // Only the low-tier `notifications` permission, so seeding enables it
    // (§4.2), with the routing the agent reads.
    let permissions = manifest()["permissions"].clone();
    assert_eq!(permissions.as_array().unwrap().len(), 1);
    assert_eq!(permissions[0]["kind"], "notifications");
    assert_eq!(
        permissions[0]["routing"],
        json!({"desktop": "desktop", "statusBar": "statusBar", "minimumSeverity": "minimumSeverity"})
    );
}

/// What the plugin sent: title, body and severity.
type Sent = (String, String, Option<NotifySeverity>);

/// A stand-in agent: answers `notify` with `delivery` and `run.context`
/// with `run`, and records the notifications.
struct Agent {
    host: MockHost,
    sent: Rc<RefCell<Vec<Sent>>>,
}

fn agent(delivery: CallResult, run: Value) -> Agent {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let seen = sent.clone();
    let host = MockHost::new(move |request| match request {
        CapabilityRequest::Notify(n) => {
            seen.borrow_mut()
                .push((n.title.clone(), n.body.clone(), n.severity));
            delivery.clone()
        }
        CapabilityRequest::RunContext(_) => CallResult::Ok(run.clone()),
        other => panic!("unexpected request {other:?}"),
    });
    Agent { host, sent }
}

fn delivered(desktop: bool, status_bar: bool) -> CallResult {
    CallResult::Ok(json!({"shown": true, "desktop": desktop, "statusBar": status_bar}))
}

fn not_shown(reason: &str) -> CallResult {
    CallResult::Ok(json!({"shown": false, "reason": reason}))
}

fn ok(outcome: ActionOutcome) -> (String, Params) {
    match outcome {
        ActionOutcome::Ok { message, data } => (message.unwrap_or_default(), data),
        other => panic!("not ok: {other:?}"),
    }
}

fn data(params: &Params) -> Value {
    serde_json::to_value(params).unwrap()
}

#[test]
fn shows_plain_text_where_the_agent_says() {
    let mut agent = agent(delivered(false, true), json!({"trigger": "app"}));
    let (message, outputs) = ok(agent.host.invoke(
        &plugin(),
        "show",
        json!({"title": "Deploy\u{7} done", "body": "<b>main</b>\tis green", "severity": "success"}),
    ));
    assert_eq!(message, "showed “Deploy done”");
    assert_eq!(
        data(&outputs),
        json!({"shown": true, "desktop": false, "statusBar": true})
    );
    let sent = agent.sent.borrow();
    assert_eq!(
        sent[0],
        (
            "Deploy done".to_owned(),
            "<b>main</b> is green".to_owned(),
            Some(NotifySeverity::Success)
        )
    );
    // The body is not repeated in the node's message (history).
    assert!(!message.contains("green"));
}

#[test]
fn long_text_is_cut_to_what_notify_takes() {
    let mut agent = agent(delivered(true, true), json!({"trigger": "app"}));
    let title = "t".repeat(100);
    let (message, _) = ok(agent.host.invoke(
        &plugin(),
        "show",
        json!({"title": title, "body": "b".repeat(500)}),
    ));
    // The message names the built-in's title (100 characters) ...
    assert_eq!(message, format!("showed “{title}”"));
    // ... the agent gets what `notify` takes.
    let (sent_title, sent_body, severity) = agent.sent.borrow()[0].clone();
    assert_eq!(sent_title.chars().count(), SENT_TITLE);
    assert!(sent_title.ends_with('…'));
    assert_eq!(sent_body.chars().count(), SENT_BODY);
    assert_eq!(severity, Some(NotifySeverity::Info));
}

#[test]
fn not_shown_goes_on_unless_critical() {
    let reason = "the app is not open and desktop notifications are off";
    let mut quiet = agent(not_shown(reason), json!({"trigger": "app"}));
    let (message, outputs) = ok(quiet
        .host
        .invoke(&plugin(), "show", json!({"title": "Hello"})));
    assert_eq!(message, format!("“Hello” was not shown: {reason}"));
    assert_eq!(
        data(&outputs),
        json!({"shown": false, "desktop": false, "statusBar": false, "reason": reason})
    );
    let mut critical = agent(not_shown(reason), json!({"trigger": "app"}));
    match critical.host.invoke(
        &plugin(),
        "show",
        json!({"title": "Hello", "critical": true}),
    ) {
        ActionOutcome::Failed { code, message, .. } => {
            assert_eq!(code, codes::NOT_SHOWN);
            assert_eq!(
                message.as_deref(),
                Some(format!("“Hello” was not shown: {reason}").as_str())
            );
        }
        other => panic!("{other:?}"),
    }

    // The rate limit and a revoked permission show nothing either.
    for (error, words) in [
        (
            CallResult::error(CallErrorCode::Throttled, "5 per minute"),
            "more than 5 notifications a minute",
        ),
        (
            CallResult::error(CallErrorCode::NotPermitted, "not granted"),
            "may not show notifications",
        ),
    ] {
        let mut refused = agent(error, json!({"trigger": "app"}));
        let (_, outputs) = ok(refused
            .host
            .invoke(&plugin(), "show", json!({"title": "x"})));
        assert_eq!(
            outputs["shown"],
            deckpilot_plugin_sdk::ParamValue::Bool(false)
        );
        assert!(
            data(&outputs)["reason"].as_str().unwrap().contains(words),
            "{outputs:?}"
        );
    }
}

fn run(failed: u32, cancelled: u32) -> Value {
    json!({
        "trigger": "workflow", "cause": "user",
        "run": {"workflow": "Start work", "elapsedMs": 12_300, "succeeded": 3,
                "failed": failed, "skipped": 2, "cancelled": cancelled,
                "failedNodes": if failed > 0 { json!(["compose"]) } else { json!([]) }}
    })
}

#[test]
fn summaries_report_the_run_without_messages() {
    let mut agent = agent(delivered(true, true), run(1, 0));
    let (message, outputs) = ok(agent.host.invoke(
        &plugin(),
        "workflow-summary",
        json!({"details": "Branch: main"}),
    ));
    assert_eq!(message, "showed “Start work”");
    let summary = "3 succeeded, 1 failed (compose), 2 skipped · 12.3 s";
    assert_eq!(
        data(&outputs),
        json!({"shown": true, "desktop": true, "statusBar": true, "summary": summary})
    );
    assert_eq!(
        agent.sent.borrow()[0],
        (
            "Start work".to_owned(),
            format!("{summary}\nBranch: main"),
            Some(NotifySeverity::Error)
        )
    );

    // Severity follows the run; a title replaces the workflow's name.
    let mut cancelled = agent_with(run(0, 1));
    ok(cancelled
        .host
        .invoke(&plugin(), "workflow-summary", json!({"title": "  Done "})));
    assert_eq!(cancelled.sent.borrow()[0].0, "Done", "the title is trimmed");
    assert_eq!(cancelled.sent.borrow()[0].2, Some(NotifySeverity::Warning));
    let mut clean = agent_with(run(0, 0));
    ok(clean.host.invoke(&plugin(), "workflow-summary", json!({})));
    assert_eq!(clean.sent.borrow()[0].2, Some(NotifySeverity::Success));

    // Outside a workflow: no summary, info, "Workflow".
    let mut direct = agent_with(json!({"trigger": "key"}));
    let (_, outputs) = ok(direct.host.invoke(&plugin(), "workflow-summary", json!({})));
    assert_eq!(data(&outputs)["summary"], "");
    assert_eq!(
        direct.sent.borrow()[0],
        (
            "Workflow".to_owned(),
            String::new(),
            Some(NotifySeverity::Info)
        )
    );

    // Only on failure: a clean run shows nothing.
    let mut quiet = agent_with(run(0, 0));
    let (message, outputs) = ok(quiet.host.invoke(
        &plugin(),
        "workflow-summary",
        json!({"onlyOnFailure": true}),
    ));
    assert_eq!(message, "nothing failed, so no summary was shown");
    assert_eq!(
        data(&outputs),
        json!({"shown": false, "desktop": false, "statusBar": false,
               "reason": "nothing failed", "summary": "3 succeeded, 2 skipped · 12.3 s"})
    );
    assert!(quiet.sent.borrow().is_empty());
}

fn agent_with(run: Value) -> Agent {
    agent(delivered(true, true), run)
}

#[test]
fn text_is_made_plain_like_the_core_does() {
    assert_eq!(plain(" a\u{202e}b\tc\u{7} ", 100, false), "ab c");
    assert_eq!(plain("one\ntwo", 100, false), "onetwo");
    assert_eq!(plain("one\ntwo", 100, true), "one\ntwo");
    assert_eq!(plain("abcdef", 4, false), "abc…");
    assert_eq!(severity("error"), NotifySeverity::Error);
    assert_eq!(severity("critical"), NotifySeverity::Info);
    assert_eq!(
        describe(&RunSummary {
            elapsed_ms: 75_000,
            succeeded: 1,
            ..RunSummary::default()
        }),
        "1 succeeded · 1 min 15 s"
    );
}

// The move from DeckPilot (official/README.md). DeckPilot shipped 0.1.0
// and 0.1.1 built from its own plugins/first-party/notifications; this copy
// is built against the SDK as a git dependency, so its module differs from
// DeckPilot's for the same source. A copy with a shipped version but other
// bytes fails the agent's bundled pins and shows "Broken" (2026-11-20), so
// the version here must be above every shipped one, and nothing else of the
// contract (id, actions, settings, permission) may change.

/// DeckPilot's manifest.json of 0.1.1 at the pinned commit
/// (tools/deckpilot.env), byte for byte.
const DECKPILOT_MANIFEST: &[u8] = include_bytes!("../tests/deckpilot-0.1.1.manifest.json");
/// Its sha256 and size in DeckPilot's plugins/first-party/released-pins.json
/// (deckpilot.notifications 0.1.1), so the snapshot is the shipped file.
const DECKPILOT_MANIFEST_SHA256: &str =
    "2254736ed73042e4ba5424648465fe616f63475f22084d1a59936c17f29ad663";
const DECKPILOT_MANIFEST_SIZE: usize = 7039;
/// Every version of deckpilot.notifications in DeckPilot's
/// released-pins.json at the pinned commit. A newer DeckPilot pin that
/// shipped another one adds it here (and tools/official.sh sign-test runs
/// DeckPilot's released-pins guard on the package in any case).
const SHIPPED_VERSIONS: [&str; 2] = ["0.1.0", "0.1.1"];

fn version_key(version: &str) -> (u64, u64, u64) {
    let parts: Vec<u64> = version.split('.').map(|p| p.parse().unwrap()).collect();
    assert_eq!(parts.len(), 3, "{version} is not major.minor.patch");
    (parts[0], parts[1], parts[2])
}

#[test]
fn the_snapshot_is_the_manifest_deckpilot_shipped() {
    assert_eq!(DECKPILOT_MANIFEST.len(), DECKPILOT_MANIFEST_SIZE);
    assert_eq!(sha256_hex(DECKPILOT_MANIFEST), DECKPILOT_MANIFEST_SHA256);
}

#[test]
fn only_the_version_and_the_repository_differ_from_deckpilots() {
    let theirs: Value = serde_json::from_slice(DECKPILOT_MANIFEST).unwrap();
    let ours = manifest();
    // The contract, spelled out: the id, the action ids, the settings keys
    // and the permission (kind, routing, justification).
    assert_eq!(ours["id"], "deckpilot.notifications");
    assert_eq!(ours["id"], theirs["id"]);
    let ids = |m: &Value, list: &str| -> Vec<Value> {
        let at = if list == "actions" {
            "/contributes/actions"
        } else {
            "/contributes/settings/fields"
        };
        let field = if list == "actions" { "id" } else { "key" };
        m.pointer(at)
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a[field].clone())
            .collect()
    };
    assert_eq!(
        ids(&ours, "actions"),
        vec![json!("show"), json!("workflow-summary")]
    );
    assert_eq!(ids(&ours, "actions"), ids(&theirs, "actions"));
    assert_eq!(
        ids(&ours, "settings"),
        vec![
            json!("desktop"),
            json!("statusBar"),
            json!("minimumSeverity")
        ]
    );
    assert_eq!(ids(&ours, "settings"), ids(&theirs, "settings"));
    assert_eq!(ours["permissions"], theirs["permissions"]);
    // And everything else, field for field.
    let rest = |m: &Value| {
        let mut m = m.clone();
        let object = m.as_object_mut().unwrap();
        object.remove("version");
        object.remove("repository");
        m
    };
    assert_eq!(rest(&ours), rest(&theirs));
    assert_eq!(
        ours["repository"],
        json!({"type": "git", "url": "https://github.com/knittingcode/deckpilot-plugins",
               "directory": "official/notifications"})
    );
}

#[test]
fn the_version_is_above_every_version_deckpilot_shipped() {
    let ours = manifest()["version"].as_str().unwrap().to_owned();
    assert_eq!(ours, VERSION, "manifest.json and Cargo.toml disagree");
    let theirs: Value = serde_json::from_slice(DECKPILOT_MANIFEST).unwrap();
    assert!(SHIPPED_VERSIONS.contains(&theirs["version"].as_str().unwrap()));
    for shipped in SHIPPED_VERSIONS {
        assert!(
            version_key(&ours) > version_key(shipped),
            "{ours} is not above the shipped {shipped}: a shipped version with other bytes shows Broken"
        );
    }
}

/// FIPS 180-4 SHA-256 (no dependency for one test).
fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[test]
fn sha256_is_sha256() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
