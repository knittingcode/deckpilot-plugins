//! The actions that change something on the provider
//! (`create-change-request`, `trigger-pipeline`): the write gate, the
//! confirmation (design §5) and then the write, in that order, so that
//! nothing is sent unless the connection allows changes and the user (or
//! the instance's `confirm: false`) agreed.
//!
//! From a key or a gesture they ask first, unless `confirm` is off; a
//! workflow the user started is the intent; a schedule, an event or an
//! unknown run fails with `needs-confirmation` unless `confirm` is off.

use deckpilot_plugin_sdk::dialog::{Dialog, DialogResultExt};
use deckpilot_plugin_sdk::prelude::*;

use crate::client::Api;
use crate::errors::codes;
use crate::ops;
use crate::provider::{ConnectionInfo, CrCreate, PipelineTrigger, Provider, ProviderKind, RepoRef};
use crate::transport::Transport;

/// The run context and dialogs come from `ctx`.
pub fn confirmed(ctx: &mut Context<'_>, confirm: bool, question: &str) -> Result<(), PluginError> {
    if !confirm {
        return Ok(());
    }
    let run = ctx.run_context()?;
    if !run.is_user_initiated() {
        return Err(PluginError::new(
            codes::NEEDS_CONFIRMATION,
            "this action changes something and asks first; a run nobody started cannot ask, so \
             turn its “Ask first” off to let it run unattended",
        ));
    }
    if !matches!(run.trigger, RunTrigger::Key | RunTrigger::Gesture) {
        return Ok(());
    }
    let answer = ctx.dialog(
        Dialog::confirm("Git Providers")
            .message(question.chars().take(400).collect::<String>())
            .confirm_label("Continue"),
    )?;
    if answer.cancel_reason().is_none() {
        Ok(())
    } else {
        Err(PluginError::new(codes::CANCELLED, "cancelled"))
    }
}

/// A change request to open, as the action's parameters give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrRequest {
    pub source: String,
    /// `None`: the repository's default branch.
    pub target: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub draft: bool,
    pub open_after: bool,
}

pub fn create_change_request(
    ctx: &mut Context<'_>,
    p: &dyn Provider,
    t: &mut dyn Transport,
    connection: &ConnectionInfo,
    r: &RepoRef,
    confirm: bool,
    request: CrRequest,
) -> ActionResult {
    connection.require_write()?;
    let host = Api::new(connection)?.host;
    let repo = p.get_repository(t, r)?;
    let target = match request.target {
        Some(target) => target,
        None => repo
            .default_branch
            .clone()
            .ok_or_else(|| PluginError::new(codes::INVALID_REF, "choose a target branch"))?,
    };
    let word = if p.kind() == ProviderKind::Gitlab {
        "merge request"
    } else {
        "pull request"
    };
    confirmed(
        ctx,
        confirm,
        &format!(
            "Open a {word} from {} into {target} in {} on {host}?",
            request.source, repo.path
        ),
    )?;
    let c = CrCreate {
        source: request.source,
        target,
        title: request.title,
        description: request.description,
        draft: request.draft,
    };
    let output = ops::create_change_request(p, t, connection, r, &c)?;
    // The change already happened: a page that cannot be opened is told,
    // never a failure of the action.
    if request.open_after
        && let Some(deckpilot_plugin_sdk::ParamValue::String(url)) = output.data().get("url")
    {
        let opened = !url.is_empty() && ctx.open_url(url.clone()).is_ok();
        return Ok(output.with("opened", opened));
    }
    Ok(output)
}

pub fn trigger_pipeline(
    ctx: &mut Context<'_>,
    p: &dyn Provider,
    t: &mut dyn Transport,
    connection: &ConnectionInfo,
    r: &RepoRef,
    confirm: bool,
    trigger: &PipelineTrigger,
) -> ActionResult {
    connection.require_write()?;
    // Everything that would refuse the trigger (variables on GitHub, a bad
    // name or value, a ref with control characters) is refused before the
    // user is asked, and before any request.
    p.check_trigger(trigger)?;
    let host = Api::new(connection)?.host;
    let path = p.get_repository(t, r)?.path;
    confirmed(ctx, confirm, &trigger_question(trigger, &path, &host))?;
    Ok(ops::trigger_pipeline(p, t, connection, r, trigger)?)
}

/// Longest confirmation question (the dialog's message bound).
pub const MAX_QUESTION: usize = 400;

/// What starting the pipeline does, as the confirmation asks it: the ref,
/// the repository and server, the GitHub workflow file, and the names of
/// the GitLab variables (never their values). At most [`MAX_QUESTION`]
/// characters: names that do not fit are counted.
pub fn trigger_question(trigger: &PipelineTrigger, path: &str, host: &str) -> String {
    let mut question = format!(
        "Start a pipeline on {} in {path} ({host})",
        crate::errors::clean(&trigger.reference, 80)
    );
    if let Some(workflow) = &trigger.workflow {
        question.push_str(&format!(
            " with the workflow {}",
            crate::errors::clean(workflow, 100)
        ));
    }
    if !trigger.variables.is_empty() {
        question.push_str(" with the variables ");
        // Room for the names, the ", and N more" and the question mark.
        let room = MAX_QUESTION.saturating_sub(question.chars().count() + 20);
        let mut names = String::new();
        let mut shown = 0;
        for (name, _) in &trigger.variables {
            let next = if shown == 0 {
                name.clone()
            } else {
                format!(", {name}")
            };
            if names.chars().count() + next.chars().count() > room {
                break;
            }
            names.push_str(&next);
            shown += 1;
        }
        question.push_str(&names);
        let more = trigger.variables.len() - shown;
        if more > 0 {
            question.push_str(&format!(" and {more} more"));
        }
    }
    question.push('?');
    question.chars().take(MAX_QUESTION).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use deckpilot_plugin_sdk::testing::MockHost;
    use deckpilot_plugin_sdk::{CallResult, CapabilityRequest};
    use serde_json::{Value, json};

    use super::*;
    use crate::adapters::gitlab::Gitlab;
    use crate::provider::{ConnectionStatus, Deployment};
    use crate::transport::Method;
    use crate::transport::mock::MockTransport;

    const ID: &str = "cn_01JD7Q0000000000000000000A";
    const API: &str = "https://gitlab.com/api/v4";

    pub(crate) fn writable() -> ConnectionInfo {
        ConnectionInfo {
            id: ID.into(),
            type_id: "gitlab".into(),
            deployment: Deployment::Cloud,
            api_base: API.into(),
            web_base: "https://gitlab.com".into(),
            methods: vec![Method::Get, Method::Post, Method::Put],
            status: ConnectionStatus::Ok,
            capabilities: None,
        }
    }

    /// A host whose run is `run` and whose dialogs answer `dialog`; the
    /// other calls are allowed.
    pub(crate) fn mock_host(run: Value, dialog: Value) -> MockHost {
        MockHost::new(move |req| match req {
            CapabilityRequest::RunContext(_) => CallResult::Ok(run.clone()),
            CapabilityRequest::DialogOpen(_) => CallResult::Ok(dialog.clone()),
            _ => CallResult::Ok(Value::Null),
        })
    }

    pub(crate) fn key() -> Value {
        json!({"trigger": "key"})
    }

    pub(crate) fn schedule() -> Value {
        json!({"trigger": "workflow", "cause": "schedule"})
    }

    pub(crate) fn declined() -> Value {
        json!({"outcome": "cancelled", "reason": "user"})
    }

    pub(crate) fn accepted() -> Value {
        json!({"outcome": "confirmed"})
    }

    fn transport() -> MockTransport {
        MockTransport::new()
            .on(
                Method::Get,
                &format!("{API}/projects/4711"),
                200,
                r#"{"id":4711,"path_with_namespace":"team/app","default_branch":"main","web_url":"https://gitlab.com/team/app"}"#,
            )
            .on(Method::Get, &format!("{API}/projects/4711/merge_requests?state=opened&source_branch=fix&target_branch=main&per_page=1"), 200, "[]")
            .on(
                Method::Post,
                &format!("{API}/projects/4711/merge_requests"),
                201,
                r#"{"iid":1,"title":"Fix","state":"opened","source_branch":"fix","target_branch":"main","web_url":"https://gitlab.com/team/app/-/merge_requests/1"}"#,
            )
            .on(
                Method::Post,
                &format!("{API}/projects/4711/pipeline"),
                201,
                r#"{"id":5,"status":"created","ref":"main"}"#,
            )
    }

    fn request() -> CrRequest {
        CrRequest {
            source: "fix".into(),
            target: None,
            title: "Fix".into(),
            description: None,
            draft: false,
            open_after: false,
        }
    }

    fn repo() -> RepoRef {
        RepoRef::parse("gl:4711", ProviderKind::Gitlab, ID).unwrap()
    }

    fn trigger() -> PipelineTrigger {
        PipelineTrigger {
            reference: "main".into(),
            variables: Vec::new(),
            workflow: None,
            inputs: Default::default(),
        }
    }

    fn posts(t: &MockTransport) -> usize {
        t.requests
            .iter()
            .filter(|r| r.method != Method::Get)
            .count()
    }

    type Change =
        fn(&mut Context<'_>, &Gitlab, &mut MockTransport, &ConnectionInfo, bool) -> ActionResult;

    fn both() -> [(&'static str, Change); 2] {
        [
            ("create-change-request", |ctx, g, t, c, confirm| {
                create_change_request(ctx, g, t, c, &repo(), confirm, request())
            }),
            ("trigger-pipeline", |ctx, g, t, c, confirm| {
                trigger_pipeline(ctx, g, t, c, &repo(), confirm, &trigger())
            }),
        ]
    }

    #[test]
    fn a_declined_dialog_sends_nothing() {
        let c = writable();
        let g = Gitlab::new(&c).unwrap();
        for (name, change) in both() {
            let mut host = mock_host(key(), declined());
            let mut t = transport();
            let e = change(&mut host.context(), &g, &mut t, &c, true).unwrap_err();
            assert_eq!(e.code(), codes::CANCELLED, "{name}");
            assert_eq!(posts(&t), 0, "{name}");
            // The question named the repository.
            assert!(host.requests().iter().any(|r| matches!(r,
                CapabilityRequest::DialogOpen(d) if serde_json::to_string(d).unwrap().contains("team/app"))), "{name}");
        }
    }

    #[test]
    fn an_unattended_run_needs_confirmation_and_confirm_false_runs() {
        let c = writable();
        let g = Gitlab::new(&c).unwrap();
        for (name, change) in both() {
            let mut host = mock_host(schedule(), accepted());
            let mut t = transport();
            let e = change(&mut host.context(), &g, &mut t, &c, true).unwrap_err();
            assert_eq!(e.code(), codes::NEEDS_CONFIRMATION, "{name}");
            assert_eq!(posts(&t), 0, "{name}");

            // confirm=false: runs without asking, also unattended.
            let mut host = mock_host(schedule(), declined());
            let mut t = transport();
            change(&mut host.context(), &g, &mut t, &c, false).unwrap();
            assert_eq!(posts(&t), 1, "{name}");
            assert!(
                !host
                    .requests()
                    .iter()
                    .any(|r| matches!(r, CapabilityRequest::DialogOpen(_)))
            );

            // Accepted from a key: one write.
            let mut host = mock_host(key(), accepted());
            let mut t = transport();
            change(&mut host.context(), &g, &mut t, &c, true).unwrap();
            assert_eq!(posts(&t), 1, "{name}");

            // A read-only connection: nothing at all, not even a question.
            let read_only = ConnectionInfo {
                methods: vec![Method::Get],
                ..c.clone()
            };
            let mut host = mock_host(key(), accepted());
            let mut t = transport();
            let e = change(&mut host.context(), &g, &mut t, &read_only, true).unwrap_err();
            assert_eq!(e.code(), codes::READ_ONLY_CONNECTION, "{name}");
            assert!(t.requests.is_empty());
            assert!(host.requests().is_empty());
        }
    }

    #[test]
    fn a_page_that_cannot_open_never_fails_the_change() {
        let c = writable();
        let g = Gitlab::new(&c).unwrap();
        let mut host = MockHost::new(|req| match req {
            CapabilityRequest::RunContext(_) => CallResult::Ok(key()),
            CapabilityRequest::DialogOpen(_) => CallResult::Ok(accepted()),
            CapabilityRequest::OpenUrl(_) => CallResult::error(
                deckpilot_plugin_sdk::CallErrorCode::NotPermitted,
                "not allowed",
            ),
            _ => CallResult::Ok(Value::Null),
        });
        let mut t = transport();
        let out = create_change_request(
            &mut host.context(),
            &g,
            &mut t,
            &c,
            &repo(),
            true,
            CrRequest {
                open_after: true,
                ..request()
            },
        )
        .unwrap();
        assert_eq!(
            out.data().get("opened"),
            Some(&deckpilot_plugin_sdk::ParamValue::Bool(false))
        );
    }
}
