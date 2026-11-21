#!/usr/bin/env bash
# Tests of tools/cli-artifact.sh with stand-in binaries: pack reports the
# sha256, install accepts exactly those binaries and refuses a changed one,
# a missing one and malformed sums. No network, no cargo.
#
#   tools/tests/test-cli-artifact.sh
set -euo pipefail
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/cli-artifact.XXXXXX")
trap 'rm -rf "$work"' EXIT
export DECKPILOT_PLUGINS_WORK=$work/build
mkdir -p "$work/build/bin"
printf '#!/bin/sh\necho "deckpilot-registry: stand-in"\n' > "$work/build/bin/deckpilot-registry"
printf '#!/bin/sh\necho schema-check\n' > "$work/build/bin/schema-check"
chmod +x "$work/build/bin/"*

passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }

out=$("$root/tools/cli-artifact.sh" pack "$work/artifact")
registry=$(sed -n 's/^registry=//p' <<<"$out")
schema=$(sed -n 's/^schema=//p' <<<"$out")
[[ $registry =~ ^[0-9a-f]{64}$ && $schema =~ ^[0-9a-f]{64}$ ]] && ok "pack reports both sha256" || bad "pack: $out"

# The signing job: a fresh work directory, the artifact without exec bits.
install_into() {
  DECKPILOT_PLUGINS_WORK=$work/sign "$root/tools/cli-artifact.sh" install "$@"
}
chmod -x "$work/artifact/"*
if install_into "$work/artifact" "$registry" "$schema" >/dev/null 2>&1 && [ -x "$work/sign/bin/deckpilot-registry" ]; then
  ok "install accepts the packed binaries"
else
  bad "install refused the packed binaries"
fi
rm -rf "$work/sign"
cp -R "$work/artifact" "$work/changed"
echo "# changed" >> "$work/changed/deckpilot-registry"
install_into "$work/changed" "$registry" "$schema" >/dev/null 2>&1 && bad "a changed binary installed" || ok "a changed binary is refused"
[ ! -e "$work/sign/bin/deckpilot-registry" ] && ok "... and nothing is installed" || bad "a binary was installed"
install_into "$work/artifact" "" "$schema" >/dev/null 2>&1 && bad "no sum accepted" || ok "a missing sum is refused"
install_into "$work/artifact" "$registry" "nothex" >/dev/null 2>&1 && bad "a malformed sum accepted" || ok "a malformed sum is refused"
rm "$work/changed/schema-check"
install_into "$work/changed" "$registry" "$schema" >/dev/null 2>&1 && bad "a missing binary accepted" || ok "a missing binary is refused"

echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
