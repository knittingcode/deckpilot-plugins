#!/usr/bin/env bash
# Tests of the registry freshness guards, offline:
#   - tools/mirror-site.sh with REGISTRY_MIN_STATE against a loopback HTTP
#     server standing in for Pages: a live timestamp older than the last
#     verified deployment (a cached copy) or a missing one is waited for and
#     then refused; an equal or newer one is mirrored;
#   - tools/registry-state.sh with a gh stand-in: fetch with and without a
#     branch, record (first and appended), refusal to record an older state,
#     API failures;
#   - tools/check-freshness.sh: enough validity, too little, expired, a
#     published release missing from the index, drafts ignored; a storefront
#     whose images or documents are not all served as pinned.
#   - the storefront in the mirror (docs/storefront.md): the storefront of
#     every kept index, its documents and images, byte for byte; the path
#     grammar refuses media/../x, uppercase hex and .svg, and a pinned file
#     that is missing or has other bytes fails the mirror.
# Regression tests of the launch failures: a redeploy that could replace
# the signed registry with a stale CDN copy, or with no registry at all.
#
# --with-cli (needs tools/build-cli.sh; validate.yml's registry job): also a
# real timestamp-only refresh (tools/publish-site.sh timestamp, throwaway
# keys) of a signed site with a storefront keeps storefront/ and media/
# byte for byte, and its mirror (what the refresh deploys) too.
#
#   tools/tests/test-registry-freshness.sh [--with-cli]
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/registry-freshness.XXXXXX")
with_cli=0
case ${1:-} in --with-cli) with_cli=1 ;; "") ;; *) echo "usage: test-registry-freshness.sh [--with-cli]" >&2; exit 2 ;; esac
server_pid=""
cleanup() {
  if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi
  if declare -F site_stop > /dev/null; then site_stop; fi
  rm -rf "$work"
}
trap cleanup EXIT
export DECKPILOT_PLUGINS_WORK=$work/dot-work
passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }

# --- a loopback "Pages" serving $work/www -------------------------------
www=$work/www
mkdir -p "$www/v1/keys" "$www/v1/index"
port=$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
(cd "$www" && exec python3 -I -m http.server --bind 127.0.0.1 "$port" > /dev/null 2>&1) &
server_pid=$!
base=http://127.0.0.1:$port
for _ in $(seq 1 50); do curl -s -o /dev/null "$base/" && break; sleep 0.1; done

stamp() { # stamp SEQ SIGNED_AT EXPIRES: write a live timestamp naming index/SEQ.json
  jq -n --argjson seq "$1" --arg s "$2" --arg e "${3:-2099-01-01T00:00:00Z}" \
    '{_type: "deckpilot-registry-timestamp", spec_version: 1, seq: $seq, keys_version: 1, signed_at: $s, expires: $e,
      index: {path: "index/\($seq).json", sha256: "00", length: 1}}' > "$www/v1/timestamp.json"
  echo sig > "$www/v1/timestamp.json.minisig"
}
state() { # state SEQ SIGNED_AT: a recorded state
  jq -n --argjson seq "$1" --arg s "$2" '{seq: $seq, signed_at: $s, expires: "2099-01-01T00:00:00Z", timestamp_sha256: "00", run: 1, attempt: 1}' > "$work/state.json"
}
echo '{}' > "$www/v1/keys/1.json"; echo sig > "$www/v1/keys/1.json.minisig"
for s in 2 3 4; do echo '{"plugins": []}' > "$www/v1/index/$s.json"; echo sig > "$www/v1/index/$s.json.minisig"; done

mirror() { # mirror [MIN_STATE]: prints the mirror's answer; status of the mirror
  rm -rf "$work/m"
  REGISTRY_FRESHNESS_WAIT=${WAIT:-0} REGISTRY_FRESHNESS_ATTEMPTS=${ATTEMPTS:-2} REGISTRY_MIN_STATE=${1:-} \
    "$root/tools/mirror-site.sh" "$base" "$work/m" 2> "$work/err"
}
expect_mirror() { # expect_mirror NAME WANT(live|none|fail) [MIN_STATE] [ERROR_TEXT]
  local name=$1 want=$2 got
  if got=$(mirror "${3:-}"); then :; else got=fail; fi
  if [ "$got" != "$want" ]; then bad "$name: got '$got', want '$want' ($(tail -1 "$work/err"))"; return; fi
  if [ -n "${4:-}" ] && ! grep -q -- "$4" "$work/err"; then bad "$name: error lacks '$4': $(tail -1 "$work/err")"; return; fi
  ok "$name"
}

stamp 3 2026-11-18T08:13:05.707494018Z
expect_mirror "no floor: the live registry is mirrored" live
state 3 2026-11-18T08:13:05.707494018Z
expect_mirror "the live timestamp is the recorded one" live "$work/state.json"
state 3 2026-11-17T23:00:03.065623453Z
expect_mirror "the live timestamp is newer than the recorded one" live "$work/state.json"
state 2 2026-11-19T00:00:00Z
expect_mirror "a lower recorded seq is older, whatever its signed_at" live "$work/state.json"
state 3 2026-11-18T08:13:05.707494019Z
expect_mirror "a cached timestamp one nanosecond older is refused" fail "$work/state.json" "an older registry than the last verified deployment"
state 3 2026-11-18T08:13:06Z
expect_mirror "fractions compare as nanoseconds, not as text" fail "$work/state.json" "an older registry"
stamp 3 2026-11-18T08:13:05.7Z
state 3 2026-11-18T08:13:05.65Z
expect_mirror "a shorter fraction can be the newer one" live "$work/state.json"
stamp 3 2026-11-18T08:13:05.707494018Z
state 4 2026-11-18T09:00:00Z
expect_mirror "a cached copy at a lower seq is refused (a new index would reuse seq 4)" fail "$work/state.json" "an older registry"

# The CDN catches up while the mirror waits: it then mirrors the new one.
state 4 2026-11-18T09:00:00Z
( sleep 1.5; stamp 4 2026-11-18T09:00:00Z ) &
swap_pid=$!
if got=$(WAIT=1 ATTEMPTS=6 mirror "$work/state.json") && [ "$got" = live ] &&
  [ "$(jq .seq "$work/m/site/v1/timestamp.json")" = 4 ] && grep -q "attempt 1 of 6" "$work/err"; then
  ok "a stale copy is waited for until the live site serves the recorded state"
else
  bad "waiting for the CDN: got '${got:-}' ($(tail -1 "$work/err"))"
fi
wait "$swap_pid"

rm "$www/v1/timestamp.json" "$www/v1/timestamp.json.minisig"
expect_mirror "no timestamp and no floor: no registry was ever published" none
expect_mirror "no timestamp although a registry was verified: refused (no site without the registry)" fail "$work/state.json" "serves no registry"
stamp 4 2026-11-18T09:00:00Z
if REGISTRY_MIN_STATE=$work/absent.json "$root/tools/mirror-site.sh" "$base" "$work/m" > /dev/null 2> "$work/err"; then
  bad "a missing REGISTRY_MIN_STATE file is accepted"
else ok "a missing REGISTRY_MIN_STATE file fails"; fi

# --- the storefront in the mirror ------------------------------------------
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
len() { wc -c < "$1" | tr -d ' '; }
printf 'icon bytes' > "$work/icon"; icon_sha=$(sha "$work/icon")
printf 'banner bytes' > "$work/banner"; banner_sha=$(sha "$work/banner")
# sf [ICON_SHA] [ICON_FORMAT] [EDITORIAL_PATH]: a live storefront of seq 4
# and of seq 3 (unsigned: the mirror verifies hashes, verify-site the rest),
# a listing whose icon is ICON_SHA.ICON_FORMAT and an editorial document
# whose banner is a WebP image.
sf() {
  local isha=${1:-$icon_sha} ifmt=${2:-png} epath=${3:-} lsha esha
  rm -rf "$www/v1/storefront" "$www/v1/media"
  mkdir -p "$www/v1/media" "$www/v1/storefront/editorial" "$www/v1/storefront/listings/deckpilot.memory"
  cp "$work/icon" "$www/v1/media/$isha.$ifmt"
  cp "$work/banner" "$www/v1/media/$banner_sha.webp"
  jq -n --arg s "$isha" --arg f "$ifmt" --argjson n "$(len "$work/icon")" \
    '{_type: "deckpilot-registry-listing", listing_version: 1, id: "deckpilot.memory",
      icon: {sha256: $s, length: $n, format: $f, width: 512, height: 512}}' > "$work/l.json"
  lsha=$(sha "$work/l.json")
  cp "$work/l.json" "$www/v1/storefront/listings/deckpilot.memory/$lsha.json"
  jq -n --arg s "$banner_sha" --argjson n "$(len "$work/banner")" \
    '{_type: "deckpilot-registry-editorial", editorial_version: 1, sections: [{kind: "banner_carousel",
      items: [{artwork: {asset: {sha256: $s, length: $n, format: "webp", width: 1800, height: 600}}}]}]}' > "$work/e.json"
  esha=$(sha "$work/e.json")
  cp "$work/e.json" "$www/v1/storefront/editorial/$esha.json"
  for s in 3 4; do
    jq -n --argjson seq "$s" --arg l "storefront/listings/deckpilot.memory/$lsha.json" --arg lsha "$lsha" \
      --argjson ln "$(len "$work/l.json")" --arg e "${epath:-storefront/editorial/$esha.json}" --arg esha "$esha" \
      --argjson en "$(len "$work/e.json")" \
      '{_type: "deckpilot-registry-storefront", seq: $seq, listings: [{id: "deckpilot.memory", ref: {path: $l, sha256: $lsha, length: $ln}}],
        editorial: {path: $e, sha256: $esha, length: $en}}' > "$www/v1/storefront/$s.json"
    echo sig > "$www/v1/storefront/$s.json.minisig"
  done
}
same_storefront() { # the mirror holds exactly the live storefront/ and media/
  diff -r "$www/v1/storefront" "$work/m/site/v1/storefront" > /dev/null && diff -r "$www/v1/media" "$work/m/site/v1/media" > /dev/null
}
sf
expect_mirror "a live storefront is mirrored" live
if same_storefront; then ok "... storefront/ and media/ byte for byte (both kept indexes' storefronts, documents, images)"
else bad "the mirrored storefront differs from the live one"; fi
sf "" "" "media/../x"
expect_mirror "the grammar refuses media/../x" fail "" "refusing to mirror media/../x"
sf "$(tr 'a-f' 'A-F' <<< "$icon_sha")"
expect_mirror "the grammar refuses uppercase hex" fail "" "refusing to mirror media/$(tr 'a-f' 'A-F' <<< "$icon_sha").png"
sf "" svg
expect_mirror "the grammar refuses .svg" fail "" "refusing to mirror media/$icon_sha.svg"
sf; printf 'other bytes' > "$www/v1/media/$icon_sha.png"
expect_mirror "an image with other bytes than pinned fails" fail "" "is not the file its storefront pins"
sf; rm "$www/v1/media/$banner_sha.webp"
expect_mirror "a pinned image missing on the live site fails" fail "" "media/$banner_sha.webp is missing on the live site"
sf; rm "$www/v1/storefront/4.json.minisig"
expect_mirror "a storefront without its signature fails" fail "" "storefront/4.json.minisig is missing"
sf; rm "$www/v1/storefront/3.json" "$www/v1/storefront/3.json.minisig"
expect_mirror "an index without a storefront is mirrored without one" live
sf

# --- tools/registry-state.sh with a gh stand-in ----------------------------
mkdir -p "$work/bin"
cat > "$work/bin/gh" << 'STUB'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ] || exit 2; shift
method=GET q=. endpoint="" body="" paginate=""
while [ $# -gt 0 ]; do
  case "$1" in -X) method=$2; shift 2 ;; --jq) q=$2; shift 2 ;; --input) body=$(cat); shift 2 ;; --paginate) shift ;; *) endpoint=$1; shift ;; esac
done
echo "$method $endpoint" >> "$STUB/calls"
if [ -n "${STUB_FAIL:-}" ] && [[ "$method $endpoint" =~ $STUB_FAIL ]]; then echo "gh: Server Error (HTTP 502)" >&2; exit 1; fi
case "$method $endpoint" in
  "GET repos/o/r/git/ref/heads/registry-state")
    [ -f "$STUB/head" ] || { echo "gh: Not Found (HTTP 404)" >&2; exit 1; }
    jq -n --arg s "$(cat "$STUB/head")" '{object: {sha: $s}}' | jq -r "$q" ;;
  "GET repos/o/r/contents/state.json?ref="*)
    jq -n --arg c "$(base64 < "$STUB/state.json")" '{content: $c}' | jq -r "$q" ;;
  "POST repos/o/r/git/trees") echo "$body" > "$STUB/tree.json"; echo '{"sha":"2222222222222222222222222222222222222222"}' | jq -r "$q" ;;
  "POST repos/o/r/git/commits") echo "$body" > "$STUB/commit.json"; echo '{"sha":"3333333333333333333333333333333333333333"}' | jq -r "$q" ;;
  "PATCH repos/o/r/git/refs/heads/registry-state") echo "$body" > "$STUB/patch.json"; echo '{}' ;;
  "POST repos/o/r/git/refs") echo "$body" > "$STUB/create.json"; echo '{}' ;;
  "GET repos/o/r/releases?per_page=100") cat "$STUB/releases.json" | jq -r "$q" ;;
  *) echo "unexpected $method $endpoint" >&2; exit 2 ;;
esac
STUB
chmod +x "$work/bin/gh"
export GH=$work/bin/gh GH_TOKEN=t GITHUB_REPOSITORY=o/r GITHUB_RUN_ID=77 GITHUB_RUN_ATTEMPT=1
fresh_stub() { rm -rf "$work/s"; mkdir -p "$work/s"; export STUB=$work/s; }

fresh_stub
if "$root/tools/registry-state.sh" fetch "$work/out.json" 2> "$work/err" && [ ! -e "$work/out.json" ]; then
  ok "fetch without the branch: no state, no failure"
else bad "fetch without the branch ($(tail -1 "$work/err"))"; fi

fresh_stub; echo 1111111111111111111111111111111111111111 > "$STUB/head"; state 3 2026-11-18T08:13:05.707494018Z; cp "$work/state.json" "$STUB/state.json"
if "$root/tools/registry-state.sh" fetch "$work/out.json" 2> "$work/err" && cmp -s "$work/out.json" "$work/state.json"; then
  ok "fetch: the recorded state.json"
else bad "fetch with a state ($(tail -1 "$work/err"))"; fi

fresh_stub; echo 1111111111111111111111111111111111111111 > "$STUB/head"; echo '{"seq": "x"}' > "$STUB/state.json"
if "$root/tools/registry-state.sh" fetch "$work/out.json" 2> "$work/err"; then bad "fetch accepts an invalid state.json"
else ok "fetch refuses an invalid state.json"; fi

fresh_stub
if STUB_FAIL='git/ref/heads' "$root/tools/registry-state.sh" fetch "$work/out.json" 2> "$work/err"; then bad "fetch ignores an API failure"
else ok "fetch fails on an API failure other than 404"; fi

stamp 4 2026-11-18T09:00:00.5Z
fresh_stub
if "$root/tools/registry-state.sh" record "$www/v1/timestamp.json" 2> "$work/err" &&
  [ "$(jq -r .ref "$STUB/create.json")" = refs/heads/registry-state ] &&
  [ "$(jq -r '.parents | length' "$STUB/commit.json")" = 0 ] &&
  jq -e --arg sha "$(shasum -a 256 "$www/v1/timestamp.json" | cut -d' ' -f1)" \
    '.tree[0].path == "state.json" and (.tree[0].content | fromjson | .seq == 4 and .signed_at == "2026-11-18T09:00:00.5Z" and .timestamp_sha256 == $sha and .run == 77)' "$STUB/tree.json" > /dev/null; then
  ok "record: the first state starts registry-state"
else bad "record without the branch ($(tail -1 "$work/err"))"; fi

fresh_stub; echo 1111111111111111111111111111111111111111 > "$STUB/head"; state 3 2026-11-18T08:13:05Z; cp "$work/state.json" "$STUB/state.json"
if "$root/tools/registry-state.sh" record "$www/v1/timestamp.json" 2> "$work/err" &&
  [ "$(jq -r '.parents[0]' "$STUB/commit.json")" = 1111111111111111111111111111111111111111 ] &&
  [ "$(jq -r '.force' "$STUB/patch.json")" = false ]; then
  ok "record: a newer state is appended (no force)"
else bad "record appending ($(tail -1 "$work/err"))"; fi

fresh_stub; echo 1111111111111111111111111111111111111111 > "$STUB/head"; state 4 2026-11-18T09:00:01Z; cp "$work/state.json" "$STUB/state.json"
if "$root/tools/registry-state.sh" record "$www/v1/timestamp.json" 2> "$work/err" || [ -e "$STUB/commit.json" ]; then
  bad "record accepts a state older than the recorded one"
else ok "record refuses a state older than the recorded one"; fi

fresh_stub; echo 1111111111111111111111111111111111111111 > "$STUB/head"; state 3 2026-11-18T08:13:05Z; cp "$work/state.json" "$STUB/state.json"
if STUB_FAIL='PATCH' "$root/tools/registry-state.sh" record "$www/v1/timestamp.json" 2> "$work/err"; then
  bad "record ignores a failure to advance the branch"
else ok "record fails when the branch cannot be advanced"; fi

# --- tools/check-freshness.sh ---------------------------------------------
detail_sha=$(printf '%064d' 7)
mkdir -p "$www/v1/plugins/deckpilot.memory"
jq -n '{versions: [{version: "0.1.0"}, {version: "0.2.0"}]}' > "$www/v1/plugins/deckpilot.memory/$detail_sha.json"
jq -n --arg d "plugins/deckpilot.memory/$detail_sha.json" '{plugins: [{id: "deckpilot.memory", detail: {path: $d}}]}' > "$www/v1/index/4.json"
stamp 4 2026-11-18T09:00:00Z 2026-11-25T09:00:00.25Z
releases() { jq -n "$1" > "$STUB/releases.json"; }
monitor() { # monitor NOW: status of the monitor
  NOW=$(jq -rn --arg t "$1" '$t | fromdateiso8601') "$root/tools/check-freshness.sh" "$base" 5 > /dev/null 2> "$work/err"
}
fresh_stub
releases '[{tag_name: "deckpilot.memory--v0.2.0", draft: false}, {tag_name: "deckpilot.memory--v0.1.0", draft: false}, {tag_name: "registry--v2", draft: false}]'
if monitor 2026-11-18T10:00:00Z; then ok "monitor: 7 days left, every release listed"; else bad "monitor healthy ($(tail -1 "$work/err"))"; fi
if monitor 2026-11-20T10:00:00Z; then bad "monitor accepts 4 days 23 hours left"
elif grep -q "less than 5 days" "$work/err"; then ok "monitor: less than 5 days left fails"; else bad "monitor near expiry: $(tail -1 "$work/err")"; fi
if monitor 2026-11-26T00:00:00Z; then bad "monitor accepts an expired timestamp"
elif grep -q "EXPIRED" "$work/err"; then ok "monitor: an expired timestamp fails"; else bad "monitor expired: $(tail -1 "$work/err")"; fi
releases '[{tag_name: "deckpilot.memory--v0.3.0", draft: false}, {tag_name: "deckpilot.memory--v0.2.0", draft: false}]'
if monitor 2026-11-18T10:00:00Z; then bad "monitor misses an unpublished release"
elif grep -q "deckpilot.memory--v0.3.0 is not in the live index" "$work/err"; then ok "monitor: a release missing from the index fails"; else bad "monitor missing release: $(tail -1 "$work/err")"; fi
releases '[{tag_name: "deckpilot.memory--v0.3.0", draft: true}, {tag_name: "deckpilot.memory--v0.2.0", draft: false}]'
if monitor 2026-11-18T10:00:00Z; then ok "monitor: a draft release is not expected in the index"; else bad "monitor draft ($(tail -1 "$work/err"))"; fi

# The storefront of the live seq (seq 4, from the mirror tests above).
releases '[{tag_name: "deckpilot.memory--v0.2.0", draft: false}]'
if monitor 2026-11-18T10:00:00Z && grep -q "storefront, if any, is served as pinned" "$work/err"; then
  ok "monitor: the storefront and every file it pins served as pinned"
else bad "monitor with a storefront ($(tail -1 "$work/err"))"; fi
mv "$www/v1/media/$banner_sha.webp" "$work/banner.moved"
if monitor 2026-11-18T10:00:00Z; then bad "monitor misses a dropped image"
elif grep -q "media/$banner_sha.webp, which is not served: a deployment dropped it" "$work/err"; then ok "monitor: a dropped image fails"
else bad "monitor dropped image: $(tail -1 "$work/err")"; fi
mv "$work/banner.moved" "$www/v1/media/$banner_sha.webp"
printf '\n' >> "$(ls "$www"/v1/storefront/listings/deckpilot.memory/*.json)"
if monitor 2026-11-18T10:00:00Z; then bad "monitor misses a changed listing"
elif grep -q "is not the file the storefront of seq 4 pins" "$work/err"; then ok "monitor: a changed listing fails"
else bad "monitor changed listing: $(tail -1 "$work/err")"; fi
rm -rf "$www/v1/storefront" "$www/v1/media"
if monitor 2026-11-18T10:00:00Z; then ok "monitor: no storefront is not a problem"; else bad "monitor without a storefront ($(tail -1 "$work/err"))"; fi

# --- --with-cli: a real timestamp refresh of a site with a storefront -------
if [ "$with_cli" = 1 ]; then
  # shellcheck source=lib/registry-site.sh
  . "$root/tools/tests/lib/registry-site.sh"
  site_www=$work/cli-www
  site_start
  repo=$work/cli-repo
  site_repo "$repo"
  site_storefront "$repo"
  if site_publish "$repo" index "$work/cli-on" REGISTRY_STOREFRONT_ENABLED=true; then ok "cli: a signed site with a storefront (seq 1)"
  else bad "cli: publication: $(tail -3 "$work/cli-on.log" | tr '\n' ' ')"; fi
  site_serve "$work/cli-on"
  if site_publish "$repo" timestamp "$work/cli-ts"; then ok "cli: the timestamp refresh (publish-site.sh timestamp)"
  else bad "cli: refresh: $(tail -3 "$work/cli-ts.log" | tr '\n' ' ')"; fi
  on=$work/cli-on/site/v1 ts=$work/cli-ts/site/v1
  if [ -n "$(ls "$ts/media" 2> /dev/null)" ] && diff -r "$on/storefront" "$ts/storefront" > /dev/null && diff -r "$on/media" "$ts/media" > /dev/null; then
    ok "cli: storefront/ and media/ byte for byte after the refresh"
  else bad "cli: the refresh changed storefront/ or media/"; fi
  if diff <(cd "$on" && find . -type f ! -name 'timestamp.json*' | sort | xargs shasum -a 256) \
    <(cd "$ts" && find . -type f ! -name 'timestamp.json*' | sort | xargs shasum -a 256) > /dev/null &&
    [ "$(jq .seq "$ts/timestamp.json")" = "$(jq .seq "$on/timestamp.json")" ] && ! cmp -s "$on/timestamp.json" "$ts/timestamp.json"; then
    ok "cli: only timestamp.json changed (same seq, new signature)"
  else bad "cli: more than the timestamp changed"; fi
  # What _site.yml deploys after the refresh is the mirror of it.
  site_serve "$work/cli-ts"
  rm -rf "$work/cli-m"
  if "$root/tools/mirror-site.sh" "$site_base" "$work/cli-m" > /dev/null 2> "$work/err" &&
    diff -r "$ts" "$work/cli-m/site/v1" > /dev/null; then
    ok "cli: the mirror of the refreshed site is all of it, byte for byte"
  else bad "cli: the mirror of the refreshed site differs ($(tail -1 "$work/err"))"; fi
fi

echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
