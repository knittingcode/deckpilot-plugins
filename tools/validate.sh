#!/usr/bin/env bash
# validate.sh [registry|examples|storefront|all]   (default: all)
#
# What validate.yml runs, also locally (after tools/build-cli.sh):
#
# registry  the committed registry/ and plugins/<id>/:
#   1. tools/check-inputs.py: schemas and layout rules.
#   2. Rebuilds every recorded version from its pinned source
#      (tools/build-plugin.sh) and re-derives its version record with
#      `deckpilot-registry add-version` in a scratch registry (`--first-party`
#      for `deckpilot.*`, under a throwaway key set that authorizes the
#      namespace, as the production one must); the result must
#      be byte-identical to registry/plugins/<id>/versions/<v>.json (sha256,
#      size, manifest digest, permissions, engines, platforms, URL).
#      An official plugin (official/marketplace/<id>/, docs/official-
#      marketplace.md) has no recipe: its release asset and both signatures
#      are downloaded and verified again with the pinned publisher keys
#      (tools/official-entry.py package); the re-derived record must be its
#      committed candidate byte for byte, and the registry record that
#      candidate with the host floor (official-entry.py registry-record).
#   3. plugins/<id>/manifest.json must equal the newest package's manifest
#      (not for official plugins: their manifest is official/<name>/).
#   4. If keys/ holds a key set chain, `build-index` walks it with the
#      client's own rules (proves the committed chain verifies).
#   5. Signs the whole registry with THROWAWAY keys made here (a key set
#      with the first-party authorization, `keyset init --first-party`), and
#      runs `verify-site --require-packages`: the DeckPilot client accepts it
#      and every package matches its pins. The keys and the output are scratch.
# examples  plugins/examples/*: builds each example from source.json and runs
#   step 5 on a registry holding only it. Examples are never published.
# storefront  registry/plugins/*/listing.json, their media/ and editorial/
#   (docs/storefront.md):
#   1. tools/check-storefront.py: the input schemas, the cross-file, text,
#      URL and markdown rules, and a header-level check of every media file.
#   2. The same schemas with the pinned schema-check (the reference JSON
#      Schema implementation), on the committed files and on the fixtures of
#      tools/tests/test_storefront.py, so both implementations agree.
#   3. `deckpilot-registry storefront check --dir .` (DeckPilot WP1: full
#      decode of every asset, the authoritative rules) when the pinned CLI
#      has it; until tools/deckpilot.env pins it, this step says so and only
#      1 and 2 run.
#
# Runs plugin build scripts: never run it where a secret is available.
. "$(dirname "$0")/lib/common.sh"
require_cli
what=${1:-all}

VAL=$WORK/validate
rm -rf "$VAL"
mkdir -p "$VAL"
COMMIT=$(git -C "$REPO_ROOT" rev-parse HEAD)

check_inputs() {
  python3 -I "$REPO_ROOT/tools/check-inputs.py" --schema-check "$SCHEMA_CHECK" \
    --schemas "$REPO_ROOT/schemas" "$@"
}

# throwaway_keys DIR KEYS: new keys in the empty directory KEYS and a
# root-signed key set in DIR that authorizes the first-party namespace
# `deckpilot` (`keyset init --first-party`).
throwaway_keys() {
  local dir=$1 keys=$2
  for role in root index timestamp; do
    "$CLI" keys generate --role "$role" --out "$keys/$role.key" --no-password >/dev/null
  done
  "$CLI" keyset init --dir "$dir" --root "$keys/root.key.pub" \
    --index "$keys/index.key.pub" --timestamp "$keys/timestamp.key.pub" --first-party
  "$CLI" keyset sign --dir "$dir" --key "$keys/root.key"
}

# throwaway_sign DIR: a complete signed site in DIR/site/v1 under new keys.
throwaway_sign() {
  local dir=$1 keys
  keys=$(mktemp -d "$VAL/keys.XXXXXX")
  throwaway_keys "$dir" "$keys"
  "$CLI" build-index --dir "$dir" --source-commit "$COMMIT"
  "$CLI" sign-index --dir "$dir" --key "$keys/index.key"
  "$CLI" timestamp --dir "$dir" --key "$keys/timestamp.key"
  rm -rf "$keys"
  "$CLI" verify-site --dir "$dir" --require-packages --min-remaining 6d
}

validate_registry() {
  note "registry inputs"
  local list
  list=$(check_inputs --root "$REPO_ROOT" --recipes "$REPO_ROOT/plugins" --list-versions)

  local site=$VAL/registry derive=$VAL/derive
  mkdir -p "$site" "$derive/registry"
  cp -R "$REPO_ROOT/registry/." "$site/registry/"
  [ -d "$REPO_ROOT/registry/publishers" ] && cp -R "$REPO_ROOT/registry/publishers" "$derive/registry/"
  # add-version checks a deckpilot.* package against the current key set.
  local derive_keys
  derive_keys=$(mktemp -d "$VAL/keys.XXXXXX")
  throwaway_keys "$derive" "$derive_keys"
  rm -rf "$derive_keys"
  summary "### Registry submissions" ""
  summary "| Plugin | Version | sha256 | Size | Permissions |" "| --- | --- | --- | --- | --- |"

  local id version record count=0
  mkdir -p "$VAL/newest"
  while read -r id version record; do
    [ -n "$id" ] || continue
    count=$((count + 1))
    note "$id $version"
    local pkg=$VAL/packages/$id-$version.dpplugin
    local official=
    [ -d "$REPO_ROOT/official/marketplace/$id" ] && official=$REPO_ROOT/official/marketplace/$id/versions/$version.json
    if [ -n "$official" ]; then
      python3 -I "$REPO_ROOT/tools/official-entry.py" package --id "$id" --version "$version" --out "$pkg" </dev/null
      python3 -I "$REPO_ROOT/tools/official-entry.py" registry-record --id "$id" --version "$version" \
        </dev/null >"$VAL/official-record.json"
      diff -u "$VAL/official-record.json" "$record" >&2 ||
        die "$record is not the registry record of its candidate entry (diff above: - expected, + recorded)"
    else
      local src=(--recipe "$REPO_ROOT/plugins/$id/build.json"
        --repository "$(jq -r .source.repository "$record")"
        --commit "$(jq -r .source.commit "$record")"
        --tag "$(jq -r .source.tag "$record")"
        --path "$(jq -r '.source.path // "."' "$record")"
        --out "$pkg")
      "$REPO_ROOT/tools/build-plugin.sh" "${src[@]}"
    fi

    # Re-derive the record from the built package.
    local args=(--dir "$derive" --package "$pkg"
      --plugin-detail "$REPO_ROOT/registry/plugins/$id/plugin.json"
      --source-repository "$(jq -r .source.repository "$record")"
      --tag "$(jq -r .source.tag "$record")"
      --commit "$(jq -r .source.commit "$record")"
      --builder "$(jq -r .builder "$record")"
      --now "$(jq -r .published_at "$record")")
    if [ "$(jq -r '.source.path // empty' "$record")" != "" ]; then
      args+=(--source-path "$(jq -r .source.path "$record")")
    fi
    if [ "$(jq -r '.attestation // empty' "$record")" != "" ]; then
      args+=(--attestation "$(jq -r .attestation "$record")")
    fi
    # DeckPilot's own plugins (B6).
    case $id in deckpilot.*) args+=(--first-party) ;; esac
    "$CLI" add-version "${args[@]}"
    local derived=$derive/registry/plugins/$id/versions/$version.json
    if [ -n "$official" ]; then
      cmp -s "$derived" "$official" || {
        diff -u "$derived" "$official" >&2
        die "$official is not the record add-version derives from the verified release asset"
      }
    elif ! diff -u <(jq -S 'del(.yanked)' "$derived") <(jq -S 'del(.yanked)' "$record") >&2; then
      summary "| \`$id\` | $version | built \`$(jq -r .package.sha256 "$derived")\` | $(jq -r .package.size "$derived") | **does not match the record** |" "" \
        "The record derived from the package this run built (the reference build) is below. If your local build differs, commit this one as \`registry/plugins/$id/versions/$version.json\`:" "" \
        '```json' "$(cat "$derived")" '```'
      echo "derived record (reference build):" >&2
      cat "$derived" >&2
      die "$record does not match the package built from its source (diff above: - built, + recorded)"
    fi
    summary "| \`$id\` | $version | \`$(jq -r .package.sha256 "$record")\` | $(jq -r .package.size "$record") | $(jq -c '[.permissions[] | .kind + (if .hosts then ":" + (.hosts | join(",")) else "" end)]' "$record") |"
    local url rel
    url=$(jq -r .package.url "$record")
    rel=${url#https://github.com/knittingcode/deckpilot-plugins/releases/download/}
    mkdir -p "$site/releases/$(dirname "$rel")"
    cp "$pkg" "$site/releases/$rel"
    [ -n "$official" ] || printf '%s\n' "$pkg" >"$VAL/newest/$id"   # ascending: the last one wins
  done <<<"$list"
  [ "$count" -gt 0 ] || summary "| — | no versions recorded yet | | | |"

  # plugins/<id>/manifest.json is the newest version's manifest, so reviewers
  # see the permissions and contributions in the pull request diff.
  local entry
  for entry in "$VAL"/newest/*; do
    [ -f "$entry" ] || continue
    id=$(basename "$entry")
    cmp -s <(python3 -I "$REPO_ROOT/tools/check-inputs.py" --manifest-of "$(cat "$entry")") \
      "$REPO_ROOT/plugins/$id/manifest.json" ||
      die "plugins/$id/manifest.json is not the manifest of the newest recorded version"
  done

  if [ -f "$REPO_ROOT/keys/roots.json" ]; then
    note "committed key set chain"
    local chain=$VAL/chain
    mkdir -p "$chain/site/v1/keys"
    cp "$REPO_ROOT/keys/roots.json" "$chain/roots.json"
    cp "$REPO_ROOT"/keys/[0-9]*.json "$REPO_ROOT"/keys/[0-9]*.json.minisig "$chain/site/v1/keys/"
    "$CLI" build-index --dir "$chain" --source-commit "$COMMIT" >/dev/null
    note "the committed key set chain verifies"
    # Clients refuse deckpilot.* unless the current key set authorizes it.
    local newest
    newest=$(find "$REPO_ROOT/keys" -name '[0-9]*.json' -exec basename {} .json \; | sort -n | tail -1)
    if compgen -G "$REPO_ROOT/registry/plugins/deckpilot.*/versions/*.json" >/dev/null &&
      ! jq -e '.first_party.namespaces // [] | index("deckpilot")' "$REPO_ROOT/keys/$newest.json" >/dev/null; then
      die "keys/$newest.json does not authorize the namespace deckpilot (keyset rotate --first-party), but registry/ records deckpilot.* versions"
    fi
  fi

  note "official plugins in the registry"
  python3 -I "$REPO_ROOT/tools/official-entry.py" gate --registry "$REPO_ROOT/registry"

  note "signing the registry with throwaway keys"
  throwaway_sign "$site"
}

validate_examples() {
  local ex
  for ex in "$REPO_ROOT"/plugins/examples/*/; do
    ex=${ex%/}
    [ -f "$ex/build.json" ] && [ -f "$ex/source.json" ] || continue
    local name id dir
    name=$(basename "$ex")
    note "example $name"
    id=$(jq -r .id "$ex/build.json")
    dir=$VAL/examples/$name
    # The example keeps its recipe next to source.json; check it as a
    # one-plugin registry root with the usual recipes layout.
    mkdir -p "$dir/recipes/$id"
    cp "$ex/build.json" "$dir/recipes/$id/build.json"
    check_inputs --root "$ex" --recipes "$dir/recipes" >/dev/null
    cp -R "$ex/registry" "$dir/registry"
    local pkg=$VAL/packages/example-$name.dpplugin
    "$REPO_ROOT/tools/build-plugin.sh" --recipe "$ex/build.json" \
      --repository "$(jq -r .repository "$ex/source.json")" \
      --commit "$(jq -r .commit "$ex/source.json")" \
      --path "$(jq -r '.path // "."' "$ex/source.json")" --out "$pkg"
    "$CLI" add-version --dir "$dir" --package "$pkg" \
      --source-repository "$(jq -r .repository "$ex/source.json")" \
      --tag "unpublished-example" --commit "$(jq -r .commit "$ex/source.json")" \
      --source-path "$(jq -r '.path // "."' "$ex/source.json")" --builder "$BUILDER_VERSION"
    throwaway_sign "$dir"
    summary "### Example \`$id\` (not published)" "" \
      "Built from \`$(jq -r .repository "$ex/source.json")@$(jq -r .commit "$ex/source.json")\`: sha256 \`$(shasum -a 256 "$pkg" | cut -d' ' -f1)\`, $(wc -c <"$pkg" | tr -d ' ') bytes; signed with throwaway keys and accepted by \`verify-site --require-packages\`." ""
  done
}

validate_storefront() {
  note "storefront inputs (header-level)"
  python3 -I "$REPO_ROOT/tools/check-storefront.py" --root "$REPO_ROOT"

  note "storefront schemas with the pinned schema-check"
  local inputs=https://github.com/knittingcode/deckpilot-plugins/schemas/inputs
  local listings=()
  while IFS= read -r -d '' f; do listings+=("$f"); done < <(
    find "$REPO_ROOT/registry/plugins" -mindepth 2 -maxdepth 2 -name listing.json -type f -print0 2>/dev/null | sort -z)
  if [ "${#listings[@]}" -gt 0 ]; then
    "$SCHEMA_CHECK" --schemas "$REPO_ROOT/schemas" "$inputs/listing.schema.json" "${listings[@]}"
  fi
  if [ -f "$REPO_ROOT/editorial/editorial.json" ]; then
    "$SCHEMA_CHECK" --schemas "$REPO_ROOT/schemas" "$inputs/editorial.schema.json" "$REPO_ROOT/editorial/editorial.json"
  fi
  STOREFRONT_SCHEMA_CHECK=$SCHEMA_CHECK python3 -I "$REPO_ROOT/tools/tests/test_storefront.py" 2>"$VAL/storefront-tests.log" ||
    { cat "$VAL/storefront-tests.log" >&2; die "the storefront schema fixtures disagree (log above)"; }

  if "$CLI" help 2>&1 | grep -Eq '^[[:space:]]*storefront check'; then
    note "deckpilot-registry storefront check (authoritative)"
    "$CLI" storefront check --dir "$REPO_ROOT"
    summary "Storefront: \`deckpilot-registry storefront check\` of the pinned DeckPilot passed (full decode)." ""
  else
    local why="the pinned deckpilot-registry (DeckPilot ${DECKPILOT_COMMIT:0:12}) has no 'storefront check'"
    note "$why: ran the JSON-schema and header-level checks only (no full decode of media); the authoritative check runs once tools/deckpilot.env pins DeckPilot's storefront tooling"
    summary "Storefront: $why, so only the JSON-schema and header-level checks ran (no full decode of media). The authoritative check runs once \`tools/deckpilot.env\` pins DeckPilot's storefront tooling." ""
  fi
}

case $what in
  registry) validate_registry ;;
  examples) validate_examples ;;
  storefront) validate_storefront ;;
  all) validate_registry; validate_examples; validate_storefront ;;
  *) die "usage: validate.sh [registry|examples|storefront|all]" ;;
esac
note "validation passed"
