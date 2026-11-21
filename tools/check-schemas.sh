#!/usr/bin/env bash
# schemas/*.json must be byte-identical to DeckPilot's at the pinned commit,
# rust-toolchain.toml must name the same channel, and the reserved publisher
# ids and bundled plugin ids of tools/check-submission.py (bundled ids also
# of tools/check-first-party.py) must equal DeckPilot's.
. "$(dirname "$0")/lib/common.sh"
fetch_deckpilot "$DECKPILOT_DIR"

status=0
check() {
  if cmp -s "$1" "$2"; then
    note "ok: ${1#"$REPO_ROOT"/} = DeckPilot ${2#"$DECKPILOT_DIR"/}"
  else
    printf 'error: %s differs from DeckPilot %s at %s\n' "${1#"$REPO_ROOT"/}" \
      "${2#"$DECKPILOT_DIR"/}" "$DECKPILOT_COMMIT" >&2
    status=1
  fi
}
check "$REPO_ROOT/schemas/plugin-manifest.schema.json" \
  "$DECKPILOT_DIR/crates/deckpilot-plugin-manifest/schema/plugin-manifest.schema.json"
for f in "$DECKPILOT_DIR"/crates/deckpilot-plugin-registry/schema/*.json; do
  check "$REPO_ROOT/schemas/$(basename "$f")" "$f"
done
for f in "$REPO_ROOT"/schemas/*.json; do
  name=$(basename "$f")
  [ "$name" = plugin-manifest.schema.json ] ||
    [ -f "$DECKPILOT_DIR/crates/deckpilot-plugin-registry/schema/$name" ] ||
    { echo "error: schemas/$name is not a DeckPilot schema" >&2; status=1; }
done

channel() { sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' "$1"; }
ours=$(channel "$REPO_ROOT/rust-toolchain.toml")
theirs=$(channel "$DECKPILOT_DIR/rust-toolchain.toml")
if [ -n "$ours" ] && [ "$ours" = "$theirs" ]; then
  note "ok: rust-toolchain.toml channel $ours"
else
  echo "error: rust-toolchain.toml channel '$ours', DeckPilot has '$theirs'" >&2
  status=1
fi
if python3 -I "$REPO_ROOT/tools/tests/test_check_submission.py" --deckpilot "$DECKPILOT_DIR" \
  MirrorsDeckPilot >/dev/null 2>"$WORK/reserved.log"; then
  note "ok: RESERVED_PUBLISHERS and BUNDLED_PLUGIN_IDS = DeckPilot's"
else
  cat "$WORK/reserved.log" >&2
  echo "error: RESERVED_PUBLISHERS or BUNDLED_PLUGIN_IDS of tools/ differ from DeckPilot at $DECKPILOT_COMMIT" >&2
  status=1
fi
exit $status
