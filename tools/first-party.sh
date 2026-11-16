#!/usr/bin/env bash
# first-party.sh [check|test|package|all]   (default: all)
#
# What first-party.yml runs, also locally. `package` needs tools/build-cli.sh
# first; `check` and `test` do not.
#
# check    tools/check-first-party.py: the layout of first-party/, the SDK
#          pin (tools/deckpilot.env) and the Polish texts of every plugin.
# test     In first-party/: `cargo test --locked` (native unit tests against
#          the SDK's MockHost) and, when the components are installed,
#          `cargo fmt --check` and `cargo clippy -D warnings`.
# package  For every plugin: manifest.json against the pinned schema; the
#          module built for wasm32-unknown-unknown with the `guest` profile;
#          `deckpilot-registry package` (manifest rules, size limits, the
#          static module check); then all packages in a registry signed with
#          THROWAWAY keys made here and accepted by the client's own rules
#          (`verify-site --require-packages`). Keys and output are scratch.
#
# The packages of the last step are made under the test publisher
# `deckpilot-test`, because the pinned tool refuses the first-party publisher
# `deckpilot` in a package (DeckPilot B6: first-party plugins install only
# from the application bundle today). Only the copy of manifest.json is
# changed (`id`, `publisher.id`); the module is the one built from the
# committed source. The script first checks that the unchanged manifest is
# refused for that reason, and fails once the tool accepts it, so the
# workaround cannot outlive B6 unnoticed; every other problem of the
# manifest is then reported by packaging the copy. Nothing made here is
# published or kept.
#
# Runs the plugins' build scripts and proc macros: never run it where a
# secret is available.
. "$(dirname "$0")/lib/common.sh"
what=${1:-all}

FP=$REPO_ROOT/first-party
OUT=$WORK/first-party
TEST_PUBLISHER=deckpilot-test
RESERVED_MESSAGE='publisher.id: `deckpilot` is reserved for DeckPilot'

list_plugins() {
  python3 -I "$REPO_ROOT/tools/check-first-party.py" --root "$FP" --commit "$DECKPILOT_COMMIT" "$@"
}

check() {
  note "first-party layout, SDK pin and Polish texts"
  list_plugins
}

run_tests() {
  note "cargo test --locked"
  (cd "$FP" && CARGO_TARGET_DIR=$OUT/target cargo test --locked)
  if (cd "$FP" && cargo fmt --version >/dev/null 2>&1); then
    note "cargo fmt --check"
    (cd "$FP" && cargo fmt --all --check)
  else
    note "rustfmt is not installed: skipped"
  fi
  if (cd "$FP" && cargo clippy --version >/dev/null 2>&1); then
    note "cargo clippy"
    (cd "$FP" && CARGO_TARGET_DIR=$OUT/target cargo clippy --locked --all-targets -- -D warnings)
  else
    note "clippy is not installed: skipped"
  fi
}

# A complete signed site in DIR/site/v1 under new keys (as tools/validate.sh).
throwaway_sign() {
  local dir=$1 keys commit
  commit=$(git -C "$REPO_ROOT" rev-parse HEAD)
  keys=$(mktemp -d "$OUT/keys.XXXXXX")
  for role in root index timestamp; do
    "$CLI" keys generate --role "$role" --out "$keys/$role.key" --no-password >/dev/null
  done
  "$CLI" keyset init --dir "$dir" --root "$keys/root.key.pub" \
    --index "$keys/index.key.pub" --timestamp "$keys/timestamp.key.pub"
  "$CLI" keyset sign --dir "$dir" --key "$keys/root.key"
  "$CLI" build-index --dir "$dir" --source-commit "$commit"
  "$CLI" sign-index --dir "$dir" --key "$keys/index.key"
  "$CLI" timestamp --dir "$dir" --key "$keys/timestamp.key"
  rm -rf "$keys"
  "$CLI" verify-site --dir "$dir" --require-packages --min-remaining 6d
}

package() {
  require_cli
  local plugins
  plugins=$(list_plugins --list)
  [ -n "$plugins" ] || die "first-party/ has no plugin"

  local packages=()
  local name id version crate
  while read -r name id version crate; do packages+=(-p "$crate"); done <<<"$plugins"
  note "cargo build --profile guest ($WASM_TARGET)"
  rustup target add "$WASM_TARGET" >/dev/null
  local cargo_home=${CARGO_HOME:-$HOME/.cargo}
  # Fixed path prefixes, so a module does not depend on where it was built.
  (cd "$FP" &&
    CARGO_TARGET_DIR=$OUT/target \
    RUSTFLAGS="--remap-path-prefix=$REPO_ROOT=/src --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$OUT/target=/target" \
    cargo build --locked --target "$WASM_TARGET" --profile guest "${packages[@]}")

  local reg=$OUT/registry commit
  commit=$(git -C "$REPO_ROOT" rev-parse HEAD)
  rm -rf "$reg" "$OUT/stage" "$OUT/packages"
  mkdir -p "$reg/registry/publishers" "$OUT/packages"
  jq -n --arg ns "$TEST_PUBLISHER" \
    '{namespace: $ns, github_id: 1, login: $ns, display_name: "DeckPilot (throwaway test publisher)", verified: false}' \
    >"$reg/registry/publishers/$TEST_PUBLISHER.json"
  summary "### First-party plugins (not published)" "" \
    "| Plugin | Version | Module sha256 | Module bytes | Package bytes | Permissions |" \
    "| --- | --- | --- | --- | --- | --- |"

  while read -r name id version crate; do
    note "$id $version"
    local dir=$FP/$name module=$OUT/target/$WASM_TARGET/guest/${crate//-/_}.wasm
    [ -f "$module" ] || die "the build produced no ${crate//-/_}.wasm"
    "$SCHEMA_CHECK" --schemas "$REPO_ROOT/schemas" \
      "https://knittingcode.github.io/deckpilot-plugins/v1/schemas/plugin-manifest.schema.json" \
      "$dir/manifest.json" >&2 || die "$dir/manifest.json does not match the manifest schema"

    # The unchanged manifest: refused, and only for the reserved publisher.
    local log=$OUT/$name.refusal.log
    if "$CLI" package --plugin-dir "$dir" --module "$module" \
      --out "$OUT/packages/$id-$version.dpplugin" >"$log" 2>&1; then
      die "the pinned tool now packages $id as it is: B6 changed. Remove the test publisher from tools/first-party.sh"
    fi
    grep -qF "$RESERVED_MESSAGE" "$log" || {
      cat "$log" >&2
      die "$id: the package is refused, and not for its reserved publisher (above)"
    }

    local test_id=$TEST_PUBLISHER.$name stage=$OUT/stage/$name
    mkdir -p "$stage"
    cp -R "$dir/." "$stage/"
    jq --arg id "$test_id" --arg ns "$TEST_PUBLISHER" '.id = $id | .publisher.id = $ns' \
      "$dir/manifest.json" >"$stage/manifest.json"
    local pkg=$OUT/packages/$test_id-$version.dpplugin
    "$CLI" package --plugin-dir "$stage" --module "$module" --out "$pkg"

    mkdir -p "$reg/registry/plugins/$test_id"
    jq '{id, name, summary: .description, description, license,
         source_repository: "knittingcode/deckpilot-plugins", categories: (.categories // ["other"])}' \
      "$stage/manifest.json" >"$reg/registry/plugins/$test_id/plugin.json"
    "$CLI" add-version --dir "$reg" --package "$pkg" \
      --plugin-detail "$reg/registry/plugins/$test_id/plugin.json" \
      --source-repository knittingcode/deckpilot-plugins --tag unpublished-first-party \
      --commit "$commit" --source-path "first-party/$name" --builder "$BUILDER_VERSION"
    summary "| \`$id\` | $version | \`$(shasum -a 256 "$module" | cut -d' ' -f1)\` | $(wc -c <"$module" | tr -d ' ') | $(wc -c <"$pkg" | tr -d ' ') | $(jq -c '[.permissions[] | .kind + (if .hosts then ":" + (.hosts | join(",")) else "" end)]' "$dir/manifest.json") |"
  done <<<"$plugins"

  note "signing a registry of the test packages with throwaway keys"
  throwaway_sign "$reg"
  summary "" "Packaged under the test publisher \`$TEST_PUBLISHER\` (the pinned tool refuses \`deckpilot\` in a package), signed with throwaway keys and accepted by \`verify-site --require-packages\`. Nothing is published."
}

mkdir -p "$OUT"
case $what in
  check) check ;;
  test) run_tests ;;
  package) package ;;
  all) check; run_tests; package ;;
  *) die "usage: first-party.sh [check|test|package|all]" ;;
esac
note "first-party: $what passed"
