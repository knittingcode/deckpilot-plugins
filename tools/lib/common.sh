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

# Where repositories are fetched from (tests point it at local repositories).
GIT_BASE=${DECKPILOT_PLUGINS_GIT_BASE:-https://github.com}

# require_on_main OWNER/NAME SHA: SHA is a commit of OWNER/NAME's main
# branch (an ancestor of, or equal to, its tip), or die. GitHub serves any
# commit of a repository's fork network by SHA, so a commit that exists only
# in a fork would otherwise build as "DeckPilot" (and run as
# deckpilot-registry next to the signing keys). Checked in a separate,
# treeless clone of main's full history before anything of SHA is fetched;
# nothing of a commit outside main is ever downloaded.
require_on_main() {
  local repo=$1 sha=$2 dir=$WORK/main-check
  [[ $repo =~ ^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$ ]] || die "bad repository $repo"
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || die "bad commit $sha"
  rm -rf "$dir"
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" -c protocol.version=2 fetch -q --filter=tree:0 "$GIT_BASE/$repo.git" \
    "+refs/heads/main:refs/remotes/origin/main" || die "the main branch of $repo cannot be fetched"
  if ! git -C "$dir" cat-file -e "$sha^{commit}" 2>/dev/null ||
    ! git -C "$dir" merge-base --is-ancestor "$sha" refs/remotes/origin/main; then
    rm -rf "$dir"
    die "$sha is not a commit of $repo's main branch; refusing to build it (a fork's commit is served under the same URL)"
  fi
  rm -rf "$dir"
}

# fetch_deckpilot DIR: DeckPilot at DECKPILOT_COMMIT in DIR, only once that
# commit is known to be on DeckPilot's main (require_on_main), also when DIR
# already holds it.
fetch_deckpilot() {
  require_on_main "$DECKPILOT_REPOSITORY" "$DECKPILOT_COMMIT"
  fetch_commit "$1" "$DECKPILOT_REPOSITORY" "$DECKPILOT_COMMIT"
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
  git -C "$dir" -c protocol.version=2 fetch -q --depth 1 "$GIT_BASE/$repo.git" "$sha"
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

# timestamp_order FILE: "<seq> <epoch seconds> <nanoseconds>" of a
# timestamp.json (or of a state.json written by tools/registry-state.sh),
# so two of them compare as integers: seq first, then signed_at.
timestamp_order() {
  jq -er '
    (.signed_at | capture("^(?<s>[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})(\\.(?<f>[0-9]{1,9}))?Z$")) as $t
    | select((.seq | type) == "number" and .seq >= 1 and .seq == (.seq | floor))
    | "\(.seq) \(($t.s + "Z") | fromdateiso8601) \(((($t.f // "") + "000000000")[0:9]))"' "$1" 2>/dev/null
}

# older_than A B: true when the timestamp (or state) A is older than B.
older_than() {
  local a b
  a=$(timestamp_order "$1") || die "$1: no seq or signed_at"
  b=$(timestamp_order "$2") || die "$2: no seq or signed_at"
  read -r as ass an <<< "$a"
  read -r bs bss bn <<< "$b"
  (( as < bs || (as == bs && (ass < bss || (ass == bss && 10#$an < 10#$bn))) ))
}
