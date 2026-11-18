#!/usr/bin/env bash
# Tests of tools/check-first-party-only.sh: this repository's data passes;
# a third-party publisher record, a third-party plugin, a plugin under a
# reserved-looking but unlisted namespace, and a key set without
# first-party namespaces are refused; the newest key set decides.
#
#   tools/tests/test-first-party-only.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/first-party-only.XXXXXX")
trap 'rm -rf "$work"' EXIT
export DECKPILOT_PLUGINS_WORK=$work/dot-work
passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }
fixture() { # a copy of this repository's keys/ and registry/
  rm -rf "$work/r"; mkdir -p "$work/r"
  cp -R "$root/keys" "$root/registry" "$work/r/"
}
expect() { # expect NAME pass|fail [ERROR_TEXT]
  if "$root/tools/check-first-party-only.sh" "$work/r" > /dev/null 2> "$work/err"; then got=pass; else got=fail; fi
  if [ "$got" != "$2" ]; then bad "$1: got $got ($(tail -1 "$work/err"))"; return; fi
  if [ -n "${3:-}" ] && ! grep -q -- "$3" "$work/err"; then bad "$1: error lacks '$3': $(cat "$work/err")"; return; fi
  ok "$1"
}

fixture
expect "this repository's registry is first-party only" pass

fixture
jq -n '{namespace: "acme", github_id: 1, login: "acme", display_name: "Acme"}' > "$work/r/registry/publishers/acme.json"
expect "a third-party publisher record is refused" fail "publisher acme (acme.json) is not first-party"

fixture
mkdir -p "$work/r/registry/plugins/acme.lights/versions"
expect "a third-party plugin is refused" fail "plugin acme.lights is not in a first-party namespace"

fixture
mkdir -p "$work/r/registry/plugins/deckpilotx.tool" "$work/r/registry/plugins/knittingcode.tool"
expect "only the exact namespace counts (deckpilotx, knittingcode)" fail "2 non-first-party entries"

fixture
jq '.first_party.namespaces = []' "$root/keys/1.json" > "$work/r/keys/1.json"
expect "a key set without first-party namespaces is refused" fail "names no first-party namespace"

fixture
jq '.first_party.namespaces = ["deckpilot", "acme"]' "$root/keys/1.json" > "$work/r/keys/2.json"
mkdir -p "$work/r/registry/plugins/acme.lights"
expect "the newest key set decides (keys/2.json lists acme)" pass
cp "$root/keys/1.json" "$work/r/keys/10.json"
expect "key sets are ordered numerically (keys/10.json is newest)" fail "plugin acme.lights"

echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
