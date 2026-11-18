#!/usr/bin/env bash
# mirror-site.sh BASE_URL DIR
#
# Copies the live signed registry (BASE_URL/v1/...) into DIR/site/v1: every
# key set, the timestamp, the newest index and the 10 before it, and every
# detail those indexes pin. Prints `live` or `none` (no timestamp served:
# no registry was ever published). Only the registry's own path grammar is
# fetched, and the result is data: tools/publish-site.sh runs `verify-site`
# on it with the committed roots before anything builds on it.
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

# get PATH [optional]: fetch BASE_URL/v1/PATH into the mirror. With
# `optional`, a 404 returns 1; anything else that fails is fatal.
get() {
  local path=$1 code
  [[ $path =~ ^(timestamp\.json|keys/[1-9][0-9]*\.json|index/[1-9][0-9]*\.json|plugins/[a-z][a-z0-9.-]{3,56}/[0-9a-f]{64}\.json)(\.minisig)?$ ]] ||
    die "refusing to mirror $path"
  mkdir -p "$(dirname "$site/$path")"
  local attempt
  for attempt in 1 2 3 4 5 6; do
    code=$(curl -sS --proto "$proto" --max-filesize 4194304 -o "$site/$path" -w '%{http_code}' \
      "$base/v1/$path") || code=000
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
echo live
