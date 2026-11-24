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
#     published release missing from the index, drafts and the releases of
#     plugins DeckPilot bundles only ignored; a storefront
#     whose images or documents are not all served as pinned.
#   - a seq without a storefront: no storefront/ or media/ in the mirror
#     (a 404 leaves no empty directory behind);
#   - the storefront in the mirror (docs/storefront.md): the live seq's
#     storefront, its documents and images, byte for byte, and nothing of an
#     older seq (an unsigned older storefront pinning HTML as an image is
#     never fetched); the path grammar refuses media/../x, uppercase hex
#     and .svg; a pinned file that is missing or has other bytes, an image
#     over 1 MiB, a listing over its 8 MiB, an envelope with too many
#     listings or over 64 KiB fail before anything more is fetched; with a
#     recorded state of the same seq, the live storefront must be the
#     recorded one, or absent when none was;
#   - tools/registry-state.sh records the seq's storefront sha256 (or null)
#     and refuses another one for the same seq.
# Regression tests of the launch failures: a redeploy that could replace
# the signed registry with a stale CDN copy, or with no registry at all.
#
# --with-cli (needs tools/build-cli.sh; validate.yml's registry job): also a
# real timestamp-only refresh (tools/publish-site.sh timestamp, throwaway
# keys) of a signed site without a storefront changes only the timestamp,
# and of one with a storefront keeps storefront/ and media/ byte for byte,
# and its mirror (what the refresh deploys) too.
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
# A seq without a storefront (storefront/3.json is a 404) leaves no trace:
# not even an empty storefront/, which the refresh's byte-for-byte check
# of storefront/ counted as a difference from the refreshed site, so every
# timestamp refresh of the live seq 4 failed from 2026-11-23.
if [ ! -e "$work/m/site/v1/storefront" ] && [ ! -e "$work/m/site/v1/media" ] &&
  [ -z "$(find "$work/m/site/v1" -type d -empty)" ]; then
  ok "no storefront: no storefront/ or media/ and no empty directory in the mirror"
else bad "no storefront: the mirror has $(cd "$work/m/site/v1" && find . -type d | tr '\n' ' ')"; fi
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
# envelope SEQ LISTING_JSON EDITORIAL_JSON [EDITORIAL_PATH]: storefront/SEQ.json
# pinning the two documents (each written content-addressed).
envelope() {
  local seq=$1 l=$2 e=$3 epath=${4:-} lsha esha
  lsha=$(sha "$l") esha=$(sha "$e")
  mkdir -p "$www/v1/storefront/editorial" "$www/v1/storefront/listings/deckpilot.memory"
  cp "$l" "$www/v1/storefront/listings/deckpilot.memory/$lsha.json"
  cp "$e" "$www/v1/storefront/editorial/$esha.json"
  jq -n --argjson seq "$seq" --arg l "storefront/listings/deckpilot.memory/$lsha.json" --arg lsha "$lsha" \
    --argjson ln "$(len "$l")" --arg e "${epath:-storefront/editorial/$esha.json}" --arg esha "$esha" --argjson en "$(len "$e")" \
    '{_type: "deckpilot-registry-storefront", seq: $seq, listings: [{id: "deckpilot.memory", ref: {path: $l, sha256: $lsha, length: $ln}}],
      editorial: {path: $e, sha256: $esha, length: $en}}' > "$www/v1/storefront/$seq.json"
  echo sig > "$www/v1/storefront/$seq.json.minisig"
}
# sf [ICON_SHA] [ICON_FORMAT] [EDITORIAL_PATH]: the live seq's (4) storefront,
# unsigned (the mirror checks hashes and budgets, verify-site the rest): a
# listing whose icon is ICON_SHA.ICON_FORMAT, an editorial document whose
# banner is a WebP image.
sf() {
  local isha=${1:-$icon_sha} ifmt=${2:-png}
  rm -rf "$www/v1/storefront" "$www/v1/media"
  mkdir -p "$www/v1/media"
  cp "$work/icon" "$www/v1/media/$isha.$ifmt"
  cp "$work/banner" "$www/v1/media/$banner_sha.webp"
  jq -n --arg s "$isha" --arg f "$ifmt" --argjson n "$(len "$work/icon")" \
    '{_type: "deckpilot-registry-listing", listing_version: 1, id: "deckpilot.memory",
      icon: {sha256: $s, length: $n, format: $f, width: 512, height: 512}}' > "$work/l.json"
  jq -n --arg s "$banner_sha" --argjson n "$(len "$work/banner")" \
    '{_type: "deckpilot-registry-editorial", editorial_version: 1, sections: [{kind: "banner_carousel",
      items: [{artwork: {asset: {sha256: $s, length: $n, format: "webp", width: 1800, height: 600}}}]}]}' > "$work/e.json"
  envelope 4 "$work/l.json" "$work/e.json" "${3:-}"
}
same_storefront() { # the mirror holds exactly the live seq's storefront/ and media/
  diff -r "$www/v1/storefront" "$work/m/site/v1/storefront" > /dev/null && diff -r "$www/v1/media" "$work/m/site/v1/media" > /dev/null
}
sf
expect_mirror "the live seq's storefront is mirrored" live
if same_storefront; then ok "... storefront/ and media/ byte for byte (envelope, signature, documents, images)"
else bad "the mirrored storefront differs from the live one"; fi
# The review's proof of concept: an unsigned storefront of an older kept
# seq, with a garbage signature, pinning HTML bytes as an image. The mirror
# never fetches it, so nothing deploys it again.
printf '<html><script>alert(1)</script></html>' > "$work/html"; html_sha=$(sha "$work/html")
cp "$work/html" "$www/v1/media/$html_sha.png"
jq -n --arg s "$html_sha" --argjson n "$(len "$work/html")" \
  '{_type: "deckpilot-registry-listing", listing_version: 1, id: "deckpilot.memory",
    icon: {sha256: $s, length: $n, format: "png", width: 512, height: 512}}' > "$work/evil-l.json"
cp "$work/e.json" "$work/evil-e.json"
mv "$www/v1/storefront/4.json" "$work/keep4.json"
envelope 3 "$work/evil-l.json" "$work/evil-e.json"
mv "$work/keep4.json" "$www/v1/storefront/4.json"; echo sig > "$www/v1/storefront/4.json.minisig"
echo garbage > "$www/v1/storefront/3.json.minisig"
expect_mirror "an older seq's unsigned storefront (the review's PoC)" live
if [ ! -e "$work/m/site/v1/storefront/3.json" ] && [ ! -e "$work/m/site/v1/media/$html_sha.png" ] &&
  [ ! -e "$work/m/site/v1/storefront/listings/deckpilot.memory/$(sha "$work/evil-l.json").json" ]; then
  ok "... is not mirrored: neither its envelope, its document nor the HTML it pins as an image"
else bad "the mirror took an older seq's storefront or what it pins"; fi
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
# Budgets, on the declared lengths, before anything is fetched.
bomb_listing() { # bomb_listing JQ: the live listing document, edited
  sf; jq "$1" "$work/l.json" > "$work/bomb.json"; envelope 4 "$work/bomb.json" "$work/e.json"
}
bomb_listing '.icon.length = 2097152'
expect_mirror "an image declared over 1 MiB is refused" fail "" "over its 1048576-byte limit"
bomb_listing '.screenshots = [range(9) | {asset: {sha256: ("\(.)" * 64)[0:64] | gsub("[^0-9]"; "a"), length: 1048576, format: "png", width: 1280, height: 800}}]'
expect_mirror "a listing pinning more than 8 MiB of images is refused before any fetch" fail "" "over its 8388608-byte budget"
sf; jq -c '.listings = [range(257) | {id: "deckpilot.memory", ref: {path: "storefront/listings/deckpilot.memory/\("0" * 64).json", sha256: ("0" * 64), length: 10}}]' \
  "$www/v1/storefront/4.json" > "$work/many.json" && mv "$work/many.json" "$www/v1/storefront/4.json"
expect_mirror "an envelope with more than 256 listings is refused" fail "" "more than 256 listings"
sf; head -c 70000 /dev/zero | tr '\0' ' ' >> "$www/v1/storefront/4.json"
expect_mirror "an envelope over 64 KiB is refused at once" fail "" "storefront/4.json is over its 65536-byte limit"
# The last verified deployment's storefront (state.json storefront_sha256).
sf
state_sf() { # state_sf SHA|null: a recorded state of seq 4 with that storefront
  jq -n --argjson f "$1" '{seq: 4, signed_at: "2026-11-18T09:00:00Z", expires: "2099-01-01T00:00:00Z",
    timestamp_sha256: "00", storefront_sha256: $f, run: 1, attempt: 1}' > "$work/state.json"
}
state_sf "\"$(sha "$www/v1/storefront/4.json")\""
expect_mirror "state: the recorded storefront is the live one" live "$work/state.json"
state_sf "\"$(printf '%064d' 5)\""
expect_mirror "state: a live storefront other than the recorded one is refused" fail "$work/state.json" "but the last verified deployment of seq 4 had sha256"
state_sf null
expect_mirror "state: a live storefront where none was deployed is refused" fail "$work/state.json" "had none"
state_sf "\"$(sha "$www/v1/storefront/4.json")\""
mv "$www/v1/storefront/4.json" "$work/keep4.json"
expect_mirror "state: a dropped storefront is refused" fail "$work/state.json" "storefront/4.json is absent"
mv "$work/keep4.json" "$www/v1/storefront/4.json"
state 4 2026-11-18T09:00:00Z
expect_mirror "state: a state that predates storefronts checks nothing" live "$work/state.json"
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

# The storefront of the recorded seq (storefront/<seq>.json beside the
# timestamp): its sha256, or null; the same seq never gets another one.
mkdir -p "$work/rec/storefront"; cp "$www/v1/timestamp.json" "$work/rec/timestamp.json"
fresh_stub
"$root/tools/registry-state.sh" record "$work/rec/timestamp.json" 2> "$work/err" &&
  jq -e '.tree[0].content | fromjson | .storefront_sha256 == null' "$STUB/tree.json" > /dev/null &&
  ok "record: no storefront is recorded as null" || bad "record without a storefront ($(tail -1 "$work/err"))"
echo '{"seq": 4}' > "$work/rec/storefront/4.json"
fresh_stub
"$root/tools/registry-state.sh" record "$work/rec/timestamp.json" 2> "$work/err" &&
  jq -e --arg s "$(sha "$work/rec/storefront/4.json")" '.tree[0].content | fromjson | .storefront_sha256 == $s' "$STUB/tree.json" > /dev/null &&
  ok "record: the seq's storefront sha256" || bad "record with a storefront ($(tail -1 "$work/err"))"
fresh_stub; echo 1111111111111111111111111111111111111111 > "$STUB/head"
jq -n '{seq: 4, signed_at: "2026-11-18T09:00:00Z", expires: "2099-01-01T00:00:00Z", timestamp_sha256: "00", storefront_sha256: null, run: 1, attempt: 1}' > "$STUB/state.json"
if "$root/tools/registry-state.sh" record "$work/rec/timestamp.json" 2> "$work/err" || [ -e "$STUB/commit.json" ]; then
  bad "record accepts another storefront for a recorded seq"
elif grep -q "the recorded state of seq 4 has null" "$work/err"; then ok "record refuses another storefront for the same seq"
else bad "record same seq: $(tail -1 "$work/err")"; fi

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
# The plugins DeckPilot bundles only (official-release.yml; never in the
# registry): the release of deckpilot.docker--v0.1.1 and the four others
# failed every monitor run from 2026-11-22. A bundled plugin the registry
# serves (deckpilot.github) is still expected.
releases '[{tag_name: "deckpilot.docker--v0.1.1", draft: false}, {tag_name: "deckpilot.git--v0.1.1", draft: false}, {tag_name: "deckpilot.projects--v0.1.1", draft: false}, {tag_name: "deckpilot.workflows--v0.1.1", draft: false}, {tag_name: "deckpilot.workspace--v0.1.1", draft: false}, {tag_name: "deckpilot.memory--v0.2.0", draft: false}]'
if monitor 2026-11-18T10:00:00Z; then ok "monitor: a bundled-only plugin's release is not expected in the index"; else bad "monitor bundled-only ($(tail -1 "$work/err"))"; fi
releases '[{tag_name: "deckpilot.github--v0.2.1", draft: false}, {tag_name: "deckpilot.memory--v0.2.0", draft: false}]'
if monitor 2026-11-18T10:00:00Z; then bad "monitor misses a registry-served official plugin's release"
elif grep -q "deckpilot.github--v0.2.1 is not in the live index" "$work/err"; then ok "monitor: a registry-served official plugin's release missing from the index fails"
else bad "monitor official release: $(tail -1 "$work/err")"; fi
releases '[{tag_name: "deckpilot.dockerx--v0.1.1", draft: false}, {tag_name: "deckpilot.memory--v0.2.0", draft: false}]'
if monitor 2026-11-18T10:00:00Z; then bad "monitor exempts an id that only starts with a bundled-only one"
elif grep -q "deckpilot.dockerx--v0.1.1 is not in the live index" "$work/err"; then ok "monitor: only the exact bundled-only ids are exempt"
else bad "monitor id prefix: $(tail -1 "$work/err")"; fi

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
  # The live registry of 2026-11-23: a seq without a storefront. Its
  # refresh failed ("the refreshed site's storefront/ is not the live one
  # byte for byte") on the empty storefront/ the mirror left.
  if site_publish "$repo" index "$work/cli-off"; then ok "cli: a signed site without a storefront (seq 1)"
  else bad "cli: publication without a storefront: $(tail -3 "$work/cli-off.log" | tr '\n' ' ')"; fi
  site_serve "$work/cli-off"
  if site_publish "$repo" timestamp "$work/cli-off-ts"; then ok "cli: the timestamp refresh of a seq without a storefront"
  else bad "cli: refresh without a storefront: $(tail -3 "$work/cli-off-ts.log" | tr '\n' ' ')"; fi
  off=$work/cli-off/site/v1 offts=$work/cli-off-ts/site/v1
  if [ ! -e "$offts/storefront" ] && [ ! -e "$offts/media" ] &&
    diff <(cd "$off" && find . -type f ! -name 'timestamp.json*' | sort | xargs shasum -a 256) \
      <(cd "$offts" && find . -type f ! -name 'timestamp.json*' | sort | xargs shasum -a 256) > /dev/null &&
    [ "$(jq .seq "$offts/timestamp.json")" = 1 ] && ! cmp -s "$off/timestamp.json" "$offts/timestamp.json"; then
    ok "cli: ... only timestamp.json changed, no storefront/ or media/"
  else bad "cli: the refresh without a storefront changed more than the timestamp"; fi
  site_serve "$work/cli-off-ts"
  site_storefront "$repo"
  if site_publish "$repo" index "$work/cli-on" REGISTRY_STOREFRONT_ENABLED=true; then ok "cli: a signed site with a storefront (seq 2)"
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
