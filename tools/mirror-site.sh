#!/usr/bin/env bash
# mirror-site.sh BASE_URL DIR
#
# Copies the live signed registry (BASE_URL/v1/...) into DIR/site/v1: every
# key set, the timestamp, the newest index and the 10 before it, and every
# detail those indexes pin; and the storefront of the live seq only, if it
# has one (storefront/<seq>.json and its signature, docs/storefront.md),
# with every listing and editorial document it pins and every image those
# documents pin (media/<sha256>.<png|webp|jpg>), each checked against its
# pinned sha256 and length, within the storefront's budgets. Nothing is
# deployed from this mirror before verify-site --require-storefront has
# accepted that storefront; no other storefront file is ever fetched. Prints `live`
# or `none` (no timestamp served: no registry was ever published). Only the
# registry's own path grammar is fetched, and the result is data:
# tools/publish-site.sh runs `verify-site` on it with the committed roots
# before anything builds on it.
#
# REGISTRY_MIN_STATE (optional): the state.json of the last verified
# deployment (tools/registry-state.sh fetch). Pages' CDN caches every file
# for 10 minutes and ignores query strings and no-cache, so right after a
# deployment an edge can still serve the previous timestamp, or none. A
# mirror older than that state, or a site without a registry, is never
# built on: it would redeploy a registry that clients already moved past
# (they refuse a lower seq), or sign a second index at a seq already used.
# The timestamp is fetched again every REGISTRY_FRESHNESS_WAIT seconds
# (default 60), REGISTRY_FRESHNESS_ATTEMPTS times (default 15), then the
# mirror fails.
. "$(dirname "$0")/lib/common.sh"
base=${1:?usage: mirror-site.sh BASE_URL DIR}
dir=${2:?usage: mirror-site.sh BASE_URL DIR}
site=$dir/site/v1
mkdir -p "$site"
proto='=https'
# Local tests only: a loopback HTTP server standing in for Pages.
[[ $base =~ ^http://127\.0\.0\.1:[0-9]+$ ]] && proto='=http'

# The path grammar: nothing else is ever fetched. The client's documents
# (each with an optional .minisig), the storefront envelope (and its
# .minisig), the content-addressed listing and editorial documents, and
# content-addressed images: lowercase hex, raster formats only (never SVG).
HEX='[0-9a-f]{64}'
ID='[a-z][a-z0-9.-]{3,56}'
GRAMMAR="^((timestamp\.json|keys/[1-9][0-9]*\.json|index/[1-9][0-9]*\.json|plugins/$ID/$HEX\.json|storefront/[1-9][0-9]*\.json)(\.minisig)?|storefront/listings/$ID/$HEX\.json|storefront/editorial/$HEX\.json|media/$HEX\.(png|webp|jpg))$"
# in_grammar PATH: whether the mirror may fetch PATH.
in_grammar() { [[ $1 =~ $GRAMMAR ]]; }

# The storefront's budgets (docs/storefront.md), checked on the declared
# lengths before anything is fetched, so an unsigned document cannot make
# the mirror download more: per file (an envelope or listing 64 KiB, the
# editorial document 128 KiB, an image 1 MiB), per document (a listing's
# images 8 MiB, the editorial's 16 MiB), and in all (256 listings, 1024
# images, 256 MiB).
MAX_LISTINGS=256 MAX_IMAGES=1024 MAX_TOTAL=$((256 * 1048576))
max_bytes() {
  case $1 in
    media/*) echo 1048576 ;;
    storefront/editorial/*) echo 131072 ;;
    storefront/*.minisig) echo 4096 ;;
    storefront/*) echo 65536 ;;
    *) echo 4194304 ;;
  esac
}

# get PATH [optional]: fetch BASE_URL/v1/PATH into the mirror. With
# `optional`, a 404 returns 1; anything else that fails is fatal.
get() {
  local path=$1 code max
  in_grammar "$path" || die "refusing to mirror $path"
  max=$(max_bytes "$path")
  mkdir -p "$(dirname "$site/$path")"
  local attempt
  for attempt in 1 2 3 4 5 6; do
    local rc=0
    code=$(curl -sS --proto "$proto" --max-filesize "$max" -o "$site/$path" -w '%{http_code}' \
      "$base/v1/$path") || rc=$?
    # Over the size limit (curl: maximum file size exceeded) is not transient.
    if [ "$rc" = 63 ]; then rm -f "$site/$path"; die "the live $path is over its $max-byte limit"; fi
    [ "$rc" = 0 ] || code=000
    case $code in
      200) return 0 ;;
      404) rm -f "$site/$path"; [ "${2:-}" = optional ] && return 1; die "$path is missing on the live site" ;;
      # Transient (Pages answers 503 while a deployment propagates, 429 when
      # rate limiting): retry with backoff; anything else fails closed.
      000 | 429 | 5??) rm -f "$site/$path"; note "fetching $path: HTTP $code, retrying"; sleep $((attempt * 10)) ;;
      *) rm -f "$site/$path"; die "fetching $path: HTTP $code" ;;
    esac
  done
  die "fetching $path: HTTP $code after 6 attempts"
}

min_state=${REGISTRY_MIN_STATE:-}
[ -z "$min_state" ] || [ -f "$min_state" ] || die "REGISTRY_MIN_STATE $min_state does not exist"
wait=${REGISTRY_FRESHNESS_WAIT:-60}
attempts=${REGISTRY_FRESHNESS_ATTEMPTS:-15}
for attempt in $(seq 1 "$attempts"); do
  if get timestamp.json optional; then
    if [ -z "$min_state" ] || ! older_than "$site/timestamp.json" "$min_state"; then
      break
    fi
    note "the live timestamp ($(timestamp_order "$site/timestamp.json")) is older than the last verified deployment ($(timestamp_order "$min_state")): a cached copy; attempt $attempt of $attempts"
    stale=older
  else
    [ -n "$min_state" ] || { echo none; exit 0; }
    note "the live site serves no timestamp, but a registry was deployed and verified ($(timestamp_order "$min_state")); attempt $attempt of $attempts"
    stale=missing
  fi
  [ "$attempt" -lt "$attempts" ] || die "the live site serves $([ "$stale" = missing ] && echo "no registry" || echo "an older registry") than the last verified deployment after $attempts attempts; refusing to build on it (docs/publishing.md, Recovery)"
  sleep "$wait"
done
get timestamp.json.minisig
n=1
while get "keys/$n.json" optional; do
  get "keys/$n.json.minisig"
  n=$((n + 1))
done
[ "$n" -gt 1 ] || die "the live site has a timestamp but no keys/1.json"

# pinned PATH SHA256 LENGTH: PATH (content-addressed: its name is SHA256)
# in the mirror, fetched unless already there, with exactly that sha256 and
# length.
pinned() {
  local path=$1 sha=$2 length=$3
  in_grammar "$path" || die "refusing to mirror $path"
  [[ $sha =~ ^[0-9a-f]{64}$ && $length =~ ^[1-9][0-9]{0,7}$ ]] || die "a storefront pins $path with a bad sha256 or length"
  [ "$length" -le "$(max_bytes "$path")" ] || die "a storefront pins $path with $length bytes, over its $(max_bytes "$path")-byte limit"
  [ "$(basename "$path" | sed 's/\.[a-z]*$//')" = "$sha" ] || die "a storefront pins $path, which is not named by its sha256 $sha"
  [ -f "$site/$path" ] || get "$path"
  [ "$(shasum -a 256 "$site/$path" | cut -d' ' -f1)" = "$sha" ] && [ "$(wc -c < "$site/$path" | tr -d ' ')" = "$length" ] ||
    die "the live $path is not the file its storefront pins (sha256 $sha, $length bytes)"
}

# storefront SEQ: storefront/SEQ.json of the live seq, if the live site has
# one, with its signature, the documents it pins and the images those pin;
# nothing of any other seq (clients accept only the live seq's storefront,
# and nothing is mirrored that is not verified before it is deployed:
# tools/publish-site.sh and _site.yml run verify-site --require-storefront
# on the mirror). With REGISTRY_MIN_STATE of the same seq recording the
# storefront deployed (`storefront_sha256`: its sha256, or null for none),
# the live one must be exactly that one, or absent when none was.
storefront() {
  local s=$1 path sha length doc docs images=0 total=0 have=""
  if get "storefront/$s.json" optional; then
    have=$(shasum -a 256 "$site/storefront/$s.json" | cut -d' ' -f1)
  fi
  if [ -n "$min_state" ] && jq -e --argjson s "$s" '.seq == $s and has("storefront_sha256")' "$min_state" > /dev/null; then
    local want
    want=$(jq -r '.storefront_sha256 // ""' "$min_state")
    [ "$have" = "$want" ] || die "the live storefront/$s.json is $([ -n "$have" ] && echo "sha256 $have" || echo absent), but the last verified deployment of seq $s had $([ -n "$want" ] && echo "sha256 $want" || echo none) (branch registry-state); refusing to build on it"
  fi
  [ -n "$have" ] || return 0
  [ "$(wc -c < "$site/storefront/$s.json" | tr -d ' ')" -le 65536 ] || die "the live storefront/$s.json is over 64 KiB"
  get "storefront/$s.json.minisig"
  jq -e 'type == "object" and (.listings | type) == "array"' "$site/storefront/$s.json" > /dev/null 2>&1 &&
    docs=$(jq -r '[.listings[].ref, (.editorial // empty)] | .[] | [.path, .sha256, (.length | tostring)] | @tsv' \
      "$site/storefront/$s.json") || die "the live storefront/$s.json is not a storefront envelope"
  [ "$(jq '.listings | length' "$site/storefront/$s.json")" -le "$MAX_LISTINGS" ] ||
    die "the live storefront/$s.json lists more than $MAX_LISTINGS listings"
  while IFS=$'\t' read -r path sha length; do
    [ -n "$path" ] || continue
    pinned "$path" "$sha" "$length"
    # Every image reference of the document: {sha256, length, format, ...};
    # the client derives the path (media/<sha256>.<format>).
    jq -e 'type == "object"' "$site/$path" > /dev/null 2>&1 &&
      doc=$(jq -r '[.. | objects | select((.sha256 | type) == "string" and (.format | type) == "string" and (.length | type) == "number")]
        | unique_by(.sha256) | .[] | ["media/\(.sha256).\(.format)", .sha256, (.length | tostring)] | @tsv' "$site/$path") ||
      die "the live $path is not a JSON document"
    # The document's budget, on the declared lengths, before any fetch.
    local n=0 bytes=0 budget=$((8 * 1048576))
    [[ $path == storefront/editorial/* ]] && budget=$((16 * 1048576))
    while IFS=$'\t' read -r _ _ length; do
      [ -n "$length" ] || continue
      [[ $length =~ ^[1-9][0-9]{0,7}$ ]] || die "$path pins an image with a bad length"
      n=$((n + 1)) bytes=$((bytes + length))
    done <<< "$doc"
    [ "$bytes" -le "$budget" ] || die "$path pins $bytes bytes of images, over its $budget-byte budget"
    images=$((images + n)) total=$((total + bytes))
    [ "$images" -le "$MAX_IMAGES" ] || die "the live storefront/$s.json pins more than $MAX_IMAGES images"
    [ "$total" -le "$MAX_TOTAL" ] || die "the live storefront/$s.json pins more than $MAX_TOTAL bytes of images"
    while IFS=$'\t' read -r path sha length; do
      [ -n "$path" ] || continue
      pinned "$path" "$sha" "$length"
    done <<< "$doc"
  done <<< "$docs"
}

seq=$(jq -er '.seq | select(type == "number" and . >= 1 and . == floor)' "$site/timestamp.json") ||
  die "the live timestamp has no valid seq"
s=$seq
while [ "$s" -ge 1 ] && [ "$s" -gt $((seq - 11)) ]; do
  if get "index/$s.json" optional; then
    get "index/$s.json.minisig"
    jq -r '.plugins[]?.detail.path' "$site/index/$s.json" | while read -r detail; do
      [ -f "$site/$detail" ] || get "$detail"
    done
  elif [ "$s" -eq "$seq" ]; then
    die "the live timestamp names index/$seq.json, which is missing"
  fi
  s=$((s - 1))
done
storefront "$seq"
echo live
