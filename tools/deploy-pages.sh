#!/usr/bin/env bash
# Deploys this run's `github-pages` artifact (actions/upload-pages-artifact)
# to GitHub Pages, as actions/deploy-pages does, but under the build commit
# PAGES_BUILD_COMMIT (tools/pages-build-commit.sh) instead of GITHUB_SHA.
#
# actions/deploy-pages always uses the commit of the run as the
# pages_build_version, and Pages answers a second deployment of the same
# version with the existing one: it reports success and serves the old
# content (observed with deckpilot.memory--v0.1.0: its tag is on the main
# commit pages.yml had just deployed, and the signed registry never went
# live). A version that is not a real commit is refused with 404 (observed
# with "<commit>-<run>-<attempt>"), so each deployment gets a commit of its
# own.
#
#   PAGES_BUILD_COMMIT=<sha> tools/deploy-pages.sh
#                                    (in a job with pages: write, id-token: write,
#                                     actions: read and the github-pages environment)
#
# Writes page_url to $GITHUB_OUTPUT. Prints no token.
# Environment: GH_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID, PAGES_BUILD_COMMIT,
# ACTIONS_ID_TOKEN_REQUEST_URL and _TOKEN (set by the
# runner for id-token: write); GH (default: gh); DEPLOY_TIMEOUT_SECONDS
# (default 600, the action's maximum).
set -euo pipefail

gh=${GH:-gh}
fail() { echo "::error::$*" >&2; exit 1; }
for v in GH_TOKEN GITHUB_REPOSITORY GITHUB_RUN_ID PAGES_BUILD_COMMIT ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_ID_TOKEN_REQUEST_TOKEN; do
  [ -n "${!v:-}" ] || fail "$v is not set (the job needs pages: write, id-token: write and actions: read)"
done
repo=$GITHUB_REPOSITORY
[[ $repo =~ ^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$ ]] || fail "bad GITHUB_REPOSITORY"
[[ $GITHUB_RUN_ID =~ ^[0-9]+$ ]] || fail "bad run context"
[[ $PAGES_BUILD_COMMIT =~ ^[0-9a-f]{40}$ ]] || fail "PAGES_BUILD_COMMIT is not a commit SHA"
version=$PAGES_BUILD_COMMIT
timeout=${DEPLOY_TIMEOUT_SECONDS:-600}

# The one github-pages artifact of this run.
artifact=""
for _ in 1 2 3 4 5 6; do
  artifact=$("$gh" api "repos/$repo/actions/runs/$GITHUB_RUN_ID/artifacts?name=github-pages&per_page=100" \
    --jq '[.artifacts[] | select(.name == "github-pages" and (.expired | not))] | if length == 1 then .[0].id else empty end') \
    || fail "the artifacts of run $GITHUB_RUN_ID cannot be listed"
  [ -n "$artifact" ] && break
  sleep 5
done
[[ $artifact =~ ^[0-9]+$ ]] || fail "run $GITHUB_RUN_ID has no single github-pages artifact"

# An OIDC token for Pages (default audience, as @actions/core getIDToken()).
oidc=$(curl -fsS --proto '=https' -H "Authorization: bearer $ACTIONS_ID_TOKEN_REQUEST_TOKEN" "$ACTIONS_ID_TOKEN_REQUEST_URL" | jq -r '.value // empty') \
  || fail "no OIDC token (id-token: write)"
[ -n "$oidc" ] || fail "no OIDC token (id-token: write)"
echo "::add-mask::$oidc"

created=$(jq -n --argjson a "$artifact" --arg v "$version" --arg t "$oidc" \
    '{artifact_id: $a, pages_build_version: $v, oidc_token: $t}' \
  | "$gh" api -X POST "repos/$repo/pages/deployments" --input -) \
  || fail "Pages refused the deployment of artifact $artifact (build version $version)"
unset oidc
id=$(jq -r '(.id // empty) | tostring' <<< "$created")
[ -n "$id" ] || id=$(jq -r '.status_url // "" | split("/") | last' <<< "$created")
[ -n "$id" ] || fail "Pages returned no deployment id"
page_url=$(jq -r '.page_url // empty' <<< "$created")
echo "created Pages deployment $id for build version $version (artifact $artifact)"

deadline=$(( $(date +%s) + timeout ))
interval=5 errors=0
while :; do
  sleep "$interval"
  if status=$("$gh" api "repos/$repo/pages/deployments/$id" --jq '.status' 2> /dev/null); then
    errors=0
    case "$status" in
      succeed) break ;;
      deployment_failed | deployment_content_failed | deployment_cancelled | deployment_lost)
        fail "Pages deployment $id ended with $status" ;;
      *) echo "status: ${status:-unknown}" ;;
    esac
  else
    errors=$((errors + 1))
    [ "$errors" -lt 10 ] || fail "Pages deployment $id: too many errors reading its status"
  fi
  if [ "$(date +%s)" -ge "$deadline" ]; then
    "$gh" api -X POST "repos/$repo/pages/deployments/$id/cancel" > /dev/null 2>&1 || true
    fail "Pages deployment $id did not finish within ${timeout}s (cancelled)"
  fi
  interval=$(( interval * 3 / 2 )); [ "$interval" -le 30 ] || interval=30
done
echo "Pages deployment $id succeeded: $page_url"
[ -z "${GITHUB_OUTPUT:-}" ] || echo "page_url=$page_url" >> "$GITHUB_OUTPUT"
