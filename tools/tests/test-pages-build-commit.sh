#!/usr/bin/env bash
# Tests of tools/pages-build-commit.sh with a gh stand-in: an empty commit
# on the empty tree, appended to pages-builds (or starting it), its SHA
# printed and written to GITHUB_OUTPUT; every API failure stops it.
#
#   tools/tests/test-pages-build-commit.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/pages-build-commit.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
cat > "$work/bin/gh" << 'STUB'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ] || exit 2; shift
method=GET q=. endpoint="" body=""
while [ $# -gt 0 ]; do
  case "$1" in -X) method=$2; shift 2 ;; --jq) q=$2; shift 2 ;; --input) body=$(cat); shift 2 ;; *) endpoint=$1; shift ;; esac
done
echo "$method $endpoint $body" >> "$STUB/calls"
if [ -n "${STUB_FAIL:-}" ] && [[ "$method $endpoint" =~ $STUB_FAIL ]]; then echo "HTTP 422" >&2; exit 1; fi
case "$method $endpoint" in
  "POST repos/o/r/git/trees") echo '{"sha":"4b825dc642cb6eb9a060e54bf8d69288fbee4904"}' | jq -r "$q" ;;
  "GET repos/o/r/git/ref/heads/pages-builds") [ -f "$STUB/head" ] || { echo "HTTP 404" >&2; exit 1; }; jq -n --arg s "$(cat "$STUB/head")" '{object: {sha: $s}}' | jq -r "$q" ;;
  "POST repos/o/r/git/commits") echo "$body" > "$STUB/commit.json"; echo '{"sha":"1111111111111111111111111111111111111111"}' | jq -r "$q" ;;
  "PATCH repos/o/r/git/refs/heads/pages-builds") echo "$body" > "$STUB/patch.json"; echo '{}' ;;
  "POST repos/o/r/git/refs") echo "$body" > "$STUB/create.json"; echo '{}' ;;
  *) echo "unexpected $method $endpoint" >&2; exit 2 ;;
esac
STUB
chmod +x "$work/bin/gh"
passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }
run() {
  rm -rf "$work/s"; mkdir -p "$work/s"; export STUB="$work/s"
  [ -z "${HEAD_SHA:-}" ] || echo "$HEAD_SHA" > "$STUB/head"
  : > "$work/out.env"
  PATH="$work/bin:$PATH" GH_TOKEN=t GITHUB_REPOSITORY=o/r GITHUB_SHA=0123456789abcdef0123456789abcdef01234567 \
    GITHUB_RUN_ID=42 GITHUB_RUN_ATTEMPT=3 GITHUB_OUTPUT="$work/out.env" "$root/tools/pages-build-commit.sh" > "$work/stdout" 2> "$work/log"
}

if HEAD_SHA=2222222222222222222222222222222222222222 run \
  && [ "$(cat "$work/stdout")" = 1111111111111111111111111111111111111111 ] \
  && grep -qx 'sha=1111111111111111111111111111111111111111' "$work/out.env" \
  && [ "$(jq -c '[.tree, .parents]' "$STUB/commit.json")" = '["4b825dc642cb6eb9a060e54bf8d69288fbee4904",["2222222222222222222222222222222222222222"]]' ] \
  && jq -e '.message | test("0123456789abcdef0123456789abcdef01234567.*run 42, attempt 3")' "$STUB/commit.json" > /dev/null \
  && [ "$(jq -c . "$STUB/patch.json")" = '{"sha":"1111111111111111111111111111111111111111","force":false}' ]; then
  ok "an empty commit on the empty tree, appended to pages-builds without force"
else bad "append: $(cat "$work/log")"; fi
if run && [ "$(jq -c '.parents' "$STUB/commit.json")" = '[]' ] \
  && [ "$(jq -c . "$STUB/create.json")" = '{"ref":"refs/heads/pages-builds","sha":"1111111111111111111111111111111111111111"}' ]; then
  ok "the first build commit starts pages-builds"
else bad "start: $(cat "$work/log")"; fi
for f in 'POST repos/o/r/git/trees' 'POST repos/o/r/git/commits' 'PATCH repos/o/r/git/refs' 'POST repos/o/r/git/refs$'; do
  h=2222222222222222222222222222222222222222; [ "$f" = 'POST repos/o/r/git/refs$' ] && h=""
  if HEAD_SHA=$h STUB_FAIL="^$f" run; then bad "a failing $f passed"
  elif grep -q '::error::' "$work/log" && ! grep -q '^sha=' "$work/out.env"; then ok "a failing $f stops it, no SHA"; else bad "$f: $(cat "$work/log")"; fi
done
if PATH="$work/bin:$PATH" GITHUB_REPOSITORY=o/r "$root/tools/pages-build-commit.sh" > /dev/null 2> "$work/log"; then bad "missing context passed"
elif grep -q "is not set" "$work/log"; then ok "a job without the token fails"; else bad "context: $(cat "$work/log")"; fi
echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
