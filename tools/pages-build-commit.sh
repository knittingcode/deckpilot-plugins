#!/usr/bin/env bash
# Makes the commit a Pages deployment is recorded under, and prints its SHA
# (also as `sha` in $GITHUB_OUTPUT).
#
# Pages keys a deployment by its build version: a second deployment of a
# version it has seen returns the existing one, reports success and changes
# nothing; and a build version that is not a real commit is refused (404).
# The commit of the run cannot serve: a release tag is made on the main
# commit pages.yml has just deployed, and the daily timestamp refresh
# deploys the same main commit day after day. So every deployment gets a
# commit of its own: a commit whose only file, BUILD, names the run it was
# made for, appended to the branch `pages-builds`, which nothing builds
# from and no workflow runs on. (The Git data API refuses an empty tree
# with 422 "Invalid tree info".)
#
#   tools/pages-build-commit.sh        (job permissions: contents: write only)
#
# Environment: GH_TOKEN, GITHUB_REPOSITORY, GITHUB_SHA, GITHUB_RUN_ID,
# GITHUB_RUN_ATTEMPT; GH (default: gh). Prints no token.
set -euo pipefail

gh=${GH:-gh}
fail() { echo "::error::$*" >&2; exit 1; }
for v in GH_TOKEN GITHUB_REPOSITORY GITHUB_SHA GITHUB_RUN_ID GITHUB_RUN_ATTEMPT; do
  [ -n "${!v:-}" ] || fail "$v is not set"
done
repo=$GITHUB_REPOSITORY
[[ $repo =~ ^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$ ]] || fail "bad GITHUB_REPOSITORY"
[[ $GITHUB_SHA =~ ^[0-9a-f]{40}$ && $GITHUB_RUN_ID =~ ^[0-9]+$ && $GITHUB_RUN_ATTEMPT =~ ^[0-9]+$ ]] || fail "bad run context"
branch=pages-builds

message="Pages build of $GITHUB_SHA (run $GITHUB_RUN_ID, attempt $GITHUB_RUN_ATTEMPT)"
# A tree with one file, BUILD, holding that line.
tree=$(jq -n --arg c "$message" '{tree: [{path: "BUILD", mode: "100644", type: "blob", content: ($c + "\n")}]}' \
  | "$gh" api -X POST "repos/$repo/git/trees" --input - --jq .sha) || fail "cannot create the build tree"
[[ $tree =~ ^[0-9a-f]{40}$ ]] || fail "unexpected tree '$tree'"

# Append to the branch, so every build commit stays reachable.
if parent=$("$gh" api "repos/$repo/git/ref/heads/$branch" --jq .object.sha 2> /dev/null); then
  [[ $parent =~ ^[0-9a-f]{40}$ ]] || fail "unexpected head of $branch: '$parent'"
  parents=$(jq -n --arg p "$parent" '[$p]')
else
  parent="" parents='[]'
fi
sha=$(jq -n --arg m "$message" --arg t "$tree" --argjson p "$parents" '{message: $m, tree: $t, parents: $p}' \
  | "$gh" api -X POST "repos/$repo/git/commits" --input - --jq .sha) || fail "cannot create the build commit"
[[ $sha =~ ^[0-9a-f]{40}$ ]] || fail "unexpected commit '$sha'"
if [ -n "$parent" ]; then
  jq -n --arg s "$sha" '{sha: $s, force: false}' \
    | "$gh" api -X PATCH "repos/$repo/git/refs/heads/$branch" --input - > /dev/null \
    || fail "cannot advance $branch to $sha (another deployment moved it? deployments are serialized by the registry-pages group)"
else
  jq -n --arg r "refs/heads/$branch" --arg s "$sha" '{ref: $r, sha: $s}' \
    | "$gh" api -X POST "repos/$repo/git/refs" --input - > /dev/null || fail "cannot create $branch"
fi
echo "Pages build commit $sha on $branch ($message)" >&2
[ -z "${GITHUB_OUTPUT:-}" ] || echo "sha=$sha" >> "$GITHUB_OUTPUT"
echo "$sha"
