//! The clock QuickJS reads (`Date.now()`, `new Date()`), without an
//! import of its own.
//!
//! rquickjs-sys's `wasm32-unknown-unknown` shim (`wasm-shim/shim.c`) takes
//! the time from an `env.__rquickjs_host_now_us` import. A module may
//! import only `deckpilot_v1.host_call` (guest ABI 1), so the symbol is
//! defined here and the linker resolves it inside the module: the import
//! disappears (`tools/check-wasm-imports.py` holds the build to that).
//!
//! It is not a host call per read: a script that reads the clock in a
//! loop would spend its bridge budget. The time is the agent's, as
//! `script.load` gave it, moved forward by every later reply that carries
//! `nowUs` (the agent adds it to `script.poll`, `script.sleep` and
//! `process.run` replies), and never backwards.

use std::cell::Cell;

thread_local! {
    static NOW_US: Cell<f64> = const { Cell::new(0.0) };
}

/// Moves the clock to `us` (µs since the Unix epoch) unless it is earlier.
pub fn set(us: f64) {
    if us.is_finite() {
        NOW_US.with(|n| n.set(n.get().max(us)));
    }
}

#[cfg_attr(not(any(test, target_arch = "wasm32")), allow(dead_code))]
pub fn now_us() -> f64 {
    NOW_US.with(Cell::get)
}

/// The definition of the shim's clock import.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn __rquickjs_host_now_us() -> f64 {
    now_us()
}
