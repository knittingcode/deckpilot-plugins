#!/usr/bin/env bash
# Tests of official.sh release-sign, release-verify and release-upload, the
# steps of official-release.yml's sign-a/sign-b, verify and release jobs,
# and of test-resolve, test-sign and test-verify, the key test's
# (official-test-sign.yml),
# without a key, a network or the real deckpilot-registry: fail closed on a
# missing or extra secret, one key missing (one signature, a skipped
# signing job), a duplicated or misnamed signature, a DeckPilot that pins
# fewer than two keys, and a release that would not hold exactly the
# package and both signatures. Signature files are fabricated here (only
# their minisign key id matters before verification); the end-to-end
# signing with real throwaway keys is `tools/official.sh sign-test`.
#
#   tools/tests/test-official-release.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/official-release.XXXXXX")
trap 'rm -rf "$work"' EXIT
# A work directory of its own: no real CLI or DeckPilot checkout is used.
export DECKPILOT_PLUGINS_WORK=$work/w
mkdir -p "$DECKPILOT_PLUGINS_WORK"
for v in $(compgen -e | grep '^DECKPILOT_PUBLISHER_KEY' || true); do unset "$v"; done

passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }
# expect <pass|fail> <name> <message regex> -- <command...>
expect() {
  local want=$1 name=$2 msg=$3; shift 4
  local rc=0; "$@" > "$work/out" 2>&1 || rc=$?
  if { [ "$want" = pass ] && [ $rc = 0 ]; } || { [ "$want" = fail ] && [ $rc != 0 ]; }; then
    if grep -qE -- "$msg" "$work/out"; then ok "$name"; else bad "$name: unexpected message: $(tr '\n' ' ' < "$work/out")"; fi
  else bad "$name: exit $rc: $(tr '\n' ' ' < "$work/out")"; fi
}
official() { "$root/tools/official.sh" "$@"; }

ID=deckpilot.git VERSION=1.2.3 NAME=deckpilot.git-1.2.3.dpplugin
pkg_dir=$work/package
mkdir -p "$pkg_dir"
printf 'not really a package\n' > "$pkg_dir/$NAME"
SHA=$(shasum -a 256 "$pkg_dir/$NAME" | cut -d' ' -f1)
SIZE=$(wc -c < "$pkg_dir/$NAME" | tr -d ' ')

# fake_sig KEYNUM_HEX FILE: a file in the shape of a signature
# deckpilot-registry writes (base64 of the four minisign lines) whose key
# number is KEYNUM_HEX (8 bytes, as stored: little-endian).
fake_sig() {
  python3 -I - "$1" "$2" << 'PY'
import base64, sys
raw = b"ED" + bytes.fromhex(sys.argv[1]) + bytes(64)
text = ("untrusted comment: signature from deckpilot-registry\n" + base64.b64encode(raw).decode() +
        "\ntrusted comment: x\n" + base64.b64encode(bytes(64)).decode() + "\n")
open(sys.argv[2], "w").write(base64.b64encode(text.encode()).decode() + "\n")
PY
}
KA=0123456789ABCDEF KB=FEDCBA9876543210
# Stored little-endian: key id 0123456789ABCDEF is the bytes ef cd ab 89 67 45 23 01.
fake_sig efcdab8967452301 "$work/a.sig"
fake_sig 1032547698badcfe "$work/b.sig"
keyid() { python3 -I "$root/tools/lib/minisign_keyid.py" "$1"; }
expect pass "the key id of a signature is minisign's (%016X of the little-endian key number)" "^$KA\$" -- keyid "$work/a.sig"
printf 'untrusted comment: minisign public key: 0123456789ABCDEF\n%s\n' \
  "$(python3 -I -c 'import base64; print(base64.b64encode(b"Ed" + bytes.fromhex("efcdab8967452301") + bytes(32)).decode())')" > "$work/a.pub"
expect pass "the key id of a minisign public key file" "^$KA\$" -- keyid "$work/a.pub"
printf 'garbage' > "$work/garbage.sig"
expect fail "no key id from a file that is not minisign" "neither minisign" -- keyid "$work/garbage.sig"

echo "== release-sign (sign-a, sign-b)"
sign() { "$root/tools/official.sh" release-sign "$@" "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"; }
expect fail "no secret: fails closed" "DECKPILOT_PUBLISHER_KEY_A is not set.*official-release" -- sign A
expect fail "no secret for key B: fails closed" "DECKPILOT_PUBLISHER_KEY_B is not set" -- sign B
expect fail "key A without its password" "DECKPILOT_PUBLISHER_KEY_A_PASSWORD is not set" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k "$root/tools/official.sh" release-sign A "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"
expect fail "an empty secret is a missing one" "DECKPILOT_PUBLISHER_KEY_B is not set" -- \
  env DECKPILOT_PUBLISHER_KEY_B= DECKPILOT_PUBLISHER_KEY_B_PASSWORD=p "$root/tools/official.sh" release-sign B "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"
expect fail "the job of key A also has key B" "also has DECKPILOT_PUBLISHER_KEY_B.*own job" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p DECKPILOT_PUBLISHER_KEY_B=k \
  "$root/tools/official.sh" release-sign A "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"
expect fail "the job of key B also has key A's password" "also has DECKPILOT_PUBLISHER_KEY_A" -- \
  env DECKPILOT_PUBLISHER_KEY_B=k DECKPILOT_PUBLISHER_KEY_B_PASSWORD=p DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p \
  "$root/tools/official.sh" release-sign B "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"
# The retired single key (its name assembled: nothing references it) or any
# other publisher secret next to the job's own key.
retired=DECKPILOT_PUBLISHER_"KEY"
expect fail "the retired single key" "$retired is in the environment of the job signing with key A" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$retired=k" \
  "$root/tools/official.sh" release-sign A "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"
expect fail "a third publisher key" "DECKPILOT_PUBLISHER_KEY_C is in the environment" -- \
  env DECKPILOT_PUBLISHER_KEY_B=k DECKPILOT_PUBLISHER_KEY_B_PASSWORD=p DECKPILOT_PUBLISHER_KEY_C=k \
  "$root/tools/official.sh" release-sign B "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"
expect fail "a third key slot" "A or B" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$root/tools/official.sh" release-sign C "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"
expect fail "a package other than the compare job's" "not the package the build jobs produced" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$root/tools/official.sh" release-sign A "$ID" "$pkg_dir" "$VERSION" "$(printf '0%.0s' {1..64})" "$SIZE" "$work/out-sig"
mkdir -p "$work/full"; touch "$work/full/x"
expect fail "an output directory that already holds something" "new or empty" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$root/tools/official.sh" release-sign A "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/full"
expect fail "every check passed: only then the CLI is needed" "deckpilot-registry is missing" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$root/tools/official.sh" release-sign A "$ID" "$pkg_dir" "$VERSION" "$SHA" "$SIZE" "$work/out-sig"

echo "== release-verify (verify)"
# A stand-in deckpilot-registry: `release verify` accepts exactly two
# signature files next to the package or in --signatures-dir (or any number with STUB_ACCEPT_ONE=1) and writes the audit
# record a real verification writes.
mkdir -p "$DECKPILOT_PLUGINS_WORK/bin"
cat > "$DECKPILOT_PLUGINS_WORK/bin/deckpilot-registry" << 'STUB'
#!/usr/bin/env bash
set -euo pipefail
command="$1 $2"
audit="" dir="" pkg="" run="" out=""
while [ $# -gt 0 ]; do
  case "$1" in
    --signatures-dir) dir=$2; shift 2 ;;
    --package) pkg=$2; shift 2 ;;
    --audit) audit=$2; shift 2 ;;
    --run) run=$2; shift 2 ;;
    --out-dir) out=$2; shift 2 ;;
    *) shift ;;
  esac
done
case $command in
  "release test-message")
    printf 'deckpilot-publisher-test v1 run:%s\n' "$run" > "$out/deckpilot-publisher-test-v$run.txt"; exit 0 ;;
  "release test-verify")
    n=$(find "$dir" -maxdepth 1 -name "deckpilot-publisher-test-v$run.txt.*.minisig" | wc -l | tr -d ' ')
    if [ "$n" != 2 ] && [ -z "${STUB_ACCEPT_ONE:-}" ]; then
      echo "error: key test $run refused: $n valid signature(s), 2 required" >&2; exit 1
    fi
    echo "key test $run verified"; exit 0 ;;
  "release verify") ;;
  *) echo "stub: $command" >&2; exit 2 ;;
esac
# The key test message is never a release (its comment is another domain),
# unless STUB_ACCEPT_TEST says otherwise.
if head -c 24 "$pkg" | grep -q '^deckpilot-publisher-test' && [ -z "${STUB_ACCEPT_TEST:-}" ]; then
  echo "error: refused: 0 valid signature(s) from distinct pinned publisher keys, 2 required" >&2; exit 1
fi
n=$(find "${dir:-$(dirname "$pkg")}" -maxdepth 1 -name "$(basename "$pkg").*.minisig" | wc -l | tr -d ' ')
if [ "$n" != 2 ] && [ -z "${STUB_ACCEPT_ONE:-}" ]; then
  echo "error: refused: $n valid signature(s) from distinct keys of the role, 2 required" >&2; exit 1
fi
[ -z "$audit" ] || echo '{"outcome":"verified","signers":["aaaaaaaaaaaaaaaa","bbbbbbbbbbbbbbbb"]}' >> "$audit"
echo "verified"
STUB
printf '#!/bin/sh\nexit 0\n' > "$DECKPILOT_PLUGINS_WORK/bin/schema-check"
chmod +x "$DECKPILOT_PLUGINS_WORK/bin/"*
pinned=$DECKPILOT_PLUGINS_WORK/deckpilot/crates/deckpilot-plugin-registry/publisher/first-party.json
mkdir -p "$(dirname "$pinned")" "$DECKPILOT_PLUGINS_WORK/deckpilot/plugins/first-party"
# pin THRESHOLD N [KEYID...]: DeckPilot's first-party.json with N public
# keys in minisign's format, of these key ids (default: KA, KB, then others).
pin() {
  local t=$1 n=$2; shift 2
  python3 -I - "$t" "$n" "$pinned" "$@" << 'PY'
import base64, json, sys
t, n, out, ids = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3], sys.argv[4:]
ids = (ids or ["0123456789ABCDEF", "FEDCBA9876543210", "1111111111111111", "2222222222222222"])[:n]
def key(kid):
    raw = b"Ed" + bytes.fromhex(kid)[::-1] + bytes(32)
    text = f"untrusted comment: minisign public key: {kid}\n{base64.b64encode(raw).decode()}\n"
    return base64.b64encode(text.encode()).decode()
json.dump({"publisherKeysVersion": 1, "publisher": "deckpilot", "keys": [key(k) for k in ids],
           "threshold": t, "revokedKeys": []}, open(out, "w"))
PY
}
pin 2 2
# set NAME FILE...: a release directory with the package and these signatures (SRC:DEST).
release_dir() {
  local d=$work/r-$1; shift
  rm -rf "$d"; mkdir -p "$d"
  cp "$pkg_dir/$NAME" "$d/"
  local f
  for f in "$@"; do cp "$work/${f%%:*}" "$d/${f#*:}"; done
  echo "$d"
}
verify() { local d=$1; shift; official release-verify "$ID" "$d" "$VERSION" "$SHA" "$SIZE" "$@"; }
two=$(release_dir two a.sig:"$NAME.$KA.minisig" b.sig:"$NAME.$KB.minisig")
expect pass "two signatures by distinct keys, the pinned keys at threshold 2" "two distinct pinned publisher keys" -- verify "$two" "$KA" "$KB"
jq -se 'length == 3' "$two/publisher-audit.jsonl" > /dev/null && ok "audited for every platform" || bad "audit: $(cat "$two/publisher-audit.jsonl")"
expect fail "a publisher key in the verify job" "without any publisher key" -- env DECKPILOT_PUBLISHER_KEY_B=k "$root/tools/official.sh" release-verify "$ID" "$two" "$VERSION" "$SHA" "$SIZE" "$KA" "$KB"
expect fail "sign-b skipped or failed: no key id (one key missing, no release)" "no release without both signatures" -- verify "$two" "$KA" ""
expect fail "sign-a and sign-b used the same key" "same key" -- verify "$two" "$KA" "$KA"
one=$(release_dir one a.sig:"$NAME.$KA.minisig")
expect fail "one signature (one key missing, no release)" "has 1 signature\\(s\\).*exactly two" -- verify "$one" "$KA" "$KB"
none=$(release_dir none)
expect fail "no signature" "has 0 signature" -- verify "$none" "$KA" "$KB"
dup=$(release_dir dup a.sig:"$NAME.$KA.minisig" a.sig:"$NAME.$KB.minisig")
expect fail "a duplicated signature under the other key's name" "carries key id $KA, not $KB" -- verify "$dup" "$KA" "$KB"
fake_sig 0000000000000001 "$work/c.sig"
three=$(release_dir three a.sig:"$NAME.$KA.minisig" b.sig:"$NAME.$KB.minisig" c.sig:"$NAME.0100000000000000.minisig")
expect fail "a third signature" "has 3 signature" -- verify "$three" "$KA" "$KB"
other=$(release_dir other a.sig:"$NAME.$KA.minisig" c.sig:"$NAME.0100000000000000.minisig")
expect fail "a signature by a key sign-b did not use" "not the keys sign-a and sign-b used" -- verify "$other" "$KA" "$KB"
old=$(release_dir old a.sig:"$NAME.minisig" b.sig:"$NAME.$KB.minisig")
expect fail "the single-key file name" "is not a signature file .*<KEYID>" -- verify "$old" "$KA" "$KB"
lower=$(release_dir lower a.sig:"$NAME.0123456789abcdef.minisig" b.sig:"$NAME.$KB.minisig")
expect fail "a key id in lower case" "is not a signature file" -- verify "$lower" "$KA" "$KB"
printf 'x' >> "$two/$NAME"
expect fail "modified package bytes" "not the package the build jobs produced" -- verify "$two" "$KA" "$KB"
two=$(release_dir two a.sig:"$NAME.$KA.minisig" b.sig:"$NAME.$KB.minisig")
pin 1 1
expect fail "DeckPilot pins one key (threshold 1)" "pins 1 publisher key\\(s\\), threshold 1.*at least 2" -- verify "$two" "$KA" "$KB"
pin 1 2
expect fail "DeckPilot pins two keys, threshold 1" "threshold 1.*at least 2" -- verify "$two" "$KA" "$KB"
pin 2 0
expect fail "DeckPilot pins no key" "pins 0 publisher key" -- verify "$two" "$KA" "$KB"
pin 2 2 "$KA" 1111111111111111
expect fail "a signature by a key DeckPilot does not pin" "key $KB signed, but DeckPilot .* does not pin it" -- verify "$two" "$KA" "$KB"
pin 2 2
expect fail "the pinned verifier accepts one signature alone" "accept .* alone: the threshold is not two" -- env STUB_ACCEPT_ONE=1 "$root/tools/official.sh" release-verify "$ID" "$two" "$VERSION" "$SHA" "$SIZE" "$KA" "$KB"

echo "== the key test (official-test-sign.yml)"
# (A well-formed tag then needs its commit on main: not testable on a pull
# request's merge commit; official-test-sign.yml's resolve job checks it.)
for tag in deckpilot.publisher-test--v0 deckpilot.publisher-test--v07 deckpilot.publisher-test--v10000 \
  deckpilot.publisher-test--v1.0.0 deckpilot.git--v1.2.3 deckpilot.publisher-test registry--v1; do
  expect fail "not a key test tag: $tag" "is not a key test tag" -- official test-resolve "$tag"
done
tsign() { "$root/tools/official.sh" test-sign "$@"; }
expect fail "key test: no secret, fails closed" "DECKPILOT_PUBLISHER_KEY_A is not set.*official-release" -- tsign A 1 "$work/t-sig"
expect fail "key test: key B without its password" "DECKPILOT_PUBLISHER_KEY_B_PASSWORD is not set" -- \
  env DECKPILOT_PUBLISHER_KEY_B=k "$root/tools/official.sh" test-sign B 1 "$work/t-sig"
expect fail "key test: the job of key A also has key B" "also has DECKPILOT_PUBLISHER_KEY_B.*own job" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p DECKPILOT_PUBLISHER_KEY_B_PASSWORD=p \
  "$root/tools/official.sh" test-sign A 1 "$work/t-sig"
expect fail "key test: the retired single key" "$retired is in the environment of the job signing with key B" -- \
  env DECKPILOT_PUBLISHER_KEY_B=k DECKPILOT_PUBLISHER_KEY_B_PASSWORD=p "$retired=k" \
  "$root/tools/official.sh" test-sign B 1 "$work/t-sig"
expect fail "key test: a third key slot" "A or B" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$root/tools/official.sh" test-sign C 1 "$work/t-sig"
expect fail "key test: a run that is not a number" "1 to 9999" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$root/tools/official.sh" test-sign A x "$work/t-sig"
expect fail "key test: an output directory that already holds something" "new or empty" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k DECKPILOT_PUBLISHER_KEY_A_PASSWORD=p "$root/tools/official.sh" test-sign A 1 "$work/full"
TNAME=deckpilot-publisher-test-v1.txt
# key_test_dir NAME FILE...: a directory with these key test signatures (SRC:DEST).
key_test_dir() {
  local d=$work/t-$1; shift
  rm -rf "$d"; mkdir -p "$d"
  local f
  for f in "$@"; do cp "$work/${f%%:*}" "$d/${f#*:}"; done
  echo "$d"
}
tverify() { official test-verify 1 "$@"; }
pin 2 2
ttwo=$(key_test_dir two a.sig:"$TNAME.$KA.minisig" b.sig:"$TNAME.$KB.minisig")
expect pass "key test: both pinned keys signed, one alone refused, never a release" \
  "release verify refuses it as a release of [0-9]+ id" -- tverify "$ttwo" "$KA" "$KB"
expect fail "key test: a publisher key in the verify job" "test-verify runs without any publisher key" -- \
  env DECKPILOT_PUBLISHER_KEY_A=k "$root/tools/official.sh" test-verify 1 "$ttwo" "$KA" "$KB"
expect fail "key test: sign-b skipped (no key id)" "reported no key id" -- tverify "$ttwo" "$KA" ""
expect fail "key test: the same key twice" "same key" -- tverify "$ttwo" "$KA" "$KA"
tone=$(key_test_dir one a.sig:"$TNAME.$KA.minisig")
expect fail "key test: one signature" "has 1 signature\\(s\\)" -- tverify "$tone" "$KA" "$KB"
tdup=$(key_test_dir dup a.sig:"$TNAME.$KA.minisig" a.sig:"$TNAME.$KB.minisig")
expect fail "key test: a duplicated signature under the other key's name" "carries key id $KA, not $KB" -- tverify "$tdup" "$KA" "$KB"
expect fail "key test: another run's signatures" "is not a signature file deckpilot-publisher-test-v2.txt" -- official test-verify 2 "$ttwo" "$KA" "$KB"
pin 2 2 "$KA" 1111111111111111
expect fail "key test: a key DeckPilot does not pin" "key $KB signed, but DeckPilot .* does not pin it" -- tverify "$ttwo" "$KA" "$KB"
pin 1 2
expect fail "key test: DeckPilot pins threshold 1" "threshold 1.*at least 2" -- tverify "$ttwo" "$KA" "$KB"
pin 2 2
expect fail "key test: the pinned verifier accepts one signature alone" "alone: the threshold is not two" -- \
  env STUB_ACCEPT_ONE=1 "$root/tools/official.sh" test-verify 1 "$ttwo" "$KA" "$KB"
expect fail "key test: release verify would accept the test signatures as a release" "accepts the key test signatures as a release" -- \
  env STUB_ACCEPT_TEST=1 "$root/tools/official.sh" test-verify 1 "$ttwo" "$KA" "$KB"

echo "== release-upload (release)"
# A stand-in gh: one release in $work/gh.json; `upload` adds an asset,
# `edit --draft=false` publishes; every call is logged.
cat > "$work/gh" << 'STUB'
#!/usr/bin/env bash
set -euo pipefail
echo "$*" >> "$GH_LOG"
[ "$1" = release ] || exit 2
case "$2" in
  view)
    [ -f "$GH_STATE" ] || exit 1
    q=.; for ((i = 1; i <= $#; i++)); do [ "${!i}" = -q ] && { j=$((i + 1)); q=${!j}; }; done
    jq -r "$q" "$GH_STATE" ;;
  create) echo '{"isDraft": true, "assets": []}' > "$GH_STATE" ;;
  upload)
    f=${!#}
    jq --arg n "$(basename "$f")" --arg d "sha256:$(shasum -a 256 "$f" | cut -d' ' -f1)" '.assets += [{name: $n, digest: $d}]' "$GH_STATE" > "$GH_STATE.t"
    mv "$GH_STATE.t" "$GH_STATE" ;;
  edit) jq '.isDraft = false' "$GH_STATE" > "$GH_STATE.t" && mv "$GH_STATE.t" "$GH_STATE" ;;
esac
STUB
chmod +x "$work/gh"
mkdir -p "$work/ghbin"; ln -sf "$work/gh" "$work/ghbin/gh"
upload() { PATH="$work/ghbin:$PATH" GH_LOG=$work/gh.log GH_STATE=$work/gh.json official release-upload "$ID--v$VERSION" "$ID" "$VERSION" "$@" "$SHA" "$SIZE"; }
rm -f "$work/gh.json" "$work/gh.log"
expect fail "one signature: nothing is uploaded" "exactly two" -- upload "$one"
[ ! -s "$work/gh.log" ] && ok "gh was never called for one signature" || bad "gh called: $(cat "$work/gh.log")"
expect pass "the package and both signatures, draft first, then published" "release-upload passed" -- upload "$two"
[ "$(jq -r '[.assets[].name] | sort | join(" ")' "$work/gh.json")" = "$NAME $NAME.$KA.minisig $NAME.$KB.minisig" ] \
  && ok "the release holds the package and both signature files" || bad "assets: $(cat "$work/gh.json")"
grep -q -- "create .*--draft" "$work/gh.log" && [ "$(jq .isDraft "$work/gh.json")" = false ] \
  && ok "created as a draft, published after the uploads" || bad "gh calls: $(cat "$work/gh.log")"
: > "$work/gh.log"
expect pass "re-running on the complete release is a no-op" "release-upload passed" -- upload "$two"
grep -Eq "^release (upload|create)" "$work/gh.log" && bad "the re-run uploaded: $(cat "$work/gh.log")" || ok "the re-run uploaded nothing"
echo '{"isDraft": true, "assets": [{"name": "'"$NAME"'.minisig", "digest": "sha256:00"}]}' > "$work/gh.json"
expect fail "a release with another asset (an old single signature) stays a draft" "not exactly.*not publishing" -- upload "$two"
[ "$(jq .isDraft "$work/gh.json")" = true ] && ok "the draft was not published" || bad "published: $(cat "$work/gh.json")"

echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
