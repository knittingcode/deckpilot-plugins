#!/usr/bin/env bash
# Tests of require_on_main and fetch_deckpilot (tools/lib/common.sh) against
# a local repository: commits of main pass, a commit only on another ref
# (what a fork's commit is: served by SHA under the same URL) and an
# unknown commit fail, and fetch_deckpilot never checks such a commit out.
# No network.
#
#   tools/tests/test-deckpilot-pin.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/deckpilot-pin.XXXXXX")
trap 'rm -rf "$work"' EXIT

upstream=$work/remote/acme/deckpilot.git
git init -q --bare "$upstream"
git -C "$upstream" config uploadpack.allowFilter true
git -C "$upstream" config uploadpack.allowAnySHA1InWant true
clone=$work/clone
git init -q -b main "$clone"
commit() { echo "$1" > "$clone/file" && git -C "$clone" add file && git -C "$clone" -c user.name=t -c user.email=t@t commit -q -m "$1" && git -C "$clone" rev-parse HEAD; }
first=$(commit one)
second=$(commit two)
git -C "$clone" push -q "$upstream" main
git -C "$clone" checkout -q -b fork "$first"
fork=$(commit "attacker")
git -C "$clone" push -q "$upstream" "fork:refs/pull/9/head"

passed=0 failed=0
# run <pass|fail> NAME SHA [function]
run() {
  local want=$1 name=$2 sha=$3 fn=${4:-require_on_main}
  local rc=0
  (
    export DECKPILOT_PLUGINS_WORK=$work/work DECKPILOT_PLUGINS_GIT_BASE=file://$work/remote
    # shellcheck source=../lib/common.sh
    . "$root/tools/lib/common.sh"
    DECKPILOT_REPOSITORY=acme/deckpilot DECKPILOT_COMMIT=$sha
    if [ "$fn" = fetch_deckpilot ]; then
      fetch_deckpilot "$work/work/checkout"
    else
      require_on_main acme/deckpilot "$sha"
    fi
  ) > "$work/out" 2>&1 || rc=$?
  if { [ "$want" = pass ] && [ $rc = 0 ]; } || { [ "$want" = fail ] && [ $rc != 0 ]; }; then
    echo "ok   $name"; passed=$((passed + 1))
  else
    echo "FAIL $name: exit $rc: $(tr '\n' ' ' < "$work/out")"; failed=$((failed + 1))
  fi
}

run pass "the tip of main" "$second"
run pass "an older commit of main" "$first"
run fail "a commit only on another ref (a fork's)" "$fork"
grep -q "not a commit of acme/deckpilot's main" "$work/out" && { echo "ok   the refusal says why"; passed=$((passed + 1)); } \
  || { echo "FAIL the refusal: $(cat "$work/out")"; failed=$((failed + 1)); }
run fail "an unknown commit" "$(printf '%040d' 7)"
run fail "not a commit id" "main"
run pass "fetch_deckpilot checks out a commit of main" "$second" fetch_deckpilot
[ "$(git -C "$work/work/checkout" rev-parse HEAD)" = "$second" ] && { echo "ok   ... at that commit"; passed=$((passed + 1)); } \
  || { echo "FAIL fetch_deckpilot checked out $(git -C "$work/work/checkout" rev-parse HEAD)"; failed=$((failed + 1)); }
rm -rf "$work/work/checkout"
run fail "fetch_deckpilot refuses a fork's commit" "$fork" fetch_deckpilot
[ ! -e "$work/work/checkout" ] && { echo "ok   ... and fetches nothing of it"; passed=$((passed + 1)); } \
  || { echo "FAIL a checkout of the fork's commit exists"; failed=$((failed + 1)); }

echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
