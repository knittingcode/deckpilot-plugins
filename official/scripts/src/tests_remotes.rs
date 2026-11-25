//! `Repository.remotes` (decision K4, deckpilot #491): the script objects
//! of a reply with remotes and of one from an agent before them, the
//! host's limits (8 remotes, one primary), malformed replies, frozen
//! objects, and the K4 isolation: no text the guest builds (log lines,
//! errors, the run's outcomes and result, every request it sends) carries
//! a `connectionId` or a `remoteRepositoryId`.

use serde_json::{Value, json};

use crate::mock::{Mock, exit, ok, refuse, repository};
use crate::tests::{failed, result, run_mock};

const CONN: &str = "conn-7f3a9e5d";
const RID: &str = "gh:987654321";

fn remote(name: &str, primary: bool, url: Value, branch: Value, conn: Value, rid: Value) -> Value {
    json!({ "name": name, "primary": primary, "url": url, "defaultBranch": branch,
            "connectionId": conn, "remoteRepositoryId": rid })
}

/// The primary, linked; a mirror with every text member null.
fn two_remotes() -> Value {
    json!([
        remote(
            "origin",
            true,
            json!("git@github.com:example/cms.git"),
            json!("main"),
            json!(CONN),
            json!(RID)
        ),
        remote(
            "mirror",
            false,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null
        ),
    ])
}

/// `cms` with `remotes` (absent when `None`) and a `remote`, and `web`
/// without either, as deckpilot #491 sends them; `process.run` fails in
/// `cms`.
fn with_remotes(source: &str, remotes: Option<Value>) -> Mock {
    Mock::with(
        source,
        json!({ "projectsRead": true }),
        Box::new(move |op, args| match op {
            "script.log" => ok(json!(null)),
            "project.list" => {
                ok(json!({ "projects": [{ "id": "p1", "key": "shop", "name": "Shop" }] }))
            }
            "project.repositories" => {
                let mut cms = repository("cms", true);
                cms["remote"] =
                    json!({ "url": "git@github.com:example/cms.git", "defaultBranch": "main" });
                if let Some(r) = &remotes {
                    cms["remotes"] = r.clone();
                }
                ok(json!({
                    "project": { "id": "p1", "key": "shop", "name": "Shop" },
                    "repositories": [cms, repository("web", false)],
                }))
            }
            "process.run" if args["cwd"]["repository"] == "cms" => {
                exit(1, "", "fatal: Not possible to fast-forward, aborting.\n")
            }
            "process.run" => exit(0, "", ""),
            other => refuse("unsupported-op", "unsupported", other),
        }),
    )
}

const DESCRIBE: &str = r#"
    const all = deckpilot.repositories.current();
    const of = (r) => r.remotes.map(m => ({ keys: Object.keys(m), name: m.name, primary: m.primary, url: m.url,
        defaultBranch: m.defaultBranch, connectionId: m.connectionId, remoteRepositoryId: m.remoteRepositoryId, token: m.token }));
    ({ cms: of(all[0]), web: of(all[1]), remote: all[0].remote, isArray: Array.isArray(all[0].remotes),
       listed: deckpilot.projects.list()[0].repositories()[0].remotes.length })
"#;

const KEYS: [&str; 6] = [
    "name",
    "primary",
    "url",
    "defaultBranch",
    "connectionId",
    "remoteRepositoryId",
];

#[test]
fn a_reply_with_remotes_gives_each_repository_its_remotes() {
    let mut remotes = two_remotes();
    // A member the guest does not know is dropped, never exposed.
    remotes[0]["token"] = json!("secret");
    let got = result(run_mock(&with_remotes(DESCRIBE, Some(remotes))));
    assert_eq!(
        got,
        json!({
            "cms": [
                { "keys": KEYS, "name": "origin", "primary": true, "url": "git@github.com:example/cms.git",
                  "defaultBranch": "main", "connectionId": CONN, "remoteRepositoryId": RID },
                { "keys": KEYS, "name": "mirror", "primary": false, "url": null,
                  "defaultBranch": null, "connectionId": null, "remoteRepositoryId": null },
            ],
            "web": [],
            // `remote` exactly as before.
            "remote": { "url": "git@github.com:example/cms.git", "defaultBranch": "main" },
            "isArray": true,
            "listed": 2,
        })
    );
    // Absent text members are null too.
    let sparse = json!([{ "name": "origin", "primary": true }]);
    let got = result(run_mock(&with_remotes(DESCRIBE, Some(sparse))));
    assert_eq!(
        got["cms"],
        json!([{ "keys": KEYS, "name": "origin", "primary": true, "url": null,
                 "defaultBranch": null, "connectionId": null, "remoteRepositoryId": null }])
    );
}

#[test]
fn an_agent_before_remotes_gives_none_and_keeps_remote() {
    for remotes in [None, Some(Value::Null), Some(json!([]))] {
        let got = result(run_mock(&with_remotes(DESCRIBE, remotes.clone())));
        assert_eq!(got["cms"], json!([]), "{remotes:?}");
        assert_eq!(got["web"], json!([]), "{remotes:?}");
        assert_eq!(got["isArray"], json!(true));
        // Not derived from `remote`, which stays as it was.
        assert_eq!(
            got["remote"],
            json!({ "url": "git@github.com:example/cms.git", "defaultBranch": "main" })
        );
    }
}

const REFUSED: &str = r#"
    try { deckpilot.repositories.current(); "accepted" }
    catch (e) { ({ code: e.code, repository: e.repository, message: e.message }) }
"#;

fn refused(remotes: Value) -> Value {
    let mock = with_remotes(REFUSED, Some(remotes));
    let got = result(run_mock(&mock));
    let text = got.to_string();
    assert!(!text.contains(CONN) && !text.contains(RID), "{text}");
    got
}

fn linked(name: &str, primary: bool) -> Value {
    remote(
        name,
        primary,
        json!("https://example.com/x.git"),
        json!("main"),
        json!(CONN),
        json!(RID),
    )
}

#[test]
fn more_than_eight_remotes_or_two_primaries_refuse_the_reply() {
    let eight: Vec<Value> = (0..8).map(|i| linked(&format!("r{i}"), i == 0)).collect();
    assert_eq!(
        result(run_mock(&with_remotes(DESCRIBE, Some(json!(eight)))))["cms"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    let nine: Vec<Value> = (0..9).map(|i| linked(&format!("r{i}"), i == 0)).collect();
    let got = refused(json!(nine));
    assert_eq!(got["code"], "invalidResponse");
    assert_eq!(got["repository"], "cms");
    assert_eq!(
        got["message"],
        "malformed reply to project.repositories: more than 8 remotes (repository \"cms\")"
    );
    let got = refused(json!([linked("origin", true), linked("upstream", true)]));
    assert_eq!(got["code"], "invalidResponse");
    assert_eq!(
        got["message"],
        "malformed reply to project.repositories: more than one primary remote (repository \"cms\")"
    );
    // None primary is the host's to allow (a repository whose remotes the
    // user has not ranked yet).
    let got = result(run_mock(&with_remotes(
        DESCRIBE,
        Some(json!([linked("a", false), linked("b", false)])),
    )));
    assert_eq!(got["cms"].as_array().unwrap().len(), 2);
}

#[test]
fn a_malformed_remote_refuses_the_reply_and_names_no_value() {
    let good = || linked("origin", true);
    let with = |k: &str, v: Value| {
        let mut m = good();
        m[k] = v;
        json!([m])
    };
    let without = |k: &str| {
        let mut m = good();
        m.as_object_mut().unwrap().remove(k);
        json!([m])
    };
    let cases = [
        (json!("origin"), "`remotes` is not an array"),
        (json!({ "origin": good() }), "`remotes` is not an array"),
        (json!([42]), "remote 1 is not an object"),
        (json!([good(), json!(RID)]), "remote 2 is not an object"),
        (without("name"), "remote 1 has no valid `name`"),
        (with("name", json!("")), "remote 1 has no valid `name`"),
        (
            with("name", json!("n".repeat(65))),
            "remote 1 has no valid `name`",
        ),
        (with("name", json!(7)), "remote 1 has no valid `name`"),
        (without("primary"), "remote 1: `primary` is not a boolean"),
        (
            with("primary", json!(RID)),
            "remote 1: `primary` is not a boolean",
        ),
        (
            with("url", json!(5)),
            "remote 1: `url` is not a string or null",
        ),
        (
            with("defaultBranch", json!(false)),
            "remote 1: `defaultBranch` is not a string or null",
        ),
        (
            with("connectionId", json!({ "id": CONN })),
            "remote 1: `connectionId` is not a string or null",
        ),
        (
            with("remoteRepositoryId", json!([RID])),
            "remote 1: `remoteRepositoryId` is not a string or null",
        ),
    ];
    for (remotes, why) in cases {
        let got = refused(remotes.clone());
        assert_eq!(got["code"], "invalidResponse", "{remotes}");
        assert_eq!(
            got["message"],
            json!(format!(
                "malformed reply to project.repositories: {why} (repository \"cms\")"
            )),
            "{remotes}"
        );
    }
    // Uncaught, it fails the run with that code.
    let (code, message) = failed(run_mock(&with_remotes(
        "deckpilot.repositories.current()",
        Some(with("primary", json!(CONN))),
    )));
    assert_eq!(code, "invalid-response");
    assert!(!message.contains(CONN), "{message}");
    // A name 64 characters long (not bytes) is a name.
    let got = result(run_mock(&with_remotes(
        DESCRIBE,
        Some(with("name", json!("ż".repeat(64)))),
    )));
    assert_eq!(got["cms"][0]["name"], json!("ż".repeat(64)));
}

#[test]
fn remotes_are_frozen() {
    let src = r#"
        "use strict";
        const repo = deckpilot.repositories.current()[0];
        const m = repo.remotes[0];
        const tries = {};
        const attempt = (k, f) => { try { f(); tries[k] = "changed" } catch (e) { tries[k] = e.name } };
        attempt("push", () => repo.remotes.push(m));
        attempt("replace", () => { repo.remotes = [] });
        attempt("url", () => { m.url = "https://evil.example/x.git" });
        attempt("connectionId", () => { m.connectionId = "other" });
        attempt("add", () => { m.extra = 1 });
        attempt("delete", () => { delete m.name });
        attempt("toJSON", () => { m.toJSON = () => m.connectionId });
        ({ tries, frozen: [Object.isFrozen(repo.remotes), Object.isFrozen(m), Object.isFrozen(repo.remotes[1])],
           url: m.url, connectionId: m.connectionId, count: repo.remotes.length,
           again: deckpilot.repositories.current()[0].remotes[0] !== m })
    "#;
    let got = result(run_mock(&with_remotes(src, Some(two_remotes()))));
    assert_eq!(
        got,
        json!({
            "tries": { "push": "TypeError", "replace": "TypeError", "url": "TypeError", "connectionId": "TypeError",
                       "add": "TypeError", "delete": "TypeError", "toJSON": "TypeError" },
            "frozen": [true, true, true],
            "url": "git@github.com:example/cms.git",
            "connectionId": CONN,
            "count": 2,
            // Each call makes new objects: nothing one caller holds is shared.
            "again": true,
        })
    );
}

/// Every text of a run the guest made: each request it sent (the log
/// lines among them), and the run's error, or its result and outcomes.
fn texts(mock: &Mock, r: crate::ActionResult) -> Vec<String> {
    let mut out: Vec<String> = mock
        .0
        .borrow()
        .raw
        .iter()
        .map(|r| String::from_utf8(r.clone()).unwrap())
        .collect();
    match r {
        Err(e) => out.push(format!("{}: {}", e.code(), e.message())),
        // The message, the result and the outcomes.
        Ok(o) => out.push(format!("{o:?}")),
    }
    out
}

fn no_ids(texts: &[String]) {
    assert!(texts.len() >= 3, "{texts:?}");
    for t in texts {
        assert!(!t.contains(CONN), "a connectionId in {t}");
        assert!(!t.contains(RID), "a remoteRepositoryId in {t}");
        assert!(!t.contains("987654321"), "a remoteRepositoryId in {t}");
    }
}

#[test]
fn no_text_the_guest_builds_carries_the_ids_k4() {
    // Everything that turns a repository or a remote into text: log lines
    // of the objects, `${repo}`, JSON, errors naming the repository (the
    // guest's, a thrown one, a failed process), the outcomes, and the
    // result. The script reads the ids (they are its own) but never logs
    // them itself.
    let src = r#"
        const repos = deckpilot.repositories.current();
        const cms = repos[0];
        const m = cms.remotes[0];
        if (m.connectionId.length === 0 || m.remoteRepositoryId.length === 0) throw new Error("no ids");
        deckpilot.log.info(cms);
        deckpilot.log.info(cms.remotes, m);
        deckpilot.log.warn("in", `${cms}`, String(cms), JSON.stringify(cms), JSON.stringify(m));
        console.log({ repos, first: m });
        console.error(repos);
        deckpilot.log.debug(deckpilot.projects.current().repositories());
        const caught = [];
        try { cms.join("..", "..") } catch (e) { caught.push(e.message) }
        try { deckpilot.fs.readFile(cms.join("x")) } catch (e) { caught.push(e.message) }
        caught.push(ScriptError.fromResult(deckpilot.git.run(["pull", "--ff-only"], { cwd: cms }), cms).message);
        deckpilot.log.error(caught);
        const outcomes = deckpilot.forEachRepository(r => {
            deckpilot.log.info(`Updating ${r.name}`, r.remotes);
            if (r.key === "web") throw new ScriptError("limit", `stopped in ${r}`);
            return deckpilot.git.run(["pull", "--ff-only"], { cwd: r });
        }, { stopOnError: false });
        deckpilot.log.info(outcomes);
        ({ cms, remotes: cms.remotes, outcomes })
    "#;
    let mock = with_remotes(src, Some(two_remotes()));
    let r = run_mock(&mock);
    let all = texts(&mock, r);
    let joined = all.join("\n");
    // The run touched the remotes and named the repository.
    assert!(
        joined.contains("origin") && joined.contains("mirror"),
        "{joined}"
    );
    assert!(joined.contains("repository \"cms\""), "{joined}");
    assert!(joined.contains("repository-failed"), "{joined}");
    no_ids(&all);

    // A run that succeeds: its result holds the repository and its
    // remotes, without the ids.
    let src = r#"
        const cms = deckpilot.repositories.current()[0];
        deckpilot.log.info(cms.remotes);
        ({ cms, remotes: cms.remotes, json: JSON.parse(JSON.stringify(cms.remotes)) })
    "#;
    let mock = with_remotes(src, Some(two_remotes()));
    let r = run_mock(&mock);
    let all = texts(&mock, r);
    no_ids(&all);
    let got = result(run_mock(&with_remotes(src, Some(two_remotes()))));
    assert_eq!(
        got["json"],
        json!([
            { "name": "origin", "primary": true, "url": "git@github.com:example/cms.git", "defaultBranch": "main" },
            { "name": "mirror", "primary": false, "url": null, "defaultBranch": null },
        ])
    );
    assert_eq!(got["cms"]["remotes"], got["json"]);

    // A refused reply names neither.
    let mock = with_remotes(
        "deckpilot.repositories.current()",
        Some(json!([linked("origin", true), linked("upstream", true)])),
    );
    let r = run_mock(&mock);
    no_ids(&texts(&mock, r));

    // The script may log an id itself: that is its own text.
    let mock = with_remotes(
        "deckpilot.log.info(deckpilot.repositories.current()[0].remotes[0].connectionId)",
        Some(two_remotes()),
    );
    let _ = run_mock(&mock);
    assert_eq!(mock.0.borrow().logs(), vec![CONN.to_owned()]);
}
