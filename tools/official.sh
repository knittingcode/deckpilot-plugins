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
# sign-test        TWO THROWAWAY publisher keys, threshold 2: release sign
#                  with each, then release verify against them (rollback
#                  floor: DeckPilot's released-pins.json; audit log): the
#                  two distinct keys pass; one signature, a duplicated
#                  signature, a wrong key, a revoked key and modified bytes
#                  are refused. Then the released-pins guard on the
#                  verified files, and the pinned keys refusing it while no
#                  production key is pinned. Keys are scratch.
# all              check, test, package, repro, compare, sign-test.
#
# fresh-build SUMS one build of every member and its packages in a fresh
#                  target directory, nothing run first; sha256 of each module
#                  and package to SUMS. official.yml runs it in two separate
#                  jobs and compares them.
#
# Release (official-release.yml only):
# fetch-deckpilot  the pinned DeckPilot checkout (a commit of its main; git
#                  only, nothing compiled), for the signing job's
#                  released-pins.json and artifacts.json.
# resolve TAG      TAG is <id>--v<version> of a released plugin whose
#                  manifest has that version, at a commit on main.
# release-build ID VERSION
#                  ONE build of ID in a fresh target directory and its
#                  package; prints `module=`, `sha256=`, `size=`. Two jobs
#                  on fresh runners run it; release-compare checks they agree.
# release-compare A_MODULE A_SHA256 A_SIZE B_MODULE B_SHA256 B_SIZE
#                  the unprivileged `compare` job: both builds agree.
# release-sign A|B ID DIR VERSION SHA256 SIZE OUT
#                  in the step of `sign-a` (or `sign-b`) that alone has
#                  DECKPILOT_PUBLISHER_KEY_A (or _B) and its password:
#                  `release sign --id --version` (the tag's) of the package
#                  in DIR, re-checked against SHA256 and SIZE, into OUT as
#                  the one file <id>-<version>.dpplugin.<KEYID>.minisig;
#                  prints `keyid=`. Refuses without its key and password, and
#                  with the other key in its environment.
# release-verify ID DIR VERSION SHA256 SIZE KEYID_A KEYID_B
#                  the `verify` job, without any key: exactly the two
#                  signatures of KEYID_A and KEYID_B (distinct, each file
#                  named by the key id it carries), DeckPilot pinning a
#                  threshold of at least 2 distinct keys, release verify
#                  with both and the keys pinned in the DeckPilot build
#                  (never --keys), the rollback floor and the revocations,
#                  every platform; each signature alone is refused.
# release-upload TAG ID VERSION DIR SHA256 SIZE
#                  the package and both signatures on the tag's release.
#
# Key test (official-test-sign.yml only; never a release):
# test-resolve TAG TAG is deckpilot.publisher-test--v<N> (N 1-9999) at a
#                  commit on main; prints `run=`, `commit=`.
# test-sign A|B RUN OUT
#                  in the step of `sign-a` (or `sign-b`) that alone has that
#                  key and its password (the same environment checks as
#                  release-sign): `release test-sign --run RUN`, the fixed
#                  key test message in its own trusted-comment domain, into
#                  OUT as the one file
#                  deckpilot-publisher-test-v<RUN>.txt.<KEYID>.minisig;
#                  prints `keyid=`.
# test-verify RUN DIR KEYID_A KEYID_B
#                  the `verify` job, without any key: exactly the two
#                  signatures, both key ids pinned in DeckPilot's
#                  first-party.json (threshold >= 2 of distinct keys),
#                  `release test-verify` with the pinned keys (never --keys);
#                  each alone refused; and `release verify` refusing them as
#                  a release of every official plugin id and version.
#
# Runs plugin build scripts: never run check..sign-test where a secret is.
# The key test tag (official-test-sign.yml); never a release or registry tag.
TEST_TAG_PREFIX=deckpilot.publisher-test--v
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
      [ "$kind" = plugin ] || die "$id: DeckPilot's build has no $id"
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

# keyid FILE: the minisign key id (16 uppercase hex) of a signature or
# public key file, as minisign prints it.
keyid() { python3 -I "$REPO_ROOT/tools/lib/minisign_keyid.py" "$1"; }

# signature_set DIR NAME: the signature files of package NAME in DIR, one
# path per line: exactly two, each NAME.<KEYID>.minisig named by the key id
# it carries, from two distinct keys. No other signature file is in DIR.
signature_set() {
  local dir=$1 name=$2 file base kid ids="" n=0 paths=""
  for file in "$dir"/*.minisig; do
    [ -e "$file" ] || continue
    base=$(basename "$file")
    kid=${base#"$name".}
    kid=${kid%.minisig}
    [ "$base" = "$name.$kid.minisig" ] && [[ $kid =~ ^[0-9A-F]{16}$ ]] \
      || die "$base is not a signature file $name.<KEYID>.minisig"
    [ "$(keyid "$file")" = "$kid" ] || die "$base carries key id $(keyid "$file" || true), not $kid"
    case " $ids " in *" $kid "*) die "two signatures of key $kid: two distinct keys must sign" ;; esac
    ids="$ids $kid" n=$((n + 1)) paths="$paths$file"$'\n'
  done
  [ "$n" = 2 ] || die "$dir has $n signature(s) of $name; an official release needs exactly two, by two distinct publisher keys"
  printf '%s' "$paths"
}

sign_test() {
  require_cli
  [ -d "$PACKAGES" ] || package
  local released=$DECKPILOT_DIR/plugins/first-party/released-pins.json
  [ -f "$released" ] || die "no pinned DeckPilot checkout; run tools/build-cli.sh"
  # Two throwaway keys of the release (a, b), a key nobody pinned (wrong)
  # and a revoked one; the test key set is a, b, threshold 2.
  local keys k
  keys=$(mktemp -d "$OUT/keys.XXXXXX")
  for k in a b wrong revoked; do
    "$CLI" keys generate --role publisher --out "$keys/$k.key" --no-password >/dev/null
  done
  [ "$(keyid "$keys/a.key.pub")" != "$(keyid "$keys/b.key.pub")" ] || die "the two throwaway keys share a key id"
  jq -n --arg a "$(cat "$keys/a.key.pub")" --arg b "$(cat "$keys/b.key.pub")" --arg r "$(cat "$keys/revoked.key.pub")" \
    '{publisherKeysVersion: 1, publisher: "deckpilot", keys: [$a, $b], threshold: 2, revokedKeys: [$r]}' \
    >"$keys/publisher.json"
  local audit=$OUT/publisher-audit.jsonl path id version crate kind pkg sigs
  rm -f "$audit"
  # refused WHAT PATTERN ARGS...: release verify with the test keys must
  # refuse, saying PATTERN.
  refused() {
    local what=$1 pattern=$2; shift 2
    if "$CLI" release verify "${claim[@]}" "${test_keys[@]}" "$@" >"$OUT/refused.out" 2>&1; then
      die "$id: release verify accepted $what"
    fi
    grep -Eqi -- "$pattern" "$OUT/refused.out" || die "$id: $what refused for another reason: $(cat "$OUT/refused.out")"
    note "$id: $what refused"
  }
  while read -r path id version crate kind; do
    pkg=$PACKAGES/$id-$version.dpplugin
    local name=$id-$version.dpplugin
    sigs=$OUT/signatures/$id
    rm -rf "$sigs" "$PACKAGES/$name".*minisig
    mkdir -p "$sigs/all"
    for k in a b wrong revoked; do
      "$CLI" release sign --package "$pkg" --id "$id" --version "$version" --key "$keys/$k.key" \
        --out-dir "$sigs/all" >&2
      [ -f "$sigs/all/$name.$(keyid "$keys/$k.key.pub").minisig" ] \
        || die "release sign did not write $name.$(keyid "$keys/$k.key.pub").minisig for key $k"
    done
    local A B W R
    A=$name.$(keyid "$keys/a.key.pub").minisig B=$name.$(keyid "$keys/b.key.pub").minisig
    W=$name.$(keyid "$keys/wrong.key.pub").minisig R=$name.$(keyid "$keys/revoked.key.pub").minisig
    # set DIR FILE...: a signatures directory holding these files of all/.
    set_of() {
      local d=$sigs/$1 f; shift
      mkdir -p "$d"
      for f in "$@"; do cp "$sigs/all/$f" "$d/"; done
      echo "$d"
    }
    local claim=(--id "$id" --version "$version"
      --app-version "$DECKPILOT_VERSION" --plugin-api "$DECKPILOT_PLUGIN_API" --platform macos
      --released-pins "$released" --revocations "$DECKPILOT_DIR/plugins/first-party/artifacts.json")
    local test_keys=(--keys "$keys/publisher.json")
    local digest=(--package "$pkg" --sha256 "$(sha "$pkg")" --size "$(size "$pkg")")
    refused "one signature (threshold 2)" "threshold|required" "${digest[@]}" --signatures-dir "$(set_of one "$A")"
    local dup=$sigs/duplicate
    mkdir -p "$dup" && cp "$sigs/all/$A" "$dup/$A" && cp "$sigs/all/$A" "$dup/$B"
    refused "the same key's signature twice (under the other key's name)" "threshold|required" "${digest[@]}" --signatures-dir "$dup"
    refused "a signature by a key that is not pinned" "threshold|required" "${digest[@]}" \
      --signatures-dir "$(set_of wrong "$A" "$W")"
    refused "a signature by a revoked key" "revoked" "${digest[@]}" --signatures-dir "$(set_of revoked "$A" "$R")"
    refused "a revoked key's signature next to both keys" "revoked" "${digest[@]}" \
      --signatures-dir "$(set_of revoked-too "$A" "$B" "$R")"
    local release
    release=$(set_of release "$A" "$B")
    cp "$pkg" "$sigs/modified.dpplugin"
    printf 'x' >>"$sigs/modified.dpplugin"
    refused "modified bytes (claimed with their own sha256 and size)" "does not verify|0 valid" \
      --package "$sigs/modified.dpplugin" --sha256 "$(sha "$sigs/modified.dpplugin")" \
      --size "$(size "$sigs/modified.dpplugin")" --signatures-dir "$release"
    # The file naming of a release: a duplicate, a third key or a misnamed
    # file is refused here before anything is verified.
    (signature_set "$sigs/all" "$name" >/dev/null 2>&1) && die "$id: four signature files accepted"
    (signature_set "$sigs/one" "$name" >/dev/null 2>&1) && die "$id: one signature file accepted"
    (signature_set "$dup" "$name" >/dev/null 2>&1) && die "$id: a duplicated signature under another key's name accepted"
    signature_set "$release" "$name" >/dev/null
    note "$id: one, four, a duplicated or a misnamed signature file refused"
    # Two distinct keys: verified.
    rm -rf "$OUT/verified/$id"
    "$CLI" release verify "${claim[@]}" "${test_keys[@]}" "${digest[@]}" --signatures-dir "$release" \
      --audit "$audit" --unpack "$OUT/verified/$id" >&2
    [ "$(tail -1 "$audit" | jq '[.signers[]?] | unique | length')" = 2 ] || die "$id: the audit record does not name two signers"
    # The pins DeckPilot would record for these files must pass its guard.
    pins_of "$OUT/verified/$id" "$id" "$version" >"$OUT/verified/$id.pins.json"
    "$DECKPILOT_DIR/tools/release-pipeline/check-released-pins.sh" "$OUT/verified/$id.pins.json" \
      --platform macos >&2
    # The keys pinned in the build (the production keys) never count a
    # throwaway key: refused below the threshold.
    if "$CLI" release verify "${claim[@]}" "${digest[@]}" --signatures-dir "$release" >"$OUT/pinned.out" 2>&1; then
      die "$id: the pinned publisher keys accept a release signed by two throwaway keys"
    fi
    if ! grep -q "0 valid signature(s)" "$OUT/pinned.out" || ! grep -q "not a pinned publisher key" "$OUT/pinned.out"; then
      die "$id: the pinned keys refused it for another reason: $(cat "$OUT/pinned.out")"
    fi
    note "$id: the pinned (production) keys refuse the throwaway keys' signatures, as expected"
  done < <(list --list)
  rm -rf "$keys" "$OUT/signatures"
  summary "" "Signed with two throwaway publisher keys (threshold 2) and verified (claim, both signatures, rollback floor, released-pins guard); one signature, a duplicated signature, a wrong key, a revoked key and modified bytes refused; audit: $(wc -l <"$audit" | tr -d ' ') record(s). The keys pinned in DeckPilot (the production keys) refuse it. Nothing is released."
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

# fresh_build SUMS: one build of every member in a fresh target directory
# and its packages, without running anything else first (no tests, no
# build scripts of a previous step); SUMS gets the sha256 of every module
# and package. official.yml runs it in two separate jobs, on fresh runners,
# and compares the two files (independent builds, finding 4).
fresh_build() {
  local sums=$1
  rm -rf "$OUT/fresh" "$OUT/fresh-packages"
  build "$OUT/fresh"
  package_all "$OUT/fresh" "$OUT/fresh-packages"
  local path id version crate kind
  : >"$sums"
  while read -r path id version crate kind; do
    printf '%s  module %s\n' "$(sha "$(module_of "$OUT/fresh" "$crate")")" "$id-$version" >>"$sums"
    printf '%s  package %s.dpplugin (%s bytes)\n' "$(sha "$OUT/fresh-packages/$id-$version.dpplugin")" \
      "$id-$version" "$(size "$OUT/fresh-packages/$id-$version.dpplugin")" >>"$sums"
  done < <(list --list)
  cat "$sums"
}

# release_build ID VERSION: ONE build of ID in a fresh target directory and
# its package, for official-release.yml's two independent build jobs (each
# on its own fresh runner, nothing run before it); the sign job compares
# their outputs. VERSION is the release tag's: the manifest must say it.
release_build() {
  local id=$1 version=$2 crate
  crate=$(list --list | awk -v id="$id" '$2 == id && $5 == "plugin" {print $4}')
  [ -n "$crate" ] || die "$id is not a released official plugin"
  [ "$(list --list | awk -v id="$id" '$2 == id {print $3}')" = "$version" ] \
    || die "$id is not at version $version (the release tag's)"
  build "$OUT/release-target" "$crate"
  package_all "$OUT/release-target" "$OUT/release" "$id"
  local pkg=$OUT/release/$id-$version.dpplugin
  printf 'module=%s\nsha256=%s\nsize=%s\n' "$(sha "$(module_of "$OUT/release-target" "$crate")")" \
    "$(sha "$pkg")" "$(size "$pkg")"
}

# release_compare A_MODULE A_SHA256 A_SIZE B_MODULE B_SHA256 B_SIZE: the
# two independent builds agree on the module and the package.
release_compare() {
  [ "$1" = "$4" ] || die "the two independent builds produced different modules ($1, $4); refusing to release"
  [ "$2" = "$5" ] && [ "$3" = "$6" ] \
    || die "the two independent builds produced different packages ($2 $3 bytes, $5 $6 bytes); refusing to release"
  [[ $2 =~ ^[0-9a-f]{64}$ ]] || die "no package sha256 from the build jobs"
  note "independent builds agree: module $1, package $2 ($3 bytes)"
}

# signing_environment SLOT OTHER: fail closed, before anything else: the
# job's own key and password are set, and nothing of the other key or of
# the retired single key is (release-sign and test-sign alike).
signing_environment() {
  local slot=$1 other=$2
  local key=DECKPILOT_PUBLISHER_KEY_$slot
  local password=${key}_PASSWORD other_key=DECKPILOT_PUBLISHER_KEY_$other
  local other_password=${other_key}_PASSWORD
  [ -n "${!key:-}" ] \
    || die "$key is not set: the owner provisions it (and $password) in the environment official-release; refusing to sign, no release without both publisher keys"
  [ -n "${!password:-}" ] \
    || die "$password is not set: the owner provisions it with $key in the environment official-release; refusing to sign"
  [ -z "${!other_key:-}${!other_password:-}" ] \
    || die "the job signing with publisher key $slot also has $other_key: each key signs in its own job (sign-a, sign-b)"
  local var
  for var in $(compgen -e); do
    case $var in
      "$key" | "$password") ;;
      DECKPILOT_PUBLISHER_KEY*) die "$var is in the environment of the job signing with key $slot: only $key and $password are" ;;
    esac
  done
}

# no_key_environment WHAT: WHAT runs without any publisher key in its
# environment.
no_key_environment() {
  local var
  for var in $(compgen -e); do
    case $var in DECKPILOT_PUBLISHER_KEY*) die "$1 runs without any publisher key in its environment ($var)" ;; esac
  done
}

# pinned_keys_file: DeckPilot's first-party.json at DECKPILOT_COMMIT (the
# keys the pinned deckpilot-registry compiles in), refused unless it pins a
# threshold of at least 2 distinct keys. Prints its path.
pinned_keys_file() {
  local pinned=$DECKPILOT_DIR/crates/deckpilot-plugin-registry/publisher/first-party.json
  [ -f "$pinned" ] || die "no pinned DeckPilot checkout ($pinned); run tools/official.sh fetch-deckpilot"
  jq -e '(.threshold | type == "number" and . >= 2) and ((.keys | unique | length) >= .threshold)' "$pinned" >/dev/null \
    || die "DeckPilot $DECKPILOT_COMMIT pins $(jq '.keys | length' "$pinned") publisher key(s), threshold $(jq .threshold "$pinned"): an official release needs a threshold of at least 2 distinct pinned keys"
  echo "$pinned"
}

# pinned_key_ids FILE: the minisign key ids of FILE's current keys, sorted.
pinned_key_ids() {
  python3 -I -c '
import base64, json, sys
for key in json.load(open(sys.argv[1]))["keys"]:
    line = base64.b64decode(key).decode().splitlines()[1]
    raw = base64.b64decode(line)
    assert raw[:2] == b"Ed" and len(raw) == 42
    print(raw[2:10][::-1].hex().upper())' "$1" | LC_ALL=C sort
}

# require_pinned_signers FILE KEYID...: every KEYID is a key FILE pins.
require_pinned_signers() {
  local file=$1 ids kid; shift
  ids=$(pinned_key_ids "$file") || die "the key ids of $file cannot be read"
  for kid in "$@"; do
    grep -qxF -- "$kid" <<<"$ids" || die "key $kid signed, but DeckPilot $DECKPILOT_COMMIT does not pin it ($(tr '\n' ' ' <<<"$ids"))"
  done
}

# release_sign A|B ID DIR VERSION SHA256 SIZE OUT: one signature, nothing
# else, in the step of sign-a (or sign-b) that alone has that key. ID and
# VERSION are the release tag's (resolve), passed to `release sign`, which
# refuses a package that says otherwise and parses the package before it
# loads the key. OUT gets exactly one file, <id>-<version>.dpplugin.<KEYID>.minisig;
# `keyid=` goes to stdout. The verify job, without any key, checks both.
release_sign() {
  local slot=$1 id=$2 dir=$3 version=$4 sha256=$5 bytes=$6 out=$7 other
  case $slot in
    A) other=B ;;
    B) other=A ;;
    *) die "release-sign: the publisher key is A or B, not '$slot'" ;;
  esac
  signing_environment "$slot" "$other"
  local key=DECKPILOT_PUBLISHER_KEY_$slot
  local password=${key}_PASSWORD
  local name=$id-$version.dpplugin
  local pkg=$dir/$name
  [ -f "$pkg" ] && [ "$(sha "$pkg")" = "$sha256" ] && [ "$(size "$pkg")" = "$bytes" ] \
    || die "$pkg is not the package the build jobs produced (sha256 $sha256, $bytes bytes)"
  [ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || die "$out must be new or empty: it holds only this job's signature"
  require_cli
  mkdir -p "$out"
  local scratch kid file
  scratch=$(mktemp -d)
  "$CLI" release sign --package "$pkg" --id "$id" --version "$version" --key-env "$key" \
    --password-env "$password" --out-dir "$scratch" >&2
  set -- "$scratch"/*
  [ $# = 1 ] && [ -f "$1" ] || die "release sign wrote $# file(s); exactly one signature is expected"
  file=$(basename "$1")
  kid=$(keyid "$1") || die "the new signature carries no minisign key id"
  [ "$file" = "$name.$kid.minisig" ] || die "release sign wrote $file, not $name.$kid.minisig"
  mv "$1" "$out/$file"
  rmdir "$scratch"
  note "signed $name with publisher key $slot, key id $kid"
  echo "keyid=$kid"
}

# release_verify ID DIR VERSION SHA256 SIZE KEYID_A KEYID_B: with the keys
# pinned in the DeckPilot build (never --keys), for every platform, against
# DeckPilot's rollback floor (released-pins.json) and revocations
# (artifacts.json), audited. Exactly the two signatures sign-a and sign-b
# reported, from two distinct keys, and DeckPilot pins at least two keys
# with a threshold of at least two: each signature alone is refused.
# Refuses to run where a publisher key is in the environment.
release_verify() {
  local id=$1 dir=$2 version=$3 sha256=$4 bytes=$5 want_a=$6 want_b=$7
  no_key_environment release-verify
  [[ $want_a =~ ^[0-9A-F]{16}$ ]] && [[ $want_b =~ ^[0-9A-F]{16}$ ]] \
    || die "sign-a and sign-b reported no key id ('$want_a', '$want_b'): no release without both signatures"
  [ "$want_a" != "$want_b" ] || die "sign-a and sign-b signed with the same key ($want_a): two distinct keys must sign"
  local name=$id-$version.dpplugin
  local pkg=$dir/$name
  [ -f "$pkg" ] && [ "$(sha "$pkg")" = "$sha256" ] && [ "$(size "$pkg")" = "$bytes" ] \
    || die "$pkg is not the package the build jobs produced"
  local file got=() files=()
  while IFS= read -r file; do
    got+=("$(keyid "$file")")
    files+=("$file")
  done < <(signature_set "$dir" "$name")
  [ ${#got[@]} = 2 ] || die "no signature set"
  [ "$(printf '%s\n' "${got[@]}" | LC_ALL=C sort | tr '\n' ' ')" = "$(printf '%s\n' "$want_a" "$want_b" | LC_ALL=C sort | tr '\n' ' ')" ] \
    || die "the signatures are by ${got[*]}, not the keys sign-a and sign-b used ($want_a, $want_b)"
  require_cli
  local pinned
  pinned=$(pinned_keys_file)
  require_pinned_signers "$pinned" "$want_a" "$want_b"
  local claim=(--package "$pkg" --id "$id" --version "$version" --sha256 "$sha256" --size "$bytes"
    --app-version "$DECKPILOT_VERSION" --plugin-api "$DECKPILOT_PLUGIN_API"
    --released-pins "$DECKPILOT_DIR/plugins/first-party/released-pins.json"
    --revocations "$DECKPILOT_DIR/plugins/first-party/artifacts.json")
  local audit=$dir/publisher-audit.jsonl platform
  for platform in macos linux windows; do
    "$CLI" release verify "${claim[@]}" --signatures-dir "$dir" --platform "$platform" --audit "$audit"
  done
  jq -se 'length >= 3 and all(.outcome == "verified" and ([.signers[]?] | unique | length) >= 2)' "$audit" >/dev/null \
    || die "the audit does not record a verification by two distinct signers: $(cat "$audit")"
  # The threshold, seen from outside: one signature alone is refused.
  local one
  for file in "${files[@]}"; do
    one=$(mktemp -d)
    cp "$file" "$one/"
    if "$CLI" release verify "${claim[@]}" --signatures-dir "$one" --platform macos >/dev/null 2>&1; then
      rm -rf "$one"
      die "the pinned keys accept $(basename "$file") alone: the threshold is not two"
    fi
    rm -rf "$one"
  done
  note "$name: two distinct pinned publisher keys ($want_a, $want_b), every platform; one signature alone refused"
}

# test_resolve TAG: the key test tag deckpilot.publisher-test--v<N>, N from
# 1 to 9999 without leading zeros, at a commit on main.
test_resolve() {
  local tag=$1
  [[ $tag =~ ^deckpilot\.publisher-test--v([1-9][0-9]{0,3})$ ]] \
    || die "$tag is not a key test tag ${TEST_TAG_PREFIX}<N> (N 1-9999)"
  local run=${BASH_REMATCH[1]} commit
  commit=$(git -C "$REPO_ROOT" rev-parse HEAD)
  git -C "$REPO_ROOT" fetch -q origin main
  git -C "$REPO_ROOT" merge-base --is-ancestor "$commit" origin/main || die "$commit is not on main"
  printf 'run=%s\ncommit=%s\n' "$run" "$commit"
}

# test_sign A|B RUN OUT: the key test signature of one key, nothing else,
# in the step of sign-a (or sign-b) that alone has that key. Signs only
# DeckPilot's fixed key test message of RUN (`release test-sign`: another
# trusted-comment domain than a release's; it takes no file to sign).
test_sign() {
  local slot=$1 run=$2 out=$3 other
  case $slot in
    A) other=B ;;
    B) other=A ;;
    *) die "test-sign: the publisher key is A or B, not '$slot'" ;;
  esac
  [[ $run =~ ^[1-9][0-9]{0,3}$ ]] || die "test-sign: the run is 1 to 9999, not '$run'"
  signing_environment "$slot" "$other"
  local key=DECKPILOT_PUBLISHER_KEY_$slot
  local password=${key}_PASSWORD
  [ ! -e "$out" ] || [ -z "$(ls -A "$out")" ] || die "$out must be new or empty: it holds only this job's signature"
  require_cli
  mkdir -p "$out"
  local name=deckpilot-publisher-test-v$run.txt scratch kid file
  scratch=$(mktemp -d)
  "$CLI" release test-sign --run "$run" --key-env "$key" --password-env "$password" --out-dir "$scratch" >&2
  set -- "$scratch"/*
  [ $# = 1 ] && [ -f "$1" ] || die "release test-sign wrote $# file(s); exactly one signature is expected"
  file=$(basename "$1")
  kid=$(keyid "$1") || die "the new signature carries no minisign key id"
  [ "$file" = "$name.$kid.minisig" ] || die "release test-sign wrote $file, not $name.$kid.minisig"
  mv "$1" "$out/$file"
  rmdir "$scratch"
  note "signed the key test message $run with publisher key $slot, key id $kid (a key test, never a release)"
  echo "keyid=$kid"
}

# test_verify RUN DIR KEYID_A KEYID_B: the key test's `verify` job, without
# any key. Passing proves the two stored secrets are the two keys DeckPilot
# pins and that their passwords unlock them.
test_verify() {
  local run=$1 dir=$2 want_a=$3 want_b=$4
  no_key_environment test-verify
  [[ $run =~ ^[1-9][0-9]{0,3}$ ]] || die "test-verify: the run is 1 to 9999, not '$run'"
  [[ $want_a =~ ^[0-9A-F]{16}$ ]] && [[ $want_b =~ ^[0-9A-F]{16}$ ]] \
    || die "sign-a and sign-b reported no key id ('$want_a', '$want_b')"
  [ "$want_a" != "$want_b" ] || die "sign-a and sign-b signed with the same key ($want_a): two distinct keys must sign"
  local name=deckpilot-publisher-test-v$run.txt file got=() files=()
  while IFS= read -r file; do
    got+=("$(keyid "$file")")
    files+=("$file")
  done < <(signature_set "$dir" "$name")
  [ ${#got[@]} = 2 ] || die "no signature set"
  [ "$(printf '%s\n' "${got[@]}" | LC_ALL=C sort | tr '\n' ' ')" = "$(printf '%s\n' "$want_a" "$want_b" | LC_ALL=C sort | tr '\n' ' ')" ] \
    || die "the signatures are by ${got[*]}, not the keys sign-a and sign-b used ($want_a, $want_b)"
  require_cli
  local pinned
  pinned=$(pinned_keys_file)
  require_pinned_signers "$pinned" "$want_a" "$want_b"
  note "DeckPilot $DECKPILOT_COMMIT first-party.json: sha256 $(sha "$pinned"), keys $(pinned_key_ids "$pinned" | tr '\n' ' ')"
  # The pinned keys (compiled into the pinned deckpilot-registry from that
  # file), threshold 2: both signatures verify.
  "$CLI" release test-verify --run "$run" --signatures-dir "$dir"
  # The threshold, seen from outside: one signature alone is refused.
  local one
  for file in "${files[@]}"; do
    one=$(mktemp -d)
    cp "$file" "$one/"
    if "$CLI" release test-verify --run "$run" --signatures-dir "$one" >/dev/null 2>&1; then
      rm -rf "$one"
      die "the pinned keys accept $(basename "$file") alone: the threshold is not two"
    fi
    rm -rf "$one"
  done
  # Never a release: the message with both signatures, claimed as every
  # official plugin at its version (and as a release of the key test's own
  # name), is refused by `release verify` with the pinned keys.
  local claims=() path id version crate kind
  while read -r path id version crate kind; do claims+=("$id $version"); done < <(list --list)
  claims+=("deckpilot.publisher-test $run.0.0")
  local scratch message pkg digest_sha digest_size claim
  scratch=$(mktemp -d)
  "$CLI" release test-message --run "$run" --out-dir "$scratch" >&2
  message=$scratch/$name
  digest_sha=$(sha "$message") digest_size=$(size "$message")
  for claim in "${claims[@]}"; do
    read -r id version <<<"$claim"
    local as=$scratch/as/$id
    mkdir -p "$as"
    pkg=$as/$id-$version.dpplugin
    cp "$message" "$pkg"
    for file in "${files[@]}"; do
      cp "$file" "$pkg.$(keyid "$file").minisig"
    done
    if "$CLI" release verify --package "$pkg" --id "$id" --version "$version" --sha256 "$digest_sha" \
      --size "$digest_size" --app-version "$DECKPILOT_VERSION" --plugin-api "$DECKPILOT_PLUGIN_API" \
      --platform macos >"$scratch/refused.out" 2>&1; then
      rm -rf "$scratch"
      die "release verify accepts the key test signatures as a release of $id $version"
    fi
    grep -q "0 valid signature(s)" "$scratch/refused.out" \
      || { cat "$scratch/refused.out" >&2; rm -rf "$scratch"; die "release verify refused the key test as $id $version for another reason"; }
  done
  rm -rf "$scratch"
  note "key test $run: both pinned publisher keys ($want_a, $want_b) signed; one alone refused; release verify refuses it as a release of ${#claims[@]} id(s)"
  summary "Key test $run passed: publisher keys \`$want_a\` and \`$want_b\` (pinned in DeckPilot \`$DECKPILOT_COMMIT\`, first-party.json sha256 \`$(sha "$pinned")\`) both signed the fixed test message, threshold 2; each alone is refused, and \`release verify\` refuses the signatures as a release. Nothing was released."
}

# release_upload TAG ID VERSION DIR SHA256 SIZE: the package and its two
# signatures on the GitHub Release of TAG, which the owner created (never
# creates a tag: --verify-tag). A missing release is made as a draft, the
# three assets uploaded, then published, so an immutable release is
# complete before it is visible. Re-running is a no-op once the same assets
# are there; another asset under the same name, or any other asset, fails
# and leaves the draft unpublished.
release_upload() {
  local tag=$1 id=$2 version=$3 dir=$4 sha256=$5 bytes=$6
  local name=$id-$version.dpplugin
  [ "$tag" = "$id--v$version" ] || die "$tag is not the release tag of $id $version"
  [ "$(sha "$dir/$name")" = "$sha256" ] && [ "$(size "$dir/$name")" = "$bytes" ] \
    || die "$name is not the package the build job produced"
  local files=("$dir/$name") file
  while IFS= read -r file; do files+=("$file"); done < <(signature_set "$dir" "$name")
  [ ${#files[@]} = 3 ] || die "no signature set"
  local repo=${GITHUB_REPOSITORY:-knittingcode/deckpilot-plugins} pre=()
  [[ $version == *-* ]] && pre=(--prerelease)
  local sig_a sig_b
  sig_a=$(basename "${files[1]}") sig_b=$(basename "${files[2]}")
  if ! gh release view "$tag" -R "$repo" >/dev/null 2>&1; then
    gh release create "$tag" -R "$repo" --verify-tag --draft --latest=false "${pre[@]+"${pre[@]}"}" \
      --title "$id $version" \
      --notes "Official DeckPilot plugin \`$id\` $version: \`$name\`, sha256 \`$sha256\`, $bytes bytes, signed by two first-party publisher keys (\`$sig_a\`, \`$sig_b\`). DeckPilot bundles it only after verifying both signatures against the public keys pinned in the application (threshold 2)."
  fi
  local existing
  for file in "${files[@]}"; do
    existing=$(gh release view "$tag" -R "$repo" --json assets -q ".assets[] | select(.name == \"$(basename "$file")\") | .digest")
    if [ -z "$existing" ]; then
      gh release upload "$tag" -R "$repo" "$file"
    elif [ "$existing" != "sha256:$(sha "$file")" ]; then
      die "the release of $tag already has a different $(basename "$file") ($existing)"
    fi
  done
  local want have
  want=$(printf '%s\n' "$name" "$sig_a" "$sig_b" | LC_ALL=C sort | tr '\n' ' ')
  have=$(gh release view "$tag" -R "$repo" --json assets -q '.assets[].name' | LC_ALL=C sort | tr '\n' ' ')
  [ "$have" = "$want" ] || die "the release of $tag has the assets '$have', not exactly '$want'; not publishing it"
  if [ "$(gh release view "$tag" -R "$repo" --json isDraft -q .isDraft)" = true ]; then
    gh release edit "$tag" -R "$repo" --draft=false --latest=false
  fi
  summary "Released \`$name\`, \`$sig_a\` and \`$sig_b\`: sha256 \`$sha256\`, $bytes bytes"
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
  fetch-deckpilot) fetch_deckpilot "$DECKPILOT_DIR" ;;
  resolve) resolve "${2:?TAG}" ;;
  fresh-build) fresh_build "${2:?SUMS}" ;;
  release-build) release_build "${2:?ID}" "${3:?VERSION}" ;;
  release-compare) release_compare "${2:?}" "${3:?}" "${4:?}" "${5:?}" "${6:?}" "${7:?}" ;;
  release-sign) release_sign "${2:?A|B}" "${3:?ID}" "${4:?DIR}" "${5:?VERSION}" "${6:?SHA256}" "${7:?SIZE}" "${8:?OUT}" ;;
  release-verify) release_verify "${2:?ID}" "${3:?DIR}" "${4:?VERSION}" "${5:?SHA256}" "${6:?SIZE}" "${7:-}" "${8:-}" ;;
  release-upload) release_upload "${2:?TAG}" "${3:?ID}" "${4:?VERSION}" "${5:?DIR}" "${6:?SHA256}" "${7:?SIZE}" ;;
  test-resolve) test_resolve "${2:?TAG}" ;;
  test-sign) test_sign "${2:?A|B}" "${3:?RUN}" "${4:?OUT}" ;;
  test-verify) test_verify "${2:?RUN}" "${3:?DIR}" "${4:-}" "${5:-}" ;;
  *) die "usage: official.sh [check|test|package|repro|compare|sign-test|all] | fresh-build SUMS | resolve TAG | release-build ID VERSION | release-compare A_MODULE A_SHA A_SIZE B_MODULE B_SHA B_SIZE | release-sign A|B ID DIR VERSION SHA256 SIZE OUT | release-verify ID DIR VERSION SHA256 SIZE KEYID_A KEYID_B | release-upload TAG ID VERSION DIR SHA256 SIZE | test-resolve TAG | test-sign A|B RUN OUT | test-verify RUN DIR KEYID_A KEYID_B" ;;
esac
note "official: $what passed"
