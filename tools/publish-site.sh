#!/usr/bin/env bash
# publish-site.sh index|timestamp --packages DIR --out DIR [--base-url URL]
#
# The signing half of publish.yml (`index`) and refresh-timestamp.yml
# (`timestamp`). It never builds or runs plugin code: packages come in as
# files, and every one must match the sha256 and size pinned in the reviewed
# registry/plugins/*/versions/*.json before anything is signed. An official
# plugin's package is its verified release asset (tools/validate.sh in the
# build job: downloaded and checked with the pinned publisher keys).
#
#   1. Assemble a workspace: keys/roots.json and keys/<n>.json(.minisig)
#      from this checkout, registry/ from this checkout, release assets from
#      --packages (hash-checked), the live site mirrored from --base-url.
#   2. `verify-site --state` on the live mirror: the state of an existing
#      client.
#   3. index: build-index (source commit = HEAD), sign-index, timestamp.
#      timestamp: timestamp only (same index, new expiry).
#   4. `verify-site --state <existing client> --require-packages
#      --min-remaining 6d`: existing clients accept the new site (no
#      rollback, no dropped revocation, continuous key set chain).
#
# The storefront (docs/storefront.md), behind REGISTRY_STOREFRONT_ENABLED
# (publish.yml passes the repository variable; unset or `false` is the
# pipeline as it was before storefronts, `true` enables it):
#   - index, enabled: `storefront check` of registry/plugins/*/listing.json
#     and editorial/, then after sign-index `storefront build` for the new
#     seq and `storefront sign` with the same index key, in this job (sign
#     re-checks what build wrote), before the timestamp; the live
#     storefronts of the kept indexes and their files are carried over
#     (build prunes the rest); verify-site --require-storefront.
#   - index, disabled: as before: no storefront/ or media/ on the new site.
#   - timestamp, always: the live storefront/ and media/ are kept byte for
#     byte (as index/ and plugins/ are; checked), so a refresh never drops
#     them, and verified as an existing client does when the live seq has
#     a storefront.
#   - Whenever verify-site runs with --require-storefront, the client state
#     (--state) must then hold registry-storefront.json for that seq and
#     storefront: the equivocation record clients keep beside
#     registry-trust.json, carried from the live site's verification to the
#     new site's like the rest of the state.
#
# Keys come from the environment, never from files in the repository:
#   REGISTRY_INDEX_KEY, REGISTRY_INDEX_KEY_PASSWORD           (index only)
#   REGISTRY_TIMESTAMP_KEY, REGISTRY_TIMESTAMP_KEY_PASSWORD
# (the minisign secret key file's text; the password may be empty for an
# unencrypted key). They are written to 0600 temporary files that are
# removed on exit, and nothing prints them.
. "$(dirname "$0")/lib/common.sh"
require_cli
set +x

mode=${1:-}
shift || true
packages= out= base_url=https://knittingcode.github.io/deckpilot-plugins
while [ $# -gt 0 ]; do
  case $1 in
    --packages) packages=$2 ;;
    --out) out=$2 ;;
    --base-url) base_url=$2 ;;
    *) die "unknown option $1" ;;
  esac
  shift 2
done
case $mode in index | timestamp) ;; *) die "usage: publish-site.sh index|timestamp --packages DIR --out DIR" ;; esac
[ -n "$out" ] || die "--out is required"
[ "$mode" = timestamp ] || [ -n "$packages" ] || die "--packages is required for index"
storefront=${REGISTRY_STOREFRONT_ENABLED:-false}
case $storefront in true | false) ;; *) die "REGISTRY_STOREFRONT_ENABLED is '$storefront'; it must be true, false or unset" ;; esac
[ "$mode" = index ] || storefront=false

need() { [ -n "${!1:-}" ] || die "the secret $1 is not available to this job; refusing to sign"; }
[ "$mode" = timestamp ] || need REGISTRY_INDEX_KEY
need REGISTRY_TIMESTAMP_KEY
[ -f "$REPO_ROOT/keys/roots.json" ] && [ -f "$REPO_ROOT/keys/1.json.minisig" ] ||
  die "keys/ holds no root-signed key set yet (key ceremony pending); refusing to sign"

secrets=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/registry-keys.XXXXXX")
trap 'rm -rf "$secrets"' EXIT
write_key() {
  (umask 077 && printf '%s\n' "${!1}" >"$secrets/$2")
}
key_args() {
  # key_args FILE PASSWORD_VAR
  local args=(--key "$secrets/$1")
  [ -n "${!2:-}" ] && args+=(--password-env "$2")
  printf '%s\n' "${args[@]}"
}

rm -rf "$out"
mkdir -p "$out/site/v1/keys" "$out/registry"
ws=$out
cp "$REPO_ROOT/keys/roots.json" "$ws/roots.json"
cp "$REPO_ROOT"/keys/[0-9]*.json "$REPO_ROOT"/keys/[0-9]*.json.minisig "$ws/site/v1/keys/"
cp -R "$REPO_ROOT/registry/." "$ws/registry/"

sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
# storefront_recorded DIR SEQ: after verify-site --require-storefront on
# DIR, the client state holds the record of DIR's storefront/SEQ.json.
storefront_recorded() {
  local record=$WORK/client-state/registry-storefront.json
  [ -f "$record" ] && jq -e --argjson seq "$2" --arg sha "$(sha "$1/site/v1/storefront/$2.json")" \
    '.seq == $seq and .sha256 == $sha' "$record" > /dev/null ||
    die "verify-site --require-storefront left no record of storefront/$2.json in the client state ($record)"
}

note "mirroring the live registry from $base_url"
live=$WORK/live
rm -rf "$live" "$WORK/client-state"
state=$("$REPO_ROOT/tools/mirror-site.sh" "$base_url" "$live")
mkdir -p "$WORK/client-state"
live_storefront=()
if [ "$state" = live ]; then
  live_seq=$(jq -er .seq "$live/site/v1/timestamp.json")
  # The live seq's storefront, if it has one, is verified as a client does,
  # which records it in the client state (equivocation: the same seq never
  # gets another storefront).
  [ ! -f "$live/site/v1/storefront/$live_seq.json" ] || live_storefront=(--require-storefront)
  "$CLI" verify-site --dir "$live" --roots "$REPO_ROOT/keys/roots.json" --state "$WORK/client-state" \
    "${live_storefront[@]+"${live_storefront[@]}"}"
  [ ${#live_storefront[@]} -eq 0 ] || storefront_recorded "$live" "$live_seq"
  # Published key sets are immutable; ours must extend the live chain.
  for f in "$live"/site/v1/keys/*; do
    cmp -s "$f" "$ws/site/v1/keys/$(basename "$f")" ||
      die "keys/$(basename "$f") differs from the live key set"
  done
  carried=(index plugins)
  # A refresh keeps the storefront; a publication only when it builds one.
  if [ "$mode" = timestamp ] || [ "$storefront" = true ]; then
    carried+=(storefront media)
  fi
  for d in "${carried[@]}"; do
    [ -d "$live/site/v1/$d" ] && cp -R "$live/site/v1/$d" "$ws/site/v1/"
  done
  cp "$live/site/v1/timestamp.json" "$live/site/v1/timestamp.json.minisig" "$ws/site/v1/"
else
  [ "$mode" = index ] || die "no live registry to refresh"
  note "no live registry yet: this is the first publication"
fi

if [ "$mode" = index ]; then
  "$REPO_ROOT/tools/check-first-party-only.sh" "$REPO_ROOT"
  # Official plugins only as their verified candidate entries, never a
  # privileged (bundled-only) one (docs/official-marketplace.md).
  python3 -I "$REPO_ROOT/tools/official-entry.py" gate --registry "$ws/registry"
  note "release assets (must match the reviewed pins)"
  for record in "$ws"/registry/plugins/*/versions/*.json; do
    [ -f "$record" ] || continue
    url=$(jq -r .package.url "$record")
    rel=${url#https://github.com/knittingcode/deckpilot-plugins/releases/download/}
    [[ $rel =~ ^[a-z][a-z0-9.-]{3,56}--v[0-9a-z.+-]{5,32}/[a-z0-9.+-]+\.dpplugin$ ]] || die "bad package URL $url"
    file=$packages/$rel
    [ -f "$file" ] || die "no package for $rel"
    [ "$(shasum -a 256 "$file" | cut -d' ' -f1)" = "$(jq -r .package.sha256 "$record")" ] ||
      die "$rel does not match the sha256 pinned in $(basename "$(dirname "$(dirname "$record")")")"
    [ "$(wc -c <"$file" | tr -d ' ')" = "$(jq -r .package.size "$record")" ] || die "$rel size mismatch"
    mkdir -p "$ws/releases/$(dirname "$rel")"
    cp "$file" "$ws/releases/$rel"
  done

  if [ "$storefront" = true ]; then
    note "storefront sources (docs/storefront.md)"
    [ ! -d "$REPO_ROOT/editorial" ] || cp -R "$REPO_ROOT/editorial" "$ws/editorial"
    "$CLI" storefront check --dir "$ws"
  fi

  write_key REGISTRY_INDEX_KEY index.key
  write_key REGISTRY_TIMESTAMP_KEY timestamp.key
  "$CLI" build-index --dir "$ws" --source-commit "$(git -C "$REPO_ROOT" rev-parse HEAD)"
  # shellcheck disable=SC2046
  "$CLI" sign-index --dir "$ws" $(key_args index.key REGISTRY_INDEX_KEY_PASSWORD)
  if [ "$storefront" = true ]; then
    seq=$(find "$ws/site/v1/index" -name '[1-9]*.json' -exec basename {} .json \; | sort -n | tail -1)
    "$CLI" storefront build --dir "$ws" --seq "$seq"
    # The index role signs the storefront, with its own trusted comment.
    # shellcheck disable=SC2046
    "$CLI" storefront sign --dir "$ws" --seq "$seq" $(key_args index.key REGISTRY_INDEX_KEY_PASSWORD)
  fi
  rm -f "$secrets/index.key"
else
  write_key REGISTRY_TIMESTAMP_KEY timestamp.key
fi
# shellcheck disable=SC2046
"$CLI" timestamp --dir "$ws" --expires-in 7d $(key_args timestamp.key REGISTRY_TIMESTAMP_KEY_PASSWORD)
rm -rf "$secrets"

if [ "$mode" = timestamp ]; then
  # Only the timestamp changes: every other registry file is the live one.
  for d in index plugins storefront media; do
    if [ -e "$live/site/v1/$d" ] || [ -e "$ws/site/v1/$d" ]; then
      diff -r "$live/site/v1/$d" "$ws/site/v1/$d" > /dev/null 2>&1 ||
        die "the refreshed site's $d/ is not the live one byte for byte; refusing"
    fi
  done
fi

note "verifying the new site as an existing client"
require=()
[ "$mode" = index ] && require=(--require-packages)
new_seq=$(jq -er .seq "$ws/site/v1/timestamp.json")
if [ "$storefront" = true ] || { [ "$mode" = timestamp ] && [ ${#live_storefront[@]} -gt 0 ]; }; then
  require+=(--require-storefront)
fi
"$CLI" verify-site --dir "$ws" --state "$WORK/client-state" --min-remaining 6d "${require[@]+"${require[@]}"}"
summary "Signed registry ($mode): $(jq -c '{seq, keys_version, expires}' "$ws/site/v1/timestamp.json")"
case " ${require[*]-} " in
  *" --require-storefront "*)
    storefront_recorded "$ws" "$new_seq"
    summary "Storefront: \`storefront/$new_seq.json\` sha256 \`$(sha "$ws/site/v1/storefront/$new_seq.json")\`, $(jq '.listings | length' "$ws/site/v1/storefront/$new_seq.json") listing(s), editorial $(jq -r 'if .editorial then "yes" else "none" end' "$ws/site/v1/storefront/$new_seq.json"); accepted by the client and recorded in its state." ;;
esac
