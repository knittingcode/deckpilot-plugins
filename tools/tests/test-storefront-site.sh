#!/usr/bin/env bash
# Tests of the storefront in the signing pipeline (tools/publish-site.sh and
# tools/mirror-site.sh; docs/storefront.md), offline: the pinned tools
# (tools/build-cli.sh), throwaway keys and a loopback server standing in for
# Pages, the live site of each step being the site the step before signed.
#
#   1. REGISTRY_STOREFRONT_ENABLED unset or `false`: the site is the one the
#      pipeline built before storefronts: no storefront/ or media/, the
#      same files, and the same bytes but for signatures, signing times and
#      the source commit. With BASELINE_REF (a commit of this repository),
#      also compared with what that commit's tools/ build from the same
#      inputs and keys (validate.yml: the pull request's base).
#   2. `true`: storefront check, build and sign in the signing job, before
#      the timestamp; verify-site --require-storefront passes; the client
#      state records the storefront; drafts never reach the site.
#   3. A timestamp refresh of that site keeps index/, plugins/, storefront/
#      and media/ byte for byte (also with the variable off: it is
#      index-only), at the same seq.
#   4. A revert (listing and editorial removed, as the revert of their pull
#      request does) publishes a higher seq whose storefront has neither;
#      clients accept it, and the previous storefront stays only as the
#      kept index's.
#   5. Refusals: an invalid REGISTRY_STOREFRONT_ENABLED, and a source the
#      storefront check refuses, sign nothing.
#   7. tools/verify-published.sh on the live site with the storefront:
#      every file of storefront/ and media/ fetched anonymously equals the
#      signed one, verify-site --require-storefront; a difference fails, and
#      so does an enabled publication whose signed site has no storefront.
#   6. Disabled again: the next publication has no storefront.
#
#   tools/tests/test-storefront-site.sh        (BASELINE_REF=<commit> optional)
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/storefront-site.XXXXXX")
# shellcheck source=lib/registry-site.sh
. "$root/tools/tests/lib/registry-site.sh"
cleanup() { site_stop; rm -rf "$work"; }
trap cleanup EXIT
passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }
# check NAME COMMAND...: one assertion.
check() { local name=$1; shift; if "$@"; then ok "$name"; else bad "$name"; fi; }
# publish NAME DIR MODE OUT [ENV=VALUE...]: a publication that must pass.
publish() {
  local name=$1; shift
  if site_publish "$@"; then ok "$name"; else bad "$name: $(tail -5 "$3.log" | tr '\n' ' ')"; fi
}
# refused NAME TEXT DIR MODE OUT [ENV=VALUE...]: one that must fail saying TEXT.
refused() {
  local name=$1 text=$2; shift 2
  if site_publish "$@"; then bad "$name: accepted"
  elif grep -q -- "$text" "$3.log"; then ok "$name"
  else bad "$name: refused for another reason: $(tail -3 "$3.log" | tr '\n' ' ')"; fi
}
no_storefront() { [ ! -e "$1/site/v1/storefront" ] && [ ! -e "$1/site/v1/media" ]; }
same() { cmp -s <(site_normalized "$1") <(site_normalized "$2") || { diff <(site_normalized "$1") <(site_normalized "$2") | head -10; return 1; }; }
seq_of() { jq -r .seq "$1/site/v1/timestamp.json"; }

site_start
repo=$work/repo
site_repo "$repo"

# --- 1. disabled: today's site --------------------------------------------
publish "first publication, variable unset" "$repo" index "$work/unset"
publish "first publication, variable false" "$repo" index "$work/false" REGISTRY_STOREFRONT_ENABLED=false
check "unset: no storefront/ or media/" no_storefront "$work/unset"
check "false: the same site as unset (files; bytes but signatures and times)" same "$work/unset" "$work/false"
if [ -n "${BASELINE_REF:-}" ]; then
  if git -C "$root" cat-file -e "$BASELINE_REF:tools/publish-site.sh" 2>/dev/null; then
    site_clone "$repo" "$work/base" "$BASELINE_REF"
    publish "first publication with the tools of $BASELINE_REF" "$work/base" index "$work/baseline"
    check "false: the same site as the tools of $BASELINE_REF build" same "$work/baseline" "$work/false"
  else
    bad "BASELINE_REF $BASELINE_REF has no tools/publish-site.sh here (fetch it first)"
  fi
fi
site_serve "$work/false"
publish "second publication (live seq 1), variable false" "$repo" index "$work/false2" REGISTRY_STOREFRONT_ENABLED=false
check "... seq 2, still no storefront" bash -c '[ "$(jq .seq "$1/site/v1/timestamp.json")" = 2 ] && [ ! -e "$1/site/v1/storefront" ] && [ ! -e "$1/site/v1/media" ]' _ "$work/false2"
if [ -n "${BASELINE_REF:-}" ] && [ -d "$work/base" ]; then
  publish "second publication with the tools of $BASELINE_REF" "$work/base" index "$work/baseline2"
  check "... the same site as the tools of $BASELINE_REF build" same "$work/baseline2" "$work/false2"
fi

# --- 5. refusals before anything is signed --------------------------------
refused "REGISTRY_STOREFRONT_ENABLED=yes is refused" "must be true, false or unset" \
  "$repo" index "$work/bad-flag" REGISTRY_STOREFRONT_ENABLED=yes
site_storefront "$repo"
cp "$repo/registry/plugins/deckpilot.example-default/listing.json" "$work/listing.json"
jq '.links.support = "http://example.com/support"' "$work/listing.json" \
  > "$repo/registry/plugins/deckpilot.example-default/listing.json"
refused "a listing the storefront check refuses: nothing is signed" "the storefront sources have 1 problem" \
  "$repo" index "$work/bad-listing" REGISTRY_STOREFRONT_ENABLED=true
check "... no index was built or signed (the live seq is 1)" test ! -e "$work/bad-listing/site/v1/index/2.json"
cp "$work/listing.json" "$repo/registry/plugins/deckpilot.example-default/listing.json"

# --- 2. enabled ---------------------------------------------------------
publish "publication with the storefront (live seq 1)" "$repo" index "$work/on" REGISTRY_STOREFRONT_ENABLED=true
on=$work/on/site/v1
check "seq 2 has storefront/2.json and its signature" test -f "$on/storefront/2.json" -a -f "$on/storefront/2.json.minisig"
check "the storefront is bound to index/2.json" bash -c 'jq -e --arg sha "$(shasum -a 256 "$1/index/2.json" | cut -d" " -f1)" ".seq == 2 and .index.path == \"index/2.json\" and .index.sha256 == \$sha" "$1/storefront/2.json" > /dev/null' _ "$on"
check "one listing, the editorial document and three images" bash -c '[ "$(jq ".listings | length" "$1/storefront/2.json")" = 1 ] && jq -e .editorial "$1/storefront/2.json" > /dev/null && [ "$(ls "$1/media" | wc -l | tr -d " ")" = 3 ]' _ "$on"
check "the draft section is not signed" bash -c '! grep -q "Draft, never signed" "$1"/storefront/editorial/*.json' _ "$on"
check "the client accepted and recorded it (registry-storefront.json in the state)" \
  bash -c 'jq -e --arg sha "$(shasum -a 256 "$1/storefront/2.json" | cut -d" " -f1)" ".seq == 2 and .sha256 == \$sha" "$2/registry-storefront.json" > /dev/null' _ "$on" "$repo.work/client-state"
check "the publication log says so" grep -q "storefront accepted by the client: seq 2" "$work/on.log"
# The signature file is base64 of the minisign text (the registry's format).
check "signed with the storefront comment, not the index's" bash -c 'base64 -d < "$1" | grep -qx "trusted comment: deckpilot-registry v1 role:storefront seq:2 keys:1"' _ "$on/storefront/2.json.minisig"
# The live site with a storefront: the mirror takes all of it.
site_serve "$work/on"
rm -rf "$work/mirror"
if "$repo/tools/mirror-site.sh" "$site_base" "$work/mirror" > /dev/null 2> "$work/mirror.log" &&
  cmp -s <(site_files "$on") <(site_files "$work/mirror/site/v1"); then
  ok "the mirror of that live site is every file of it, byte for byte"
else bad "the mirror differs: $(tail -2 "$work/mirror.log")"; fi
if "$site_cli" verify-site --dir "$work/mirror" --roots "$repo/keys/roots.json" --require-storefront > "$work/verify.log" 2>&1; then
  ok "verify-site --require-storefront accepts the mirror with the committed roots"
else bad "verify-site on the mirror: $(tail -2 "$work/verify.log")"; fi

# --- 7. verify-published ---------------------------------------------------
if site_verify_published "$repo" "$on/timestamp.json" "$work/vp.log" REGISTRY_STOREFRONT_ENABLED=true &&
  grep -q "storefront accepted by the client: seq 2" "$work/vp.log" &&
  grep -q "the live storefront/ and media/ are the signed ones: 7 file(s)" "$work/vp.log"; then
  ok "verify-published: the live storefront is the signed one and the client accepts it"
else bad "verify-published: $(tail -3 "$work/vp.log" | tr '\n' ' ')"; fi
cp -R "$work/on" "$work/on-signed"
printf 'x' >> "$(ls "$work"/on-signed/site/v1/media/*.png | head -1)"
if site_verify_published "$repo" "$work/on-signed/site/v1/timestamp.json" "$work/vp-bad.log" REGISTRY_STOREFRONT_ENABLED=true; then
  bad "verify-published accepts a live image that is not the signed one"
elif grep -q "are not the signed ones" "$work/vp-bad.log"; then ok "verify-published: a live image other than the signed one fails"
else bad "verify-published (other image): $(tail -2 "$work/vp-bad.log" | tr '\n' ' ')"; fi
rm -rf "$work/on-signed"
site_serve "$work/false"
if site_verify_published "$repo" "$work/false/site/v1/timestamp.json" "$work/vp-none.log" REGISTRY_STOREFRONT_ENABLED=true; then
  bad "verify-published accepts an enabled publication without a storefront"
elif grep -q "has no storefront/1.json" "$work/vp-none.log"; then ok "verify-published: enabled, but no storefront signed, fails"
else bad "verify-published (none): $(tail -2 "$work/vp-none.log" | tr '\n' ' ')"; fi
if site_verify_published "$repo" "$work/false/site/v1/timestamp.json" "$work/vp-off.log"; then
  ok "verify-published: disabled and no storefront, as before"
else bad "verify-published (disabled): $(tail -2 "$work/vp-off.log" | tr '\n' ' ')"; fi
site_serve "$work/on"

# --- 3. timestamp refresh -------------------------------------------------
for flag in "" false true; do
  publish "timestamp refresh (variable '${flag:-unset}')" "$repo" timestamp "$work/ts$flag" ${flag:+REGISTRY_STOREFRONT_ENABLED=$flag}
  ts=$work/ts$flag/site/v1
  check "... the same seq" test "$(seq_of "$work/ts$flag")" = 2
  check "... index/, plugins/, storefront/ and media/ byte for byte" bash -c 'for d in index plugins storefront media; do diff -r "$1/$d" "$2/$d" > /dev/null || exit 1; done' _ "$on" "$ts"
  check "... only the timestamp differs" bash -c 'diff <(cd "$1" && find . -type f ! -name "timestamp.json*" | sort) <(cd "$2" && find . -type f ! -name "timestamp.json*" | sort) > /dev/null' _ "$on" "$ts"
  check "... verified with the storefront and its record" grep -q "storefront accepted by the client: seq 2" "$work/ts$flag.log"
done

# --- 4. revert ----------------------------------------------------------
git -C "$repo" rm -q -r editorial "registry/plugins/deckpilot.example-default/listing.json" \
  "registry/plugins/deckpilot.example-default/media"
git -C "$repo" -c user.name=t -c user.email=t@t commit -q -m "Revert the storefront content"
publish "revert: publication (live seq 2)" "$repo" index "$work/revert" REGISTRY_STOREFRONT_ENABLED=true
rv=$work/revert/site/v1
check "revert: a higher seq" test "$(seq_of "$work/revert")" = 3
check "revert: its storefront has no listing and no editorial" \
  bash -c 'jq -e ".seq == 3 and (.listings | length) == 0 and (has(\"editorial\") | not)" "$1/storefront/3.json" > /dev/null' _ "$rv"
check "revert: the client accepted storefront/3.json" grep -q "storefront accepted by the client: seq 3, sha256 [0-9a-f]*, 0 listing(s), 0 section(s)" "$work/revert.log"
check "revert: storefront/2.json and its files are only the kept index 2's" \
  bash -c 'cmp -s "$1/storefront/2.json" "$2/storefront/2.json" && diff -r "$1/media" "$2/media" > /dev/null' _ "$on" "$rv"

# --- 6. disabled again --------------------------------------------------
site_serve "$work/revert"
publish "disabled again: publication (live seq 3)" "$repo" index "$work/off" REGISTRY_STOREFRONT_ENABLED=false
check "disabled again: seq 4 without storefront/ or media/" \
  bash -c '[ "$(jq .seq "$1/site/v1/timestamp.json")" = 4 ] && [ ! -e "$1/site/v1/storefront" ] && [ ! -e "$1/site/v1/media" ]' _ "$work/off"

echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
