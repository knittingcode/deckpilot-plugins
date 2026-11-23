#!/usr/bin/env bash
# verify-published.sh BASE_URL EXPECTED_TIMESTAMP   (publish.yml)
#
# Waits until Pages serves the timestamp this run signed (the CDN caches
# for about 10 minutes), mirrors the live site anonymously and verifies it
# like a fresh client, with every listed package downloaded from its
# release URL and checked against its pins.
#
# The storefront (docs/storefront.md): when the signed registry (the
# directory of EXPECTED_TIMESTAMP, publish.yml's registry-v1 artifact) has
# storefront/<seq>.json, the live site must serve storefront/ and media/
# exactly as signed (the mirror fetched them anonymously and checked every
# pinned sha256 and length; here every file is compared with the signed
# one), and verify-site runs with --require-storefront. With
# REGISTRY_STOREFRONT_ENABLED=true a signed registry without one fails.
. "$(dirname "$0")/lib/common.sh"
require_cli
base=$1 expected=$2
signed=$(dirname "$expected")
storefront=${REGISTRY_STOREFRONT_ENABLED:-false}
case $storefront in true | false) ;; *) die "REGISTRY_STOREFRONT_ENABLED is '$storefront'; it must be true, false or unset" ;; esac
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
seq=$(jq -er .seq "$expected")
require=()
if [ -f "$signed/storefront/$seq.json" ]; then
  # files DIR: "<path> <sha256>" of storefront/ and media/ under DIR.
  files() {
    (cd "$1" && find storefront media -type f 2> /dev/null | LC_ALL=C sort | while read -r f; do
      printf '%s %s\n' "$f" "$(shasum -a 256 "$f" | cut -d' ' -f1)"
    done)
  }
  diff <(files "$signed") <(files "$live/site/v1") >&2 ||
    die "the live storefront/ and media/ are not the signed ones (diff above: < signed, > live)"
  note "the live storefront/ and media/ are the signed ones: $(files "$signed" | wc -l | tr -d ' ') file(s)"
  require=(--require-storefront)
elif [ "$storefront" = true ]; then
  die "REGISTRY_STOREFRONT_ENABLED is true, but the signed registry has no storefront/$seq.json"
fi
"$CLI" verify-site --dir "$live" --roots "$REPO_ROOT/keys/roots.json" --require-packages --min-remaining 6d \
  "${require[@]+"${require[@]}"}"
summary "Live registry verified: $(jq -c '{seq, keys_version, expires}' "$live/site/v1/timestamp.json")"
[ ${#require[@]} -eq 0 ] ||
  summary "Live storefront verified: \`storefront/$seq.json\`, its documents and $(find "$live/site/v1/media" -type f 2> /dev/null | wc -l | tr -d ' ') image(s), fetched anonymously, the signed bytes; accepted by the client."
