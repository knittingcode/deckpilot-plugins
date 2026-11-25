//! The session alone: budgets, the request cap, stops and the remaining
//! time, against a transport that answers every op the same.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{Value, json};

use super::*;

/// Answers every request with `reply` and counts what it was sent.
struct Fixed {
    reply: Value,
    sent: Rc<RefCell<Vec<Value>>>,
}

fn session(reply: Value) -> (Session, Rc<RefCell<Vec<Value>>>) {
    let sent = Rc::new(RefCell::new(Vec::new()));
    let t = Fixed {
        reply,
        sent: Rc::clone(&sent),
    };
    (Session::new(Box::new(t)), sent)
}

impl Transport for Fixed {
    fn call(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        self.sent
            .borrow_mut()
            .push(serde_json::from_slice(request).unwrap());
        Some(serde_json::to_vec(&self.reply).unwrap())
    }
}

#[test]
fn the_call_budget_holds_exactly() {
    let (mut s, sent) = session(json!({ "ok": {} }));
    for _ in 0..limits::HOST_CALLS {
        s.call("fs.exists", json!({})).unwrap();
    }
    let f = s.call("fs.exists", json!({})).unwrap_err();
    assert_eq!(f.code, "limit");
    assert_eq!(sent.borrow().len() as u32, limits::HOST_CALLS);
    assert_eq!(s.budget.calls, limits::HOST_CALLS);
}

#[test]
fn an_op_of_several_calls_needs_them_all_up_front() {
    let (mut s, _) = session(json!({ "ok": {} }));
    s.budget.calls = limits::HOST_CALLS - 3;
    assert!(s.reserve(3).is_ok());
    assert_eq!(s.reserve(4).unwrap_err().code, "limit");
}

#[test]
fn the_poll_budget_holds_and_stops_the_run_as_a_limit() {
    let (mut s, sent) = session(json!({ "ok": { "cancelled": false, "remainingMs": 1000 } }));
    for _ in 0..limits::POLLS {
        assert!(!s.poll());
    }
    assert!(s.poll(), "the 8,193rd poll is not sent: the run stops");
    assert_eq!(sent.borrow().len() as u32, limits::POLLS);
    assert_eq!(s.budget.calls, 0, "polls are outside the call budget");
    assert!(s.budget.polls_spent);
    assert_eq!(s.stopped().code, "limit");
    // And every op from now on fails here.
    assert_eq!(s.call("fs.exists", json!({})).unwrap_err().code, "limit");
    assert_eq!(sent.borrow().len() as u32, limits::POLLS);
}

#[test]
fn after_a_stop_only_the_cleanup_of_an_open_handle_is_sent() {
    let (mut s, sent) = session(json!({ "ok": { "cancelled": true, "remainingMs": 500 } }));
    assert!(s.poll());
    assert_eq!(
        s.call("script.log", json!({})).unwrap_err().code,
        "cancelled"
    );
    assert_eq!(sent.borrow().len(), 1);
    let _ = s.cleanup("fs.write.abort", json!({ "handle": "h" }));
    assert_eq!(
        sent.borrow().last().unwrap(),
        &json!({ "op": "fs.write.abort", "args": { "handle": "h" } })
    );
}

#[test]
fn a_request_over_64_kib_never_leaves_the_guest() {
    let (mut s, sent) = session(json!({ "ok": {} }));
    let big = "x".repeat(limits::REQUEST);
    let f = s.call("process.run", json!({ "stdin": big })).unwrap_err();
    assert_eq!(f.code, "limit");
    assert!(sent.borrow().is_empty());
}

#[test]
fn a_reply_over_1_mib_is_refused() {
    let (mut s, _) = session(json!({ "ok": { "content": "y".repeat(limits::REPLY) } }));
    assert_eq!(
        s.call("fs.readFile", json!({})).unwrap_err().code,
        "hostError"
    );
}

#[test]
fn the_remaining_time_follows_the_agents_clock() {
    let (mut s, _) = session(json!({ "ok": { "nowUs": 1_000_000.0 } }));
    assert_eq!(s.budget.remaining_now(), None);
    s.budget.clock_us = 1_000_000.0;
    s.budget.set_remaining(5_000);
    assert_eq!(s.budget.remaining_now(), Some(5_000));
    // 1.5 s of agent time later.
    s.budget.clock_us = 2_500_000.0;
    assert_eq!(s.budget.remaining_now(), Some(3_500));
    s.budget.clock_us = 9_000_000.0;
    assert_eq!(s.budget.remaining_now(), Some(0));
}

#[test]
fn a_cancelled_reply_stops_the_session() {
    let (mut s, sent) = session(
        json!({ "error": { "code": "failed", "message": "stopped", "script": { "code": "cancelled" } } }),
    );
    assert_eq!(
        s.call("script.sleep", json!({ "ms": 10 }))
            .unwrap_err()
            .code,
        "cancelled"
    );
    assert!(s.budget.cancelled);
    assert_eq!(
        s.call("fs.exists", json!({})).unwrap_err().code,
        "cancelled"
    );
    assert_eq!(sent.borrow().len(), 1);
}
