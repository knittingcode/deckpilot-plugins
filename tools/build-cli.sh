#!/usr/bin/env bash
# Builds the pinned tools into .work/bin:
# - deckpilot-registry from DeckPilot at tools/deckpilot.env's commit
#   (--locked, feature `publish`), with DeckPilot's own pinned toolchain;
# - schema-check from tools/schema-check (--locked).
# The pinned commit must be on DeckPilot's main (tools/lib/common.sh
# require_on_main): never a fork's commit served under DeckPilot's URL.
# Nothing is downloaded except the pinned DeckPilot commit, crates verified
# by the Cargo.lock checksums, and the toolchain rustup verifies.
. "$(dirname "$0")/lib/common.sh"

note "DeckPilot $DECKPILOT_REPOSITORY@$DECKPILOT_COMMIT"
fetch_deckpilot "$DECKPILOT_DIR"
mkdir -p "$WORK/bin"

note "building deckpilot-registry"
(cd "$DECKPILOT_DIR" &&
  cargo build --locked --release -p deckpilot-plugin-registry --features publish --bin deckpilot-registry)
install -m 0755 "$DECKPILOT_DIR/target/release/deckpilot-registry" "$CLI"

note "building schema-check"
cargo build --locked --release --manifest-path "$REPO_ROOT/tools/schema-check/Cargo.toml" \
  --target-dir "$WORK/schema-check-target"
install -m 0755 "$WORK/schema-check-target/release/schema-check" "$SCHEMA_CHECK"
"$CLI" help | head -1
