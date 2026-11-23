#!/usr/bin/env bash
# The registry state of the last verified deployment: the seq, signed_at and
# sha256 of the timestamp.json that the live site was proven to serve, and
# the sha256 of that seq's storefront (`storefront_sha256`, null for none:
# storefront/<seq>.json beside the recorded timestamp.json;
# docs/storefront.md), kept as state.json on the branch `registry-state`
# (nothing builds from it and no workflow runs on it). tools/mirror-site.sh
# then requires the live seq's storefront to be exactly the recorded one
# (or absent), and tools/publish-site.sh starts the client's equivocation
# record from it. tools/mirror-site.sh refuses a live site older
# than it (REGISTRY_MIN_STATE), because Pages' CDN can serve the previous
# timestamp, or none, for 10 minutes after a deployment.
#
# It is a freshness floor, not a trust anchor: the registry is trusted by
# its signatures alone (verify-site with the committed roots). A wrong or
# missing state can only make a deployment wait or fail, never make a
# client accept anything.
#
#   tools/registry-state.sh fetch OUT          writes OUT, or nothing when
#                                              no state was recorded yet
#                                              (job permissions: contents: read)
#   tools/registry-state.sh record TIMESTAMP   after the live site was
#                                              verified to serve TIMESTAMP
#                                              (and DIR/storefront/<seq>.json,
#                                              DIR the TIMESTAMP's directory,
#                                              if there is one)
#                                              (job permissions: contents: write)
#
# Environment: GH_TOKEN, GITHUB_REPOSITORY, GITHUB_RUN_ID, GITHUB_RUN_ATTEMPT
# (record); GH (default: gh). Prints no token.
. "$(dirname "$0")/lib/common.sh"

gh=${GH:-gh}
branch=registry-state
current=
repo=${GITHUB_REPOSITORY:-}
[[ $repo =~ ^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$ ]] || die "GITHUB_REPOSITORY (owner/name) is not set"
[ -n "${GH_TOKEN:-}" ] || die "GH_TOKEN is not set"

# branch_head: the commit of the branch, or nothing when there is no branch yet.
branch_head() {
  local out
  if out=$("$gh" api "repos/$repo/git/ref/heads/$branch" --jq .object.sha 2>&1); then
    [[ $out =~ ^[0-9a-f]{40}$ ]] || die "unexpected head of $branch: '$out'"
    echo "$out"
  elif [[ $out == *"HTTP 404"* ]]; then
    return 0
  else
    die "cannot read the branch $branch: $out"
  fi
}

fetch() {
  local out=$1 sha
  rm -f "$out"
  sha=$(branch_head)
  if [ -z "$sha" ]; then
    note "no registry state recorded yet"
    return 0
  fi
  "$gh" api "repos/$repo/contents/state.json?ref=$sha" --jq .content | base64 -d > "$out.tmp" ||
    die "cannot read state.json at $branch $sha"
  timestamp_order "$out.tmp" > /dev/null || die "state.json at $branch $sha has no valid seq or signed_at"
  mv "$out.tmp" "$out"
  note "last verified deployment: $(jq -c '{seq, signed_at}' "$out") ($branch $sha)"
}

record() {
  local ts=$1 parent state tree sha message
  [ -f "$ts" ] || die "$ts does not exist"
  timestamp_order "$ts" > /dev/null || die "$ts has no valid seq or signed_at"
  for v in GITHUB_RUN_ID GITHUB_RUN_ATTEMPT; do
    [[ ${!v:-} =~ ^[0-9]+$ ]] || die "$v is not set"
  done
  parent=$(branch_head)
  current=$(mktemp "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/state.XXXXXX")
  trap 'rm -f "$current"' EXIT
  if [ -n "$parent" ]; then
    "$gh" api "repos/$repo/contents/state.json?ref=$parent" --jq .content | base64 -d > "$current" ||
      die "cannot read state.json at $branch $parent"
    if older_than "$ts" "$current"; then
      die "refusing to record $(timestamp_order "$ts"): older than the recorded $(timestamp_order "$current")"
    fi
  fi
  local seq storefront=null
  seq=$(jq -er .seq "$ts")
  if [ -f "$(dirname "$ts")/storefront/$seq.json" ]; then
    storefront=\"$(shasum -a 256 "$(dirname "$ts")/storefront/$seq.json" | cut -d' ' -f1)\"
  fi
  # The same seq keeps its storefront: a refresh never changes or drops it.
  if [ -n "$parent" ] && jq -e --argjson s "$seq" '.seq == $s and has("storefront_sha256")' "$current" > /dev/null &&
    ! jq -e --argjson f "$storefront" '.storefront_sha256 == $f' "$current" > /dev/null; then
    die "refusing to record seq $seq with storefront $storefront: the recorded state of seq $seq has $(jq -c .storefront_sha256 "$current")"
  fi
  state=$(jq -c --arg sha "$(shasum -a 256 "$ts" | cut -d' ' -f1)" --arg run "$GITHUB_RUN_ID" --arg attempt "$GITHUB_RUN_ATTEMPT" \
    --argjson storefront "$storefront" \
    '{seq, signed_at, expires, timestamp_sha256: $sha, storefront_sha256: $storefront, run: ($run | tonumber), attempt: ($attempt | tonumber)}' "$ts")
  message="Verified deployment: seq $(jq -r .seq <<< "$state"), signed $(jq -r .signed_at <<< "$state") (run $GITHUB_RUN_ID, attempt $GITHUB_RUN_ATTEMPT)"
  tree=$(jq -n --arg c "$(jq . <<< "$state")" '{tree: [{path: "state.json", mode: "100644", type: "blob", content: ($c + "\n")}]}' |
    "$gh" api -X POST "repos/$repo/git/trees" --input - --jq .sha) || die "cannot create the state tree"
  [[ $tree =~ ^[0-9a-f]{40}$ ]] || die "unexpected tree '$tree'"
  sha=$(jq -n --arg m "$message" --arg t "$tree" --arg p "$parent" '{message: $m, tree: $t, parents: (if $p == "" then [] else [$p] end)}' |
    "$gh" api -X POST "repos/$repo/git/commits" --input - --jq .sha) || die "cannot create the state commit"
  [[ $sha =~ ^[0-9a-f]{40}$ ]] || die "unexpected commit '$sha'"
  if [ -n "$parent" ]; then
    jq -n --arg s "$sha" '{sha: $s, force: false}' |
      "$gh" api -X PATCH "repos/$repo/git/refs/heads/$branch" --input - > /dev/null ||
      die "cannot advance $branch to $sha (deployments are serialized by the registry-pages group)"
  else
    jq -n --arg r "refs/heads/$branch" --arg s "$sha" '{ref: $r, sha: $s}' |
      "$gh" api -X POST "repos/$repo/git/refs" --input - > /dev/null || die "cannot create $branch"
  fi
  note "$message ($branch $sha)"
  summary "Registry state recorded: $state"
}

case "${1:-}" in
  fetch) fetch "${2:?usage: registry-state.sh fetch OUT}" ;;
  record) record "${2:?usage: registry-state.sh record TIMESTAMP}" ;;
  *) die "usage: registry-state.sh fetch OUT | record TIMESTAMP" ;;
esac
