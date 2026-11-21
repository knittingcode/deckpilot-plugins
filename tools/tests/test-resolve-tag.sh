#!/usr/bin/env bash
# Tests of tools/resolve-tag.sh's own refusals (before any tag lookup): the
# plugins DeckPilot bundles are never registry publications, and malformed
# tags are refused. No network.
#
#   tools/tests/test-resolve-tag.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
passed=0 failed=0
expect_fail() {
  local tag=$1 message=$2 out
  if out=$("$root/tools/resolve-tag.sh" "$tag" 2>&1); then
    echo "FAIL $tag: accepted"; failed=$((failed + 1))
  elif grep -qF -- "$message" <<<"$out"; then
    echo "ok   $tag refused"; passed=$((passed + 1))
  else
    echo "FAIL $tag: $out"; failed=$((failed + 1))
  fi
}
for id in docker git github gitlab jira notifications projects workflows workspace; do
  expect_fail "deckpilot.$id--v9.9.9" "is a plugin DeckPilot bundles"
done
expect_fail "deckpilot.publisher-test--v1" "is a publisher key test tag"
expect_fail "deckpilot.publisher-test--v42" "is a publisher key test tag"
expect_fail "v1.0.0" "is not a publication tag"
expect_fail "../x--v1" "is not a publication tag"
echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
