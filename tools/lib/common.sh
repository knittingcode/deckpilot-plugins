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

# Options for every git that runs on a throwaway repository: no automatic gc
# or maintenance. Since git 2.47 a fetch starts `git maintenance run --auto
# --detach` in the background; that process (locks, packs, commit-graph)
# can still be writing into .git while the repository is removed, so `rm -rf`
# fails with "Directory not empty" (seen in CI).
GIT_NO_BACKGROUND=(-c gc.auto=0 -c maintenance.auto=false -c fetch.writeCommitGraph=false)

# remove_tree DIR: rm -rf DIR, retried for a moment if something (a git
# process that is still exiting) recreates files in it. Never fails: DIR is
# a throwaway directory, and what decides the caller's result is already
# known; a leftover is reported, not fatal.
remove_tree() {
  local dir=$1 try
  for try in 1 2 3 4 5; do
    rm -rf "$dir" 2>/dev/null && [ ! -e "$dir" ] && return 0
    sleep "$try"
  done
  printf 'warning: could not remove %s\n' "$dir" >&2
  return 0
}

# require_on_main OWNER/NAME SHA: SHA is a commit of OWNER/NAME's main
# branch (an ancestor of, or equal to, its tip), or die. GitHub serves any
# commit of a repository's fork network by SHA, so a commit that exists only
# in a fork would otherwise build as "DeckPilot" (and run as
# deckpilot-registry next to the signing keys). Checked in a separate,
# treeless clone of main's full history before anything of SHA is fetched;
# nothing of a commit outside main is ever downloaded. The clone is a new,
# unique directory per call (never one a previous call or a still-running
# git may be using) and is removed afterwards; its removal never decides
# the result.
# With several SHAs, each must be (one clone).
require_on_main() {
  local repo=$1 dir verdict=ok sha bad=
  shift
  [[ $repo =~ ^[A-Za-z0-9-]{1,39}/[A-Za-z0-9._-]{1,100}$ ]] || die "bad repository $repo"
  [ $# -ge 1 ] || die "require_on_main: no commit"
  for sha in "$@"; do
    [[ $sha =~ ^[0-9a-f]{40}$ ]] || die "bad commit $sha"
  done
  mkdir -p "$WORK"
  dir=$(mktemp -d "$WORK/main-check.XXXXXX") || die "cannot create a directory in $WORK"
  if ! git -C "$dir" init -q ||
    ! GIT_NO_LAZY_FETCH=1 git -C "$dir" "${GIT_NO_BACKGROUND[@]}" -c protocol.version=2 fetch -q --filter=tree:0 \
      "$GIT_BASE/$repo.git" "+refs/heads/main:refs/remotes/origin/main"; then
    verdict=unfetchable
  else
    for sha in "$@"; do
      if ! GIT_NO_LAZY_FETCH=1 git -C "$dir" "${GIT_NO_BACKGROUND[@]}" cat-file -e "$sha^{commit}" 2>/dev/null ||
        ! GIT_NO_LAZY_FETCH=1 git -C "$dir" "${GIT_NO_BACKGROUND[@]}" merge-base --is-ancestor "$sha" refs/remotes/origin/main; then
        verdict=off-main bad=$sha
        break
      fi
    done
  fi
  remove_tree "$dir"
  case $verdict in
    ok) ;;
    unfetchable) die "the main branch of $repo cannot be fetched" ;;
    *) die "$bad is not a commit of $repo's main branch; refusing to build it (a fork's commit is served under the same URL)" ;;
  esac
}

# fetch_deckpilot DIR: DeckPilot at DECKPILOT_COMMIT in DIR, only once that
# commit, and DECKPILOT_SDK_COMMIT (the SDK rev official/ and first-party/
# build against), are known to be on DeckPilot's main (require_on_main),
# also when DIR already holds it.
fetch_deckpilot() {
  require_on_main "$DECKPILOT_REPOSITORY" "$DECKPILOT_COMMIT" "${DECKPILOT_SDK_COMMIT:-$DECKPILOT_COMMIT}"
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
  remove_tree "$dir"
  [ ! -e "$dir" ] || die "$dir cannot be removed"
  mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" "${GIT_NO_BACKGROUND[@]}" -c protocol.version=2 fetch -q --depth 1 "$GIT_BASE/$repo.git" "$sha"
  git -C "$dir" "${GIT_NO_BACKGROUND[@]}" checkout -q --detach FETCH_HEAD
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
