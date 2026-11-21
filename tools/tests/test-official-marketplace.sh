#!/usr/bin/env bash
# test-official-marketplace.sh: tools/official-entry.py end to end against
# fixture artifacts signed with a THROWAWAY publisher key made here (never
# a production key, nothing leaves the scratch directory). Needs
# tools/build-cli.sh and `tools/official.sh package` (official.yml runs both
# first).
#
# Covers (docs/official-marketplace.md, "Tests"): entry generated only from
# a verified release; refusals (draft, missing or extra assets, size and
# sha256 mismatches, tampered bytes, a foreign signature, incompatible host,
# unmet dependency, downgrade, rewrite, a non-fixture id in test mode, test
# keys or flags outside test mode, writing into official/marketplace/);
# then discovery, dependency resolution and the update offer through a
# registry signed with throwaway keys and accepted by verify-site.
. "$(dirname "$0")/../lib/common.sh"
require_cli

ENTRY=(python3 -I "$REPO_ROOT/tools/official-entry.py")
WASM=$WORK/official/target-guest/$WASM_TARGET/guest/deckpilot_example_default.wasm
FIXTURE=$REPO_ROOT/official/test-fixtures/example-default
[ -f "$WASM" ] || die "$WASM is missing; run tools/official.sh package"

T=$(mktemp -d "${TMPDIR:-/tmp}/official-marketplace.XXXXXX")
trap 'rm -rf "$T"' EXIT
passed=0 failed=0
ok() { passed=$((passed + 1)); printf 'ok   %s\n' "$1"; }
bad() { failed=$((failed + 1)); printf 'FAIL %s\n' "$1" >&2; }

# Throwaway publisher keys: the one that signs, and a foreign one.
mkdir -p "$T/keys"
for k in pub other; do
  "$CLI" keys generate --role publisher --out "$T/keys/$k.key" --no-password >/dev/null
  jq -n --arg k "$(cat "$T/keys/$k.key.pub")" \
    '{publisherKeysVersion: 1, publisher: "deckpilot", keys: [$k], threshold: 1, revokedKeys: []}' >"$T/keys/$k.json"
done

# An official/ whose fixtures are the plugins of this test.
OFF=$T/official
mkdir -p "$OFF"
jq -n '{officialVersion: 1, plugins: [], fixtures: [
  {path: "test-fixtures/example-default", id: "deckpilot.example-default", permissions: ["notifications"]},
  {path: "test-fixtures/example-dependent", id: "deckpilot.example-dependent", permissions: ["notifications"]}]}' \
  >"$OFF/plugins.json"

# package ID VERSION [JQ]: a first-party package of the fixture's module with
# its manifest changed (id, version, then JQ).
package() {
  local id=$1 version=$2 filter=${3:-.} dir=$T/src/$1-$2
  mkdir -p "$dir"
  cp -R "$FIXTURE/." "$dir/"
  jq --arg id "$id" --arg v "$version" ".id = \$id | .version = \$v | $filter" "$FIXTURE/manifest.json" >"$dir/manifest.json"
  mkdir -p "$T/pkg"
  "$CLI" package --plugin-dir "$dir" --module "$WASM" --out "$T/pkg/$id-$version.dpplugin" --first-party >/dev/null
}

# bundle NAME ID VERSION [KEY]: a release bundle as `fetch` writes it, signed with KEY.
bundle() {
  local name=$1 id=$2 version=$3 key=${4:-pub} b=$T/bundles/$1
  local pkg=$id-$version.dpplugin
  mkdir -p "$b/assets"
  cp "$T/pkg/$pkg" "$b/assets/"
  "$CLI" release sign --package "$b/assets/$pkg" --key "$T/keys/$key.key" --out "$b/assets/$pkg.minisig" >/dev/null
  jq -n --arg tag "$id--v$version" --arg pkg "$pkg" \
    --argjson ps "$(wc -c <"$b/assets/$pkg" | tr -d ' ')" --arg pd "sha256:$(shasum -a 256 "$b/assets/$pkg" | cut -d' ' -f1)" \
    --argjson ss "$(wc -c <"$b/assets/$pkg.minisig" | tr -d ' ')" --arg sd "sha256:$(shasum -a 256 "$b/assets/$pkg.minisig" | cut -d' ' -f1)" \
    '{repository: "knittingcode/deckpilot-plugins", commit: "1111111111111111111111111111111111111111", onMain: true,
      release: {tag_name: $tag, draft: false, prerelease: false, published_at: "2026-11-21T10:00:00Z",
        html_url: ("https://github.com/knittingcode/deckpilot-plugins/releases/tag/" + $tag),
        assets: [{name: $pkg, size: $ps, digest: $pd, state: "uploaded"},
                 {name: ($pkg + ".minisig"), size: $ss, digest: $sd, state: "uploaded"}]}}' >"$b/release.json"
}

MP=$T/marketplace
generate() { "${ENTRY[@]}" generate --bundle "$T/bundles/$1" --test-keys "$T/keys/pub.json" --official "$OFF" --out "$MP" "${@:2}" 2>&1; }

expect_ok() {
  local name=$1; shift
  if out=$("$@"); then ok "$name"; else bad "$name: $out"; fi
}
expect_refused() {
  local name=$1 pattern=$2; shift 2
  if out=$("$@" 2>&1); then bad "$name: accepted"; elif grep -Eq "$pattern" <<<"$out"; then ok "$name"; else bad "$name: $out"; fi
}

package deckpilot.example-default 0.1.0
package deckpilot.example-default 0.1.1
package deckpilot.example-default 0.0.9
package deckpilot.example-dependent 0.1.0 \
  '.engines.pluginApi = "^1.4" | .dependencies = [{id: "deckpilot.example-default", version: ">=0.1.1 <1.0.0"}]'
package deckpilot.example-dependent 0.2.0 \
  '.engines.pluginApi = "^1.4" | .dependencies = [{id: "deckpilot.example-default", version: ">=0.2.0 <1.0.0"}]'
bundle v010 deckpilot.example-default 0.1.0
bundle v011 deckpilot.example-default 0.1.1
bundle v009 deckpilot.example-default 0.0.9
bundle dep010 deckpilot.example-dependent 0.1.0
bundle dep020 deckpilot.example-dependent 0.2.0
bundle foreign deckpilot.example-default 0.1.0 other

# mutate NAME FROM JQ [SHELL]: a copy of bundle FROM with release.json changed.
mutate() {
  cp -R "$T/bundles/$2" "$T/bundles/$1"
  jq "$3" "$T/bundles/$2/release.json" >"$T/bundles/$1/release.json"
  if [ -n "${4:-}" ]; then (cd "$T/bundles/$1/assets" && eval "$4"); fi
}
P=deckpilot.example-default-0.1.0.dpplugin
mutate draft v010 '.release.draft = true'
mutate unpublished v010 '.release.published_at = null'
mutate offmain v010 '.onMain = false'
mutate othertag v010 '.release.tag_name = "deckpilot.example-default--v0.1.1"'
mutate prerelease v010 '.release.prerelease = true'
mutate nosig v010 '.release.assets |= map(select(.name | endswith(".minisig") | not))' "rm $P.minisig"
mutate nopkg v010 '.release.assets |= map(select(.name | endswith(".dpplugin") | not))' "rm $P"
mutate extra v010 '.release.assets += [{name: "notes.txt", size: 2, state: "uploaded"}]' "printf 'hi' > notes.txt"
mutate badsize v010 '.release.assets[0].size += 1'
mutate baddigest v010 '.release.assets[0].digest = "sha256:'"$(printf '0%.0s' {1..64})"'"'
# Tampered bytes, with GitHub's metadata made to match them: only the signature catches it.
cp -R "$T/bundles/v010" "$T/bundles/tampered"
printf 'x' >>"$T/bundles/tampered/assets/$P"
jq --argjson s "$(wc -c <"$T/bundles/tampered/assets/$P" | tr -d ' ')" \
  --arg d "sha256:$(shasum -a 256 "$T/bundles/tampered/assets/$P" | cut -d' ' -f1)" \
  '.release.assets[0].size = $s | .release.assets[0].digest = $d' "$T/bundles/v010/release.json" >"$T/bundles/tampered/release.json"

expect_refused "draft release" "draft" generate draft
expect_refused "unpublished release" "not published" generate unpublished
expect_refused "commit not on main" "not on main" generate offmain
expect_refused "release of another tag" "dpplugin|not the release|of tag" generate othertag
expect_refused "pre-release flag contradicts version" "pre-release" generate prerelease
expect_refused "missing signature" "no signature" generate nosig
expect_refused "missing package" "no asset" generate nopkg
expect_refused "extra asset" "other than the package" generate extra
expect_refused "size mismatch" "bytes downloaded" generate badsize
expect_refused "sha256 mismatch" "differs from the release" generate baddigest
expect_refused "tampered package" "release verify .* refused" generate tampered
expect_refused "signature by an unpinned key" "release verify .* refused" generate foreign
expect_refused "incompatible host (DeckPilot 0.4.0)" "incompatible host|refused" generate v010 --app-version 0.4.0
expect_refused "unmet required dependency" "required dependency deckpilot.example-default .* unmet" generate dep010
expect_refused "test mode into official/marketplace" "never writes into official/marketplace" \
  "${ENTRY[@]}" generate --bundle "$T/bundles/v010" --test-keys "$T/keys/pub.json" --official "$OFF" --out "$REPO_ROOT/official/marketplace"
expect_refused "fixture id outside test mode" "not a plugin DeckPilot bundles" "${ENTRY[@]}" generate --bundle "$T/bundles/v010"
expect_refused "test flags outside test mode" "test mode" "${ENTRY[@]}" generate --bundle "$T/bundles/v010" --out "$MP"
nonfixture=$T/official-nf; mkdir -p "$nonfixture"
jq '.fixtures |= map(select(.id != "deckpilot.example-default"))' "$OFF/plugins.json" >"$nonfixture/plugins.json"
expect_refused "id outside plugins.json" "only a fixture" \
  "${ENTRY[@]}" generate --bundle "$T/bundles/v010" --test-keys "$T/keys/pub.json" --official "$nonfixture" --out "$MP"
[ ! -e "$MP" ] && ok "nothing written by a refusal" || bad "a refusal wrote $MP"

expect_ok "generate 0.1.0 from its verified release" generate v010
R=$MP/deckpilot.example-default/versions
if jq -e '.package.url == "https://github.com/knittingcode/deckpilot-plugins/releases/download/deckpilot.example-default--v0.1.0/deckpilot.example-default-0.1.0.dpplugin"
    and .package.sha256 == "'"$(shasum -a 256 "$T/pkg/$P" | cut -d' ' -f1)"'" and .package.size == '"$(wc -c <"$T/pkg/$P" | tr -d ' ')"'
    and .source.tag == "deckpilot.example-default--v0.1.0" and .source.path == "official/test-fixtures/example-default"
    and .published_at == "2026-11-21T10:00:00Z"' "$R/0.1.0.json" >/dev/null; then
  ok "record: release asset URL, sha256, size, source"
else bad "record: $(cat "$R/0.1.0.json")"; fi
jq -e '.verification.keys == "test" and (.verification.signers | length) == 1 and .names.en == "Bundled Example"' \
  "$R/0.1.0.release.json" >/dev/null && ok "release record: test keys, signer, name" || bad "release record"
expect_ok "the same release again is unchanged" generate v010
expect_refused "committed check refuses fixture and test-key records" "not a plugin DeckPilot bundles|test keys" \
  "${ENTRY[@]}" check --official "$OFF" --marketplace "$MP" --deckpilot "$DECKPILOT_DIR"
expect_ok "update 0.1.1" generate v011
expect_refused "downgrade to 0.0.9" "downgrade" generate v009
mkdir -p "$T/rewrite" && mv "$T/pkg/$P" "$T/rewrite/"
package deckpilot.example-default 0.1.0 '.description = "Other bytes, same version."'
bundle rewrite deckpilot.example-default 0.1.0
mv "$T/rewrite/$P" "$T/pkg/$P"
expect_refused "rewrite of 0.1.0 from other, validly signed bytes" "append-only" generate rewrite
expect_ok "dependent 0.1.0 resolves deckpilot.example-default 0.1.1" generate dep010
jq -e '.dependencies[0].satisfiedBy == "0.1.1 (Marketplace candidate)"' \
  "$MP/deckpilot.example-dependent/versions/0.1.0.release.json" >/dev/null && ok "dependency resolved to the candidate" \
  || bad "dependency resolution: $(cat "$MP/deckpilot.example-dependent/versions/0.1.0.release.json")"
expect_refused "dependent 0.2.0 needs default >=0.2.0" "unmet \(available: 0.1.0 .*0.1.1" generate dep020

# Discovery, dependencies and the update offer as a client sees them: the
# candidates in a registry signed with throwaway keys, verify-site accepting
# it with every package.
W=$T/site
mkdir -p "$W/registry/plugins" "$W/registry/publishers" "$T/rkeys"
cp "$REPO_ROOT/registry/publishers/knittingcode.json" "$W/registry/publishers/"
cp -R "$MP/." "$W/registry/plugins/"
find "$W/registry/plugins" -name '*.release.json' -delete
for role in root index timestamp; do "$CLI" keys generate --role "$role" --out "$T/rkeys/$role.key" --no-password >/dev/null; done
"$CLI" keyset init --dir "$W" --root "$T/rkeys/root.key.pub" --index "$T/rkeys/index.key.pub" \
  --timestamp "$T/rkeys/timestamp.key.pub" --first-party >/dev/null
"$CLI" keyset sign --dir "$W" --key "$T/rkeys/root.key" >/dev/null
for v in default-0.1.0 default-0.1.1 dependent-0.1.0; do
  id=deckpilot.example-${v%-*} version=${v##*-}
  mkdir -p "$W/releases/$id--v$version"
  cp "$T/pkg/$id-$version.dpplugin" "$W/releases/$id--v$version/"
done
"$CLI" build-index --dir "$W" --source-commit 1111111111111111111111111111111111111111 >/dev/null
"$CLI" sign-index --dir "$W" --key "$T/rkeys/index.key" >/dev/null
"$CLI" timestamp --dir "$W" --key "$T/rkeys/timestamp.key" >/dev/null
if out=$("$CLI" verify-site --dir "$W" --require-packages --min-remaining 6d 2>&1) && grep -q "3 package(s) verified" <<<"$out"; then
  ok "verify-site accepts the candidates with every package"
else bad "verify-site: $out"; fi
INDEX=$(ls "$W"/site/v1/index/*.json | grep -v minisig | head -1)
jq -e '[.plugins[] | select(.id == "deckpilot.example-default")][0] | .name == "Bundled Example" and .categories == ["other"] and .latest.stable == "0.1.1"' \
  "$INDEX" >/dev/null && ok "discovery: listed with its name and category; update offer: latest 0.1.1 over 0.1.0" \
  || bad "index: $(jq -c '.plugins' "$INDEX")"
DETAIL=$(ls "$W"/site/v1/plugins/deckpilot.example-dependent/*.json | grep -v minisig | head -1)
jq -e '[.versions[] | select(.version == "0.1.0")][0].dependencies == [{"id": "deckpilot.example-default", "version": ">=0.1.1 <1.0.0"}]' \
  "$DETAIL" >/dev/null && ok "dependencies in the signed detail" || bad "detail: $(cat "$DETAIL")"

echo "official marketplace: $passed passed, $failed failed"
[ "$failed" -eq 0 ]
