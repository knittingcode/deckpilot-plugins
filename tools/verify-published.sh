#!/usr/bin/env bash
# verify-published.sh BASE_URL EXPECTED_TIMESTAMP   (publish.yml)
#
# Waits until Pages serves the timestamp this run signed (the CDN caches
# for about 10 minutes), mirrors the live site anonymously and verifies it
# like a fresh client, with every listed package downloaded from its
# release URL and checked against its pins.
. "$(dirname "$0")/lib/common.sh"
require_cli
base=$1 expected=$2
live=$WORK/published
for attempt in $(seq 1 20); do
  rm -rf "$live"
  if [ "$("$REPO_ROOT/tools/mirror-site.sh" "$base" "$live")" = live ] &&
    cmp -s "$live/site/v1/timestamp.json" "$expected"; then
    break
  fi
  [ "$attempt" -lt 20 ] || die "the live site does not serve the signed timestamp after 20 attempts"
  sleep 45
done
# Download every listed package into releases/ for --require-packages.
for detail in "$live"/site/v1/plugins/*/*.json; do
  [ -f "$detail" ] || continue
  jq -r '.versions[].package.url' "$detail" | while read -r url; do
    rel=${url#https://github.com/knittingcode/deckpilot-plugins/releases/download/}
    [[ $rel =~ ^[a-z][a-z0-9.-]{3,56}--v[0-9a-z.+-]{5,32}/[a-z0-9.+-]+\.dpplugin$ ]] || die "bad URL $url"
    mkdir -p "$live/releases/$(dirname "$rel")"
    curl -fsSL --proto '=https' -o "$live/releases/$rel" "$url"
  done
done
"$CLI" verify-site --dir "$live" --roots "$REPO_ROOT/keys/roots.json" --require-packages --min-remaining 6d
summary "Live registry verified: $(jq -c '{seq, keys_version, expires}' "$live/site/v1/timestamp.json")"
