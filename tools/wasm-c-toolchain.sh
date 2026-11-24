#!/usr/bin/env bash
# wasm-c-toolchain.sh [--install]   The C toolchain that compiles QuickJS
# into deckpilot.scripts's wasm module (rquickjs-sys, official/scripts).
#
# * clang $WASM_CLANG_MAJOR (tools/official.sh) with the wasm32 backend:
#   --install takes `clang-<major>` from Ubuntu's own signed archive (CI's
#   ubuntu-24.04 runners); the two independent builds (and later the two
#   release builds) get the same compiler. A local build may use another
#   clang through DECKPILOT_WASM_CC; its modules are then not CI's bytes.
# * llvm-ar: the pinned Rust toolchain's own (component llvm-tools, the
#   rust-toolchain.toml channel), so it moves only with that pin.
#
# Without --install it only checks, and prints both versions (official.sh's
# build checks the same before every build).
. "$(dirname "$0")/lib/common.sh"
WASM_CLANG_MAJOR=$(sed -n 's/^WASM_CLANG_MAJOR=\([0-9]*\)$/\1/p' "$REPO_ROOT/tools/official.sh")
[ -n "$WASM_CLANG_MAJOR" ] || die "no WASM_CLANG_MAJOR in tools/official.sh"

if [ "${1:-}" = --install ]; then
  command -v apt-get >/dev/null || die "--install is for CI's Ubuntu runners"
  sudo apt-get update -q
  sudo apt-get install -y -q --no-install-recommends "clang-$WASM_CLANG_MAJOR"
  (cd "$REPO_ROOT" && rustup component add llvm-tools)
fi

cc=${DECKPILOT_WASM_CC:-clang-$WASM_CLANG_MAJOR}
command -v "$cc" >/dev/null || die "no $cc (tools/wasm-c-toolchain.sh --install)"
"$cc" --version | head -1
sysroot=$(cd "$REPO_ROOT" && rustc --print sysroot)
host=$(cd "$REPO_ROOT" && rustc -vV | sed -n 's/^host: //p')
ar=$sysroot/lib/rustlib/$host/bin/llvm-ar
[ -x "$ar" ] || die "no llvm-ar in the pinned Rust toolchain (rustup component add llvm-tools)"
"$ar" --version | head -2 | tr '\n' ' '
echo
