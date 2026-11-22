#!/usr/bin/env bash
# Tests of require_on_main and fetch_deckpilot (tools/lib/common.sh) against
# a local repository: commits of main pass, a commit only on another ref
# (what a fork's commit is: served by SHA under the same URL) and an
# unknown commit fail, and fetch_deckpilot never checks such a commit out.
# The check runs in its own directory per call, starts no background git
# (gc/maintenance) and leaves nothing behind; a failure to remove that
# directory never changes the check's verdict. No network.
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
# run <pass|fail> NAME SHA [function] [stuck]: with "stuck", removing any
# directory fails (what a git still writing into it causes).
run() {
  local want=$1 name=$2 sha=$3 fn=${4:-require_on_main} stuck=${5:-}
  local rc=0
  (
    export DECKPILOT_PLUGINS_WORK=$work/work DECKPILOT_PLUGINS_GIT_BASE=file://$work/remote
    # Every git process (also a detached child) appends its events here.
    export GIT_TRACE2_EVENT=$work/trace
    # shellcheck source=../lib/common.sh
    . "$root/tools/lib/common.sh"
    # shellcheck disable=SC2329 # rm and sleep, replaced for remove_tree
    if [ -n "$stuck" ]; then
      rm() { echo "rm: cannot remove: Directory not empty" >&2; return 1; }
      sleep() { :; }
    fi
    DECKPILOT_REPOSITORY=acme/deckpilot DECKPILOT_COMMIT=$sha DECKPILOT_SDK_COMMIT=${SDK:-$sha}
    if [ "$fn" = fetch_deckpilot ]; then
      fetch_deckpilot "$work/work/checkout"
    elif [ "$fn" = require_on_main_two ]; then
      require_on_main acme/deckpilot "$sha" "$fork"
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
# The SDK pin (DECKPILOT_SDK_COMMIT: the rev official/ and first-party/
# build against) must be a commit of main too, checked in the same clone.
SDK=$first run pass "fetch_deckpilot with an older SDK commit of main" "$second" fetch_deckpilot
rm -rf "$work/work/checkout"
SDK=$fork run fail "fetch_deckpilot refuses an SDK commit that is a fork's" "$second" fetch_deckpilot
grep -q "$fork is not a commit of acme/deckpilot's main" "$work/out" && [ ! -e "$work/work/checkout" ] &&
  { echo "ok   ... names it and checks nothing out"; passed=$((passed + 1)); } \
  || { echo "FAIL the SDK refusal: $(cat "$work/out")"; failed=$((failed + 1)); }
SDK=$(printf '%040d' 7) run fail "fetch_deckpilot refuses an unknown SDK commit" "$second" fetch_deckpilot
run fail "require_on_main refuses when one of two commits is off main" "$second" require_on_main_two

# check NAME CONDITION...: one more assertion.
check() {
  local name=$1; shift
  if "$@"; then echo "ok   $name"; passed=$((passed + 1)); else echo "FAIL $name"; failed=$((failed + 1)); fi
}
no_leftover() { [ -z "$(find "$work/work" -maxdepth 1 -name 'main-check*' -print -quit)" ]; }
no_background_git() { ! grep -q '"event":"child_start".*"argv":\["git","\(maintenance\|gc\)"' "$work/trace"; }

check "no main-check directory is left behind" no_leftover
check "no background gc or maintenance was started" no_background_git
# The same check twice in a row, and while a leftover of an earlier call
# exists: each call has its own directory.
mkdir -p "$work/work/main-check/.git/objects" && touch "$work/work/main-check/.git/objects/busy"
run pass "a leftover main-check directory does not matter" "$second"
run pass "... nor does a second call right after" "$second"
rm -rf "$work/work/main-check"
check "... and both clean up after themselves" no_leftover
# A directory that cannot be removed is reported, never fatal, and never
# turns a refusal into a pass.
run pass "an unremovable check directory does not fail a commit of main" "$second" require_on_main stuck
grep -q "warning: could not remove" "$work/out" && { echo "ok   ... it is reported"; passed=$((passed + 1)); } \
  || { echo "FAIL no warning: $(cat "$work/out")"; failed=$((failed + 1)); }
run fail "an unremovable check directory still refuses a fork's commit" "$fork" require_on_main stuck
grep -q "not a commit of acme/deckpilot's main" "$work/out" && { echo "ok   ... with the refusal"; passed=$((passed + 1)); } \
  || { echo "FAIL the refusal: $(cat "$work/out")"; failed=$((failed + 1)); }

echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
