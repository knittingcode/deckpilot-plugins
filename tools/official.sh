#!/usr/bin/env bash
# official.sh <command>   The pipeline of official/, the plugins DeckPilot
# bundles (official/README.md). What official.yml and official-release.yml
# run, also locally. Commands that need the pinned deckpilot-registry run
# tools/build-cli.sh first.
#
# check            tools/check-official.py: layout, SDK pin, permission
#                  policy, compatibility with the pinned DeckPilot, fixtures
#                  identical to DeckPilot's (when the pinned checkout exists).
# test             cargo test --locked, fmt --check, clippy -D warnings.
# package          per member: manifest schema, guest wasm build, `package
#                  --first-party` (manifest rules, the module check) into
#                  .work/official/packages.
# repro            two clean builds in fresh target directories; every
#                  module and package must be byte-identical.
# compare          DeckPilot's plugins/first-party/build.sh at the pinned
#                  commit, for every member: identical or not (informational:
#                  the SDK's git source is hashed into the build, so modules
#                  usually differ; the released-pins guard of sign-test is
#                  what forbids reusing a shipped version).
# sign-test        a THROWAWAY publisher key: release sign, then release
#                  verify against it (rollback floor: DeckPilot's
#                  released-pins.json; audit log), the released-pins guard
#                  on the verified files, and the pinned keys refusing it
#                  while no production key is pinned. Keys are scratch.
# all              check, test, package, repro, compare, sign-test.
#
# A `held` plugin (plugins.json; a plugin API 1.9 privileged kind while the
# gate is closed) has no package: `package --first-party` refuses its
# manifest by design (privileged kinds only in a bundled plugin's unpacked
# files), and package must see exactly that refusal; repro compares its
# modules; sign-test skips it. Nothing of it can be released.
#
# Release (official-release.yml only):
# resolve TAG      TAG is <id>--v<version> of a released plugin whose
#                  manifest has that version, at a commit on main.
# release-build ID clean double build of ID, compared; package; prints
#                  `sha256=` and `size=` for the job outputs.
# release-sign ID  with DECKPILOT_PUBLISHER_KEY (+ _PASSWORD) from the
#                  protected environment: release sign, then release verify
#                  with the keys pinned in the DeckPilot build (never --keys).
#
# Runs plugin build scripts: never run check..sign-test where a secret is.
. "$(dirname "$0")/lib/common.sh"
what=${1:-all}

OFF=$REPO_ROOT/official
OUT=$WORK/official
PACKAGES=$OUT/packages

list() {
  python3 -I "$REPO_ROOT/tools/check-official.py" --root "$OFF" --commit "$DECKPILOT_COMMIT" \
    --app-version "$DECKPILOT_VERSION" --plugin-api "$DECKPILOT_PLUGIN_API" "$@"
}

check() {
  note "official layout, SDK pin, permission policy, compatibility"
  if [ -d "$DECKPILOT_DIR/plugins/first-party" ]; then
    list --deckpilot "$DECKPILOT_DIR"
  else
    note "no pinned DeckPilot checkout ($DECKPILOT_DIR): fixtures not compared (tools/build-cli.sh fetches it)"
    list
  fi
}

run_tests() {
  note "cargo test --locked"
  (cd "$OFF" && CARGO_TARGET_DIR=$OUT/target cargo test --locked)
  note "cargo fmt --check"
  (cd "$OFF" && cargo fmt --all --check)
  note "cargo clippy"
  (cd "$OFF" && CARGO_TARGET_DIR=$OUT/target cargo clippy --locked --all-targets -- -D warnings)
}

sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
size() { wc -c <"$1" | tr -d ' '; }

# build TARGET_DIR [CRATE...]: the guest modules, reproducibly. The flags of
# DeckPilot's plugins/first-party/build.sh: no flags from the environment,
# official/ mapped to /deckpilot/plugins, Cargo's home to /cargo, and the
# pinned DeckPilot checkout Cargo made to the paths DeckPilot's own build
# records (runtime/..., first-party/...), so a module does not depend on
# where it was built.
build() {
  local target=$1; shift
  local crates=("$@")
  if [ ${#crates[@]} -eq 0 ]; then
    while read -r _ _ _ crate _; do crates+=("$crate"); done < <(list --list)
  fi
  rustup target add "$WASM_TARGET" >/dev/null 2>&1 || true
  local cargo_home=${CARGO_HOME:-$HOME/.cargo}
  (cd "$OFF" && cargo fetch --locked >/dev/null)
  local sdk
  sdk=$(cd "$OFF" && cargo metadata --locked --format-version 1 |
    jq -r '.packages[] | select(.name == "deckpilot-plugin-sdk") | .manifest_path')
  [ -n "$sdk" ] || die "the pinned SDK is not in official/Cargo.lock"
  local checkout=${sdk%/runtime/guest/plugin-sdk/Cargo.toml}
  [ "$checkout" != "$sdk" ] || die "unexpected SDK location $sdk"
  local args=()
  for crate in "${crates[@]}"; do args+=(--package "$crate"); done
  local sep=$'\x1f'
  rm -rf "$target"
  (cd "$OFF" &&
    env -u RUSTFLAGS -u RUSTC_WRAPPER -u RUSTC_WORKSPACE_WRAPPER -u CARGO_BUILD_TARGET \
      CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$OFF=/deckpilot/plugins${sep}--remap-path-prefix=$cargo_home=/cargo${sep}--remap-path-prefix=$checkout/=" \
      cargo build --locked "${args[@]}" --target "$WASM_TARGET" --profile guest --target-dir "$target")
}

module_of() { echo "$1/$WASM_TARGET/guest/${2//-/_}.wasm"; }

# package_all TARGET_DIR OUT_DIR [ID]: schema, build output, package.
package_all() {
  local target=$1 out=$2 only=${3:-}
  require_cli
  rm -rf "$out"
  mkdir -p "$out"
  local path id version crate kind
  while read -r path id version crate kind; do
    [ -z "$only" ] || [ "$id" = "$only" ] || continue
    local dir=$OFF/$path module
    module=$(module_of "$target" "$crate")
    [ -f "$module" ] || die "the build produced no $(basename "$module")"
    "$SCHEMA_CHECK" --schemas "$REPO_ROOT/schemas" \
      "https://knittingcode.github.io/deckpilot-plugins/v1/schemas/plugin-manifest.schema.json" \
      "$dir/manifest.json" >&2 || die "$dir/manifest.json does not match the manifest schema"
    if [ "$kind" = held ]; then
      if "$CLI" package --plugin-dir "$dir" --module "$module" --out "$out/$id-$version.dpplugin" \
        --first-party >"$out/$id.held.out" 2>&1; then
        die "$id is held, but its package was accepted: has the gate opened? (remove \`held\` in a reviewed change)"
      fi
      grep -q "only for DeckPilot's own plugins shipped inside the app" "$out/$id.held.out" \
        || die "$id: the package was refused for another reason than its privileged kinds: $(cat "$out/$id.held.out")"
      rm -f "$out/$id-$version.dpplugin"
      note "$id $version: held; module built, package refused for its privileged kinds, as expected"
      continue
    fi
    "$CLI" package --plugin-dir "$dir" --module "$module" --out "$out/$id-$version.dpplugin" --first-party >&2
  done < <(list --list)
}

package() {
  note "guest build and packages"
  build "$OUT/target-guest"
  package_all "$OUT/target-guest" "$PACKAGES"
  summary "### Official plugins (not released)" "" \
    "| Member | Version | Kind | Module sha256 | Package sha256 | Package bytes |" \
    "| --- | --- | --- | --- | --- | --- |"
  local path id version crate kind
  while read -r path id version crate kind; do
    local pkg=$PACKAGES/$id-$version.dpplugin
    if [ "$kind" = held ]; then
      summary "| \`$id\` | $version | held | \`$(sha "$(module_of "$OUT/target-guest" "$crate")")\` | none (privileged kinds) | |"
      continue
    fi
    summary "| \`$id\` | $version | $kind | \`$(sha "$(module_of "$OUT/target-guest" "$crate")")\` | \`$(sha "$pkg")\` | $(size "$pkg") |"
  done < <(list --list)
}

repro() {
  note "reproducibility: two clean builds"
  build "$OUT/repro-a"
  build "$OUT/repro-b"
  package_all "$OUT/repro-a" "$OUT/repro-a-packages"
  package_all "$OUT/repro-b" "$OUT/repro-b-packages"
  local path id version crate kind a b
  while read -r path id version crate kind; do
    a=$(sha "$(module_of "$OUT/repro-a" "$crate")")
    b=$(sha "$(module_of "$OUT/repro-b" "$crate")")
    [ "$a" = "$b" ] || die "$id: two clean builds differ ($a, $b)"
    if [ "$kind" = held ]; then
      note "$id $version: module reproducible ($a); held, no package"
      continue
    fi
    a=$(sha "$OUT/repro-a-packages/$id-$version.dpplugin")
    b=$(sha "$OUT/repro-b-packages/$id-$version.dpplugin")
    [ "$a" = "$b" ] || die "$id: the packages of two clean builds differ ($a, $b)"
    note "$id $version: module and package reproducible ($a)"
  done < <(list --list)
  rm -rf "$OUT/repro-a" "$OUT/repro-b"
  summary "" "Two clean builds produced byte-identical modules and packages."
}

compare() {
  [ -x "$DECKPILOT_DIR/plugins/first-party/build.sh" ] || die "no pinned DeckPilot checkout; run tools/build-cli.sh"
  note "DeckPilot's plugins/first-party/build.sh at $DECKPILOT_COMMIT"
  "$DECKPILOT_DIR/plugins/first-party/build.sh" --test-fixtures \
    --out "$OUT/deckpilot" --target-dir "$OUT/deckpilot-target" >&2
  [ -d "$OUT/target-guest" ] || build "$OUT/target-guest"
  summary "" "| Member | Here | DeckPilot build.sh | |" "| --- | --- | --- | --- |"
  local path id version crate kind ours theirs
  while read -r path id version crate kind; do
    ours=$(sha "$(module_of "$OUT/target-guest" "$crate")")
    theirs=$OUT/deckpilot/bundled/$id/plugin.wasm
    if [ ! -f "$theirs" ]; then
      [ "$kind" != fixture ] || die "$id: DeckPilot's build has no $id"
      note "$id: DeckPilot no longer builds it (consumed as an artifact)"
      continue
    fi
    theirs=$(sha "$theirs")
    if [ "$ours" = "$theirs" ]; then
      note "$id: byte-identical to DeckPilot's build ($ours)"
      summary "| \`$id\` | \`$ours\` | \`$theirs\` | identical |"
    else
      # Expected: Cargo hashes the SDK's git source (URL and rev) into the
      # crates' metadata, which orders the module's code and data, so the
      # same source differs from DeckPilot's path build. A version a
      # DeckPilot release shipped therefore never comes from here with the
      # same number: sign-test runs DeckPilot's released-pins guard.
      note "$id: differs from DeckPilot's source build ($ours here, $theirs there): a released version needs a new version number here"
      summary "| \`$id\` | \`$ours\` | \`$theirs\` | differs (git source hashed into the build) |"
    fi
  done < <(list --list)
}

# pins_of DIR ID VERSION: the bundled pins (format of DeckPilot's build.sh)
# of the files a bundle takes from a verified package.
pins_of() {
  local dir=$1 id=$2 version=$3 files='{}' file
  while IFS= read -r file; do
    case "$file" in manifest.json | plugin.wasm | LICENSE | assets/* | locales/*) ;; *) continue ;; esac
    files=$(jq --arg f "$file" --arg s "$(sha "$dir/$file")" --argjson n "$(size "$dir/$file")" \
      '. + {($f): {sha256: $s, size: $n}}' <<<"$files")
  done < <(cd "$dir" && find . -type f | sed 's|^\./||' | LC_ALL=C sort)
  jq -n --arg id "$id" --arg v "$version" --argjson f "$files" \
    '{pinsVersion: 1, plugins: [{id: $id, version: $v, class: "default", files: $f}]}'
}

sign_test() {
  require_cli
  [ -d "$PACKAGES" ] || package
  local released=$DECKPILOT_DIR/plugins/first-party/released-pins.json
  [ -f "$released" ] || die "no pinned DeckPilot checkout; run tools/build-cli.sh"
  local keys
  keys=$(mktemp -d "$OUT/keys.XXXXXX")
  "$CLI" keys generate --role publisher --out "$keys/publisher.key" --no-password >/dev/null
  jq -n --arg k "$(cat "$keys/publisher.key.pub")" \
    '{publisherKeysVersion: 1, publisher: "deckpilot", keys: [$k], threshold: 1, revokedKeys: []}' \
    >"$keys/publisher.json"
  local audit=$OUT/publisher-audit.jsonl path id version crate kind pkg
  rm -f "$audit"
  while read -r path id version crate kind; do
    if [ "$kind" = held ]; then
      note "$id: held (no package); nothing to sign"
      continue
    fi
    pkg=$PACKAGES/$id-$version.dpplugin
    rm -f "$pkg.minisig"
    "$CLI" release sign --package "$pkg" --key "$keys/publisher.key" --out "$pkg.minisig" >&2
    local claim=(--package "$pkg" --signature "$pkg.minisig" --id "$id" --version "$version"
      --sha256 "$(sha "$pkg")" --size "$(size "$pkg")"
      --app-version "$DECKPILOT_VERSION" --plugin-api "$DECKPILOT_PLUGIN_API" --platform macos
      --released-pins "$released" --audit "$audit")
    rm -rf "$OUT/verified/$id"
    "$CLI" release verify "${claim[@]}" --keys "$keys/publisher.json" --unpack "$OUT/verified/$id" >&2
    # The pins DeckPilot would record for these files must pass its guard.
    pins_of "$OUT/verified/$id" "$id" "$version" >"$OUT/verified/$id.pins.json"
    "$DECKPILOT_DIR/tools/release-pipeline/check-released-pins.sh" "$OUT/verified/$id.pins.json" \
      --platform macos >&2
    # No production key is pinned: the build's own keys refuse it.
    if "$CLI" release verify "${claim[@]}" >"$OUT/pinned.out" 2>&1; then
      grep -q "not the pinned keys" "$OUT/pinned.out" && die "unexpected output"
      note "$id: the pinned publisher keys accept it (a production key is pinned)"
    else
      grep -q "no first-party publisher key is pinned" "$OUT/pinned.out" \
        || die "$id: the pinned keys refused it for another reason: $(cat "$OUT/pinned.out")"
      note "$id: the pinned keys refuse it (no production key is pinned yet), as expected"
    fi
  done < <(list --list)
  rm -rf "$keys"
  summary "" "Signed with a throwaway publisher key and verified (claim, signature, rollback floor, released-pins guard); audit: $(wc -l <"$audit" | tr -d ' ') record(s). The keys pinned in DeckPilot refuse it until a production key is pinned. Nothing is released."
}

resolve() {
  local tag=$1
  [[ $tag =~ ^(deckpilot\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)--v((0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*))$ ]] \
    || die "$tag is not <id>--v<version> of an official plugin"
  local id=${BASH_REMATCH[1]} version=${BASH_REMATCH[3]}
  list --release "$id" >/dev/null || die "$id is not a released official plugin"
  local path
  path=$(list --list | awk -v id="$id" '$2 == id {print $1}')
  [ "$(jq -r .version "$OFF/$path/manifest.json")" = "$version" ] \
    || die "official/$path/manifest.json is not version $version"
  local commit
  commit=$(git -C "$REPO_ROOT" rev-parse HEAD)
  git -C "$REPO_ROOT" fetch -q origin main
  git -C "$REPO_ROOT" merge-base --is-ancestor "$commit" origin/main || die "$commit is not on main"
  printf 'id=%s\nversion=%s\ncommit=%s\n' "$id" "$version" "$commit"
}

release_build() {
  local id=$1 crate
  crate=$(list --list | awk -v id="$id" '$2 == id && $5 == "plugin" {print $4}')
  [ -n "$crate" ] || die "$id is not a released official plugin"
  local version
  version=$(list --list | awk -v id="$id" '$2 == id {print $3}')
  build "$OUT/release-a" "$crate"
  build "$OUT/release-b" "$crate"
  [ "$(sha "$(module_of "$OUT/release-a" "$crate")")" = "$(sha "$(module_of "$OUT/release-b" "$crate")")" ] \
    || die "$id: two clean builds differ; refusing to release"
  package_all "$OUT/release-a" "$OUT/release" "$id"
  local pkg=$OUT/release/$id-$version.dpplugin
  printf 'sha256=%s\nsize=%s\n' "$(sha "$pkg")" "$(size "$pkg")"
}

release_sign() {
  local id=$1 dir=$2 version=$3 sha256=$4 bytes=$5
  require_cli
  local pkg=$dir/$id-$version.dpplugin
  [ "$(sha "$pkg")" = "$sha256" ] && [ "$(size "$pkg")" = "$bytes" ] \
    || die "$pkg is not the package the build job produced"
  [ -n "${DECKPILOT_PUBLISHER_KEY:-}" ] || die "DECKPILOT_PUBLISHER_KEY is not set: the owner provisions it in official-release"
  "$CLI" release sign --package "$pkg" --key-env DECKPILOT_PUBLISHER_KEY \
    --password-env DECKPILOT_PUBLISHER_KEY_PASSWORD --out "$pkg.minisig"
  # The keys pinned in the DeckPilot build must accept it: never --keys.
  local platform
  for platform in macos linux windows; do
    "$CLI" release verify --package "$pkg" --signature "$pkg.minisig" --id "$id" --version "$version" \
      --sha256 "$sha256" --size "$bytes" --app-version "$DECKPILOT_VERSION" \
      --plugin-api "$DECKPILOT_PLUGIN_API" --platform "$platform" \
      --released-pins "$DECKPILOT_DIR/plugins/first-party/released-pins.json" \
      --audit "$dir/publisher-audit.jsonl"
  done
}

# release_upload TAG ID VERSION DIR SHA256 SIZE: the package and its
# signature on the GitHub Release of TAG, which the owner created (never
# creates a tag: --verify-tag). A missing release is made as a draft, both
# assets uploaded, then published, so an immutable release is complete
# before it is visible. Re-running is a no-op once the same assets are
# there; another asset under the same name fails.
release_upload() {
  local tag=$1 id=$2 version=$3 dir=$4 sha256=$5 bytes=$6
  local name=$id-$version.dpplugin
  [ "$tag" = "$id--v$version" ] || die "$tag is not the release tag of $id $version"
  [ "$(sha "$dir/$name")" = "$sha256" ] && [ "$(size "$dir/$name")" = "$bytes" ] \
    || die "$name is not the package the build job produced"
  [ -f "$dir/$name.minisig" ] || die "$name.minisig is missing"
  local repo=${GITHUB_REPOSITORY:-knittingcode/deckpilot-plugins} pre=()
  [[ $version == *-* ]] && pre=(--prerelease)
  if ! gh release view "$tag" -R "$repo" >/dev/null 2>&1; then
    gh release create "$tag" -R "$repo" --verify-tag --draft --latest=false "${pre[@]+"${pre[@]}"}" \
      --title "$id $version" \
      --notes "Official DeckPilot plugin \`$id\` $version: \`$name\`, sha256 \`$sha256\`, $bytes bytes, signed by the first-party publisher key (\`$name.minisig\`). DeckPilot bundles it only after verifying the signature against the public key pinned in the application."
  fi
  local file existing
  for file in "$name" "$name.minisig"; do
    existing=$(gh release view "$tag" -R "$repo" --json assets -q ".assets[] | select(.name == \"$file\") | .digest")
    if [ -z "$existing" ]; then
      gh release upload "$tag" -R "$repo" "$dir/$file"
    elif [ "$existing" != "sha256:$(sha "$dir/$file")" ]; then
      die "the release of $tag already has a different $file ($existing)"
    fi
  done
  if [ "$(gh release view "$tag" -R "$repo" --json isDraft -q .isDraft)" = true ]; then
    gh release edit "$tag" -R "$repo" --draft=false --latest=false
  fi
  summary "Released \`$name\` and \`$name.minisig\`: sha256 \`$sha256\`, $bytes bytes"
}

mkdir -p "$OUT"
case $what in
  check) check ;;
  test) run_tests ;;
  package) package ;;
  repro) repro ;;
  compare) compare ;;
  sign-test) sign_test ;;
  all) check; run_tests; package; repro; compare; sign_test ;;
  resolve) resolve "${2:?TAG}" ;;
  release-build) release_build "${2:?ID}" ;;
  release-sign) release_sign "${2:?ID}" "${3:?DIR}" "${4:?VERSION}" "${5:?SHA256}" "${6:?SIZE}" ;;
  release-upload) release_upload "${2:?TAG}" "${3:?ID}" "${4:?VERSION}" "${5:?DIR}" "${6:?SHA256}" "${7:?SIZE}" ;;
  *) die "usage: official.sh [check|test|package|repro|compare|sign-test|all] | resolve TAG | release-build ID | release-sign ID DIR VERSION SHA256 SIZE" ;;
esac
note "official: $what passed"
