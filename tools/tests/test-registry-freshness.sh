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
#     published release missing from the index, drafts ignored.
# Regression tests of the launch failures: a redeploy that could replace
# the signed registry with a stale CDN copy, or with no registry at all.
#
#   tools/tests/test-registry-freshness.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/registry-freshness.XXXXXX")
server_pid=""
cleanup() { if [ -n "$server_pid" ]; then kill "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; fi; rm -rf "$work"; }
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

echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
