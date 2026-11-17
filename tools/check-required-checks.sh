#!/usr/bin/env bash
# Every status check that main's rules require passed for the commit about
# to be published. Being on main is not enough: an administrator can merge a
# pull request past failing checks, so publication checks the results
# itself. Run by publish.yml `gate`, before `sign`. Read only.
#
#   tools/check-required-checks.sh <commit on main>
#
# For each required check (the branch rules of main; unreadable or empty
# rules stop publication), the newest run of that name:
#   - on the commit itself: success passes; any other completed result fails;
#     queued or in progress is waited for (CHECKS_WAIT_SECONDS, default 1800);
#   - absent or skipped on the commit (checks that run on pull requests only,
#     like "Submission rules"): the newest run on the head of the merged pull
#     request whose merge commit this is must be a success.
#
# Environment: GITHUB_REPOSITORY (owner/name); GH (default: gh);
# CHECKS_WAIT_SECONDS; CHECKS_POLL_SECONDS (default 30).
set -euo pipefail

gh=${GH:-gh}
fail() { echo "::error::$*" >&2; exit 1; }
repo=${GITHUB_REPOSITORY:-}
sha=${1:-}
[[ $repo =~ ^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$ ]] || fail "GITHUB_REPOSITORY (owner/name) is not set"
[[ $sha =~ ^[0-9a-f]{40}$ ]] || fail "usage: check-required-checks.sh <40-hex commit>"
wait_s=${CHECKS_WAIT_SECONDS:-1800}
poll_s=${CHECKS_POLL_SECONDS:-30}

required=$("$gh" api "repos/$repo/rules/branches/main" \
  --jq '.[] | select(.type == "required_status_checks") | .parameters.required_status_checks[].context') \
  || fail "the branch rules of main cannot be read"
required=$(printf '%s\n' "$required" | sed '/^$/d' | LC_ALL=C sort -u)
[ -n "$required" ] || fail "main's rules require no status check; refusing to publish without CI"

# runs <commit>: "<name>\t<status>\t<conclusion>" for the newest run of each name
runs() {
  "$gh" api --paginate "repos/$repo/commits/$1/check-runs?per_page=100" \
    --jq '.check_runs[] | [.id, .name, .status, (.conclusion // "")] | @tsv' \
    | LC_ALL=C sort -t$'\t' -k2,2 -k1,1n \
    | awk -F'\t' '{last[$2] = $2 "\t" $3 "\t" $4} END {for (n in last) print last[n]}'
}
result() { awk -F'\t' -v n="$2" '$1 == n {print $2 "\t" $3}' <<< "$1"; }

deadline=$(( $(date +%s) + wait_s ))
while :; do
  on_commit=$(runs "$sha") || fail "the check runs of $sha cannot be read"
  pending=""
  while IFS= read -r check; do
    r=$(result "$on_commit" "$check")
    case "${r%%$'\t'*}" in queued | in_progress | waiting | requested | pending) pending="$pending $check;" ;; esac
  done <<< "$required"
  [ -z "$pending" ] && break
  [ "$(date +%s)" -lt "$deadline" ] || fail "required checks still running on $sha after ${wait_s}s:$pending"
  echo "waiting for:$pending"
  sleep "$poll_s"
done

pr_head=""
pr_number=""
head_runs=""
while IFS= read -r check; do
  r=$(result "$on_commit" "$check")
  conclusion=${r#*$'\t'}
  if [ -n "$r" ] && [ "$conclusion" != skipped ]; then
    [ "$conclusion" = success ] || fail "required check \"$check\" is $conclusion on $sha"
    echo "required check passed on $sha: $check"
    continue
  fi
  if [ -z "$pr_head" ]; then
    pr=$("$gh" api "repos/$repo/commits/$sha/pulls" \
      --jq "[.[] | select(.merged_at != null and .base.ref == \"main\" and .merge_commit_sha == \"$sha\")] | if length == 1 then .[0] | \"\(.number) \(.head.sha)\" else empty end") \
      || fail "the pull requests of $sha cannot be read"
    pr_number=${pr%% *}; pr_head=${pr#* }
    [[ $pr_head =~ ^[0-9a-f]{40}$ ]] \
      || fail "required check \"$check\" did not run on $sha, and $sha is not the merge commit of exactly one merged pull request"
    head_runs=$(runs "$pr_head") || fail "the check runs of $pr_head cannot be read"
  fi
  h=$(result "$head_runs" "$check")
  [ "${h#*$'\t'}" = success ] && [ "${h%%$'\t'*}" = completed ] \
    || fail "required check \"$check\" did not run on $sha and is '${h:-absent}' on the head of #$pr_number ($pr_head)"
  echo "required check passed on the head of #$pr_number ($pr_head): $check"
done <<< "$required"
echo "every required check of main passed for $sha"
