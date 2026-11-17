#!/usr/bin/env bash
# Tests of tools/deploy-pages.sh with gh and curl stand-ins: the build version
# is the given build commit, the payload names this run's artifact, and
# every final Pages status, a missing artifact, a refused creation and a
# timeout fail. No network, no token.
#
#   tools/tests/test-deploy-pages.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d "${TMPDIR:-/tmp}/deploy-pages.XXXXXX")
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/bin"
cat > "$work/bin/gh" << 'STUB'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ] || exit 2; shift
method=GET q=. endpoint=""
while [ $# -gt 0 ]; do
  case "$1" in -X) method=$2; shift 2 ;; --jq) q=$2; shift 2 ;; --input) shift 2 ;; *) endpoint=$1; shift ;; esac
done
echo "$method $endpoint" >> "$STUB/calls"
case "$method $endpoint" in
  "GET repos/o/r/actions/runs/42/artifacts"*) jq -r "$q" "$STUB/artifacts.json" ;;
  "POST repos/o/r/pages/deployments") cat > "$STUB/payload.json"; [ -z "${STUB_REFUSE:-}" ] || { echo "HTTP 400" >&2; exit 1; }
    jq -n --arg v "$(jq -r .pages_build_version "$STUB/payload.json")" '{id: $v, status_url: ("https://api.github.com/repos/o/r/pages/deployments/" + $v), page_url: "https://o.github.io/r/"}' ;;
  "GET repos/o/r/pages/deployments/"*) n=$(cat "$STUB/n" 2> /dev/null || echo 0); echo $((n + 1)) > "$STUB/n"
    s=$(sed -n "$((n + 1))p" "$STUB/statuses"); [ -n "$s" ] || s=$(tail -1 "$STUB/statuses"); jq -n --arg s "$s" '{status: $s}' | jq -r "$q" ;;
  "POST repos/o/r/pages/deployments/"*/cancel) echo cancelled > "$STUB/cancelled" ;;
  *) echo "unexpected $method $endpoint" >&2; exit 2 ;;
esac
STUB
printf '#!/usr/bin/env bash\necho "{\\"value\\": \\"oidc-test-token\\"}"\n' > "$work/bin/curl"
printf '#!/usr/bin/env bash\n:\n' > "$work/bin/sleep"
chmod +x "$work/bin/"*

passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }
run() {   # run <statuses...>: one status per poll
  rm -rf "$work/s"; mkdir -p "$work/s"; export STUB="$work/s"
  printf '%s\n' "$@" > "$STUB/statuses"
  echo '{"artifacts":[{"id":7,"name":"github-pages","expired":false},{"id":8,"name":"website","expired":false}]}' > "$STUB/artifacts.json"
  [ -z "${ARTIFACTS:-}" ] || echo "$ARTIFACTS" > "$STUB/artifacts.json"
  : > "$work/out.env"
  PATH="$work/bin:$PATH" GH_TOKEN=t GITHUB_REPOSITORY=o/r GITHUB_SHA=0123456789abcdef0123456789abcdef01234567 \
    GITHUB_RUN_ID=42 GITHUB_RUN_ATTEMPT=3 PAGES_BUILD_COMMIT=${BUILD:-fedcba9876543210fedcba9876543210fedcba98} ACTIONS_ID_TOKEN_REQUEST_URL=https://token ACTIONS_ID_TOKEN_REQUEST_TOKEN=r \
    GITHUB_OUTPUT="$work/out.env" DEPLOY_TIMEOUT_SECONDS=${TIMEOUT:-600} "$root/tools/deploy-pages.sh" > "$work/log" 2>&1
}

if run deployment_queued syncing_files succeed \
  && [ "$(jq -r .pages_build_version "$STUB/payload.json")" = fedcba9876543210fedcba9876543210fedcba98 ] \
  && [ "$(jq -r .artifact_id "$STUB/payload.json")" = 7 ] && [ "$(jq -r .oidc_token "$STUB/payload.json")" = oidc-test-token ] \
  && grep -qx 'page_url=https://o.github.io/r/' "$work/out.env" && ! grep -v "^::add-mask::" "$work/log" | grep -q oidc-test-token; then
  ok "deploys this run's artifact under the build commit, waits for succeed, prints no token"
else bad "a good deployment: $(cat "$work/log")"; fi
for s in deployment_failed deployment_content_failed deployment_cancelled deployment_lost; do
  if run deployment_queued "$s"; then bad "$s passed"; elif grep -q "ended with $s" "$work/log"; then ok "$s fails"; else bad "$s: $(cat "$work/log")"; fi
done
if run deployment_attempt_error succeed; then ok "a temporary deployment_attempt_error is waited out"; else bad "attempt error: $(cat "$work/log")"; fi
if ARTIFACTS='{"artifacts":[]}' run succeed; then bad "no artifact passed"; elif grep -q "no single github-pages artifact" "$work/log"; then ok "no github-pages artifact fails"; else bad "no artifact: $(cat "$work/log")"; fi
if ARTIFACTS='{"artifacts":[{"id":7,"name":"github-pages","expired":false},{"id":9,"name":"github-pages","expired":false}]}' run succeed; then bad "two artifacts passed"
elif grep -q "no single github-pages artifact" "$work/log"; then ok "two github-pages artifacts fail"; else bad "two artifacts: $(cat "$work/log")"; fi
if STUB_REFUSE=1 run succeed; then bad "a refused creation passed"; elif grep -q "Pages refused" "$work/log"; then ok "a refused creation fails"; else bad "refused: $(cat "$work/log")"; fi
if TIMEOUT=0 run deployment_queued; then bad "a timeout passed"; elif grep -q "did not finish" "$work/log" && [ -f "$STUB/cancelled" ]; then ok "a timeout cancels the deployment and fails"; else bad "timeout: $(cat "$work/log")"; fi
if PATH="$work/bin:$PATH" GITHUB_REPOSITORY=o/r "$root/tools/deploy-pages.sh" > "$work/log" 2>&1; then bad "missing context passed"
elif grep -q "is not set" "$work/log"; then ok "a job without the token or OIDC fails"; else bad "context: $(cat "$work/log")"; fi

if BUILD=main run succeed; then bad "a build version that is not a commit passed"
elif grep -q "not a commit SHA" "$work/log"; then ok "a build version that is not a commit fails"; else bad "build version: $(cat "$work/log")"; fi

echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
