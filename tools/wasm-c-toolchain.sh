#!/usr/bin/env bash
# wasm-c-toolchain.sh [--install]   The C toolchain that compiles QuickJS
# into deckpilot.scripts's wasm module (rquickjs-sys, official/scripts).
#
# * clang, exactly WASM_CLANG_APT_VERSION (tools/official.sh) with the
#   wasm32 backend: --install takes it from Ubuntu's own signed archive
#   (CI's ubuntu-24.04 runners), and its --version must be
#   WASM_CLANG_VERSION_LINE; the two independent builds (and later the two
#   release builds) get the same compiler. A local build may use another
#   clang through DECKPILOT_WASM_CC, never under CI; its modules are then
#   not CI's bytes.
# * llvm-ar: the pinned Rust toolchain's own (component llvm-tools, the
#   rust-toolchain.toml channel), so it moves only with that pin.
#
# Without --install it only checks, and prints both versions (official.sh's
# build checks the same before every build).
. "$(dirname "$0")/lib/common.sh"
pin() { sed -n "s/^$1=\"\{0,1\}\([^\"]*\)\"\{0,1\}\$/\1/p" "$REPO_ROOT/tools/official.sh"; }
WASM_CLANG_MAJOR=$(pin WASM_CLANG_MAJOR)
WASM_CLANG_APT_VERSION=$(pin WASM_CLANG_APT_VERSION)
WASM_CLANG_VERSION_LINE=$(pin WASM_CLANG_VERSION_LINE)
[ -n "$WASM_CLANG_MAJOR" ] && [ -n "$WASM_CLANG_APT_VERSION" ] && [ -n "$WASM_CLANG_VERSION_LINE" ] ||
  die "no clang pin (WASM_CLANG_MAJOR, WASM_CLANG_APT_VERSION, WASM_CLANG_VERSION_LINE) in tools/official.sh"

if [ "${1:-}" = --install ]; then
  command -v apt-get >/dev/null || die "--install is for CI's Ubuntu runners"
  sudo apt-get update -q
  # The exact package version; its dependencies on libclang and llvm are
  # exact versions too.
  sudo apt-get install -y -q --no-install-recommends "clang-$WASM_CLANG_MAJOR=$WASM_CLANG_APT_VERSION"
  (cd "$REPO_ROOT" && rustup component add llvm-tools)
fi

[ -z "${DECKPILOT_WASM_CC:-}" ] || [ -z "${CI:-}" ] || die "DECKPILOT_WASM_CC is for local builds only"
cc=${DECKPILOT_WASM_CC:-clang-$WASM_CLANG_MAJOR}
command -v "$cc" >/dev/null || die "no $cc (tools/wasm-c-toolchain.sh --install)"
line=$("$cc" --version | head -1)
echo "$line"
[ -n "${DECKPILOT_WASM_CC:-}" ] || [ "$line" = "$WASM_CLANG_VERSION_LINE" ] ||
  die "$cc is not $WASM_CLANG_VERSION_LINE"
sysroot=$(cd "$REPO_ROOT" && rustc --print sysroot)
host=$(cd "$REPO_ROOT" && rustc -vV | sed -n 's/^host: //p')
ar=$sysroot/lib/rustlib/$host/bin/llvm-ar
[ -x "$ar" ] || die "no llvm-ar in the pinned Rust toolchain (rustup component add llvm-tools)"
"$ar" --version | head -2 | tr '\n' ' '
echo
