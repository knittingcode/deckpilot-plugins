# Shared helpers for tools/*.sh. Source it; it sets strict mode.
set -euo pipefail

REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
WORK=${DECKPILOT_PLUGINS_WORK:-$REPO_ROOT/.work}
# shellcheck source=../deckpilot.env
. "$REPO_ROOT/tools/deckpilot.env"
DECKPILOT_DIR=$WORK/deckpilot
CLI=$WORK/bin/deckpilot-registry
SCHEMA_CHECK=$WORK/bin/schema-check

die() { printf 'error: %s\n' "$*" >&2; exit 1; }
note() { printf '==> %s\n' "$*" >&2; }

# Appends Markdown to the job summary when running in GitHub Actions.
summary() {
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    printf '%s\n' "$@" >> "$GITHUB_STEP_SUMMARY"
  fi
}

# fetch_commit DIR OWNER/NAME SHA: a checkout of exactly SHA in DIR.
fetch_commit() {
  local dir=$1 repo=$2 sha=$3
  [[ $repo =~ ^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$ ]] || die "bad repository $repo"
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || die "bad commit $sha"
  if [ -d "$dir/.git" ] && [ "$(git -C "$dir" rev-parse HEAD 2>/dev/null)" = "$sha" ]; then
    return
  fi
  rm -rf "$dir"
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" -c protocol.version=2 fetch -q --depth 1 "https://github.com/$repo.git" "$sha"
  git -C "$dir" checkout -q --detach FETCH_HEAD
  [ "$(git -C "$dir" rev-parse HEAD)" = "$sha" ] || die "$repo did not resolve to $sha"
}

require_cli() {
  [ -x "$CLI" ] || die "$CLI is missing; run tools/build-cli.sh"
  [ -x "$SCHEMA_CHECK" ] || die "$SCHEMA_CHECK is missing; run tools/build-cli.sh"
}

# The `builder` recorded in every version (registry-plugin schema): the
# version of this repository's build recipe format and tools/build-plugin.sh.
# Bump it when the way packages are built changes.
BUILDER_VERSION=1.0.0
WASM_TARGET=wasm32-unknown-unknown
