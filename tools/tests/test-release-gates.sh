#!/usr/bin/env bash
# Tests of tools/check-environment.sh and tools/check-required-checks.sh
# against API answers recorded from this repository (fixtures/gates; the
# environments, rulesets, branch rules, check runs and pull request of the
# merge of #38), each case changing one thing. No network, no token.
#
#   tools/tests/test-release-gates.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
fixtures=$root/tools/tests/fixtures/gates
work=$(mktemp -d "${TMPDIR:-/tmp}/release-gates.XXXXXX")
trap 'rm -rf "$work"' EXIT
export GITHUB_REPOSITORY=knittingcode/deckpilot-plugins CHECKS_WAIT_SECONDS=0 CHECKS_POLL_SECONDS=0
MERGE=c7e54be30d014a199ee26f536f29d0d947d93456
HEAD=bb3d3052948637b01a767115e5eaab4d783b50db

# A gh stand-in: answers `gh api <endpoint> [--paginate] [--jq <q>]` from
# $work/api; a missing answer is an HTTP 404; STUB_FAIL (regex) fails calls.
mkdir -p "$work/bin"
cat > "$work/bin/gh" << 'STUB'
#!/usr/bin/env bash
set -euo pipefail
[ "$1" = api ] || { echo "stub gh: $*" >&2; exit 2; }
shift; endpoint="" q="."
while [ $# -gt 0 ]; do
  case "$1" in --jq) q=$2; shift 2 ;; --paginate) shift ;; *) endpoint=$1; shift ;; esac
done
if [ -n "${STUB_FAIL:-}" ] && [[ $endpoint =~ $STUB_FAIL ]]; then echo "HTTP 502: Bad Gateway" >&2; exit 1; fi
p=${endpoint#repos/knittingcode/deckpilot-plugins/}
case "$p" in
  environments/*/deployment-branch-policies) e=${p#environments/}; f="policies-${e%%/*}.json" ;;
  environments/*) f="env-${p#environments/}.json" ;;
  "rulesets?targets=tag"*) f=rulesets-tag.json ;;
  "rulesets?targets=branch"*) f=rulesets-branch.json ;;
  rulesets/*) f="ruleset-${p#rulesets/}.json" ;;
  rules/branches/main) f=rules-main.json ;;
  commits/*/check-runs*) s=${p#commits/}; f="check-runs-${s%%/*}.json" ;;
  commits/*/pulls) s=${p#commits/}; f="pulls-${s%%/*}.json" ;;
  *) f=unknown ;;
esac
[ -f "$STUB_API/$f" ] || { echo "HTTP 404: Not Found ($endpoint)" >&2; exit 1; }
jq -r "$q" "$STUB_API/$f"
STUB
chmod +x "$work/bin/gh"
export GH="$work/bin/gh" STUB_API="$work/api"

passed=0 failed=0
ok() { echo "ok   $*"; passed=$((passed + 1)); }
bad() { echo "FAIL $*"; failed=$((failed + 1)); }
fresh() { rm -rf "$work/api"; cp -R "$fixtures" "$work/api"; unset STUB_FAIL; }
edit() { jq "$2" "$work/api/$1" > "$work/api/$1.t" && mv "$work/api/$1.t" "$work/api/$1"; }
no_reviewer() { edit env-registry-release.json '.protection_rules |= map(select(.type != "required_reviewers"))'; }
# expect <pass|fail> <name> <message regex> -- <command...>
expect() {
  local want=$1 name=$2 msg=$3; shift 4
  local rc=0; "$@" > "$work/out" 2>&1 || rc=$?
  if { [ "$want" = pass ] && [ $rc = 0 ]; } || { [ "$want" = fail ] && [ $rc != 0 ]; }; then
    if grep -qE -- "$msg" "$work/out"; then ok "$name"; else bad "$name: unexpected message: $(tr '\n' ' ' < "$work/out")"; fi
  else bad "$name: exit $rc: $(tr '\n' ' ' < "$work/out")"; fi
}
env_check() { "$root/tools/check-environment.sh" "$@"; }
checks() { "$root/tools/check-required-checks.sh" "$@"; }

echo "== check-environment.sh registry-release"
fresh; expect pass "a required reviewer authorizes" "1 required reviewer" -- env_check registry-release
fresh; no_reviewer; expect pass "no reviewer: the tag ruleset authorizes" 'authorized by ruleset "Protect tags"' -- env_check registry-release
fresh; edit env-registry-release.json '.can_admins_bypass = true'; expect fail "administrators can bypass" "administrators can bypass" -- env_check registry-release
fresh; edit env-registry-release.json '.deployment_branch_policy.protected_branches = true | .deployment_branch_policy.custom_branch_policies = false'; expect fail "protected branches instead of selected tags" "selected branches and tags" -- env_check registry-release
fresh; edit policies-registry-release.json '.branch_policies += [{name: "main", type: "branch"}]'; expect fail "a branch may deploy too" "expected exactly 'tag:\*--v\*'" -- env_check registry-release
fresh; edit policies-registry-release.json '.branch_policies[0].name = "*"'; expect fail "any tag may deploy" "deploys from 'tag:\*'" -- env_check registry-release
fresh; rm "$work/api/env-registry-release.json"; expect fail "the environment is missing" "does not exist" -- env_check registry-release
fresh; STUB_FAIL='environments/registry-release$'; export STUB_FAIL; expect fail "the environment cannot be read (API error)" "does not exist or cannot be read" -- env_check registry-release
fresh; STUB_FAIL='deployment-branch-policies'; export STUB_FAIL; expect fail "the policies cannot be read (API error)" "policies of registry-release cannot be read" -- env_check registry-release
fresh; no_reviewer; echo '[]' > "$work/api/rulesets-tag.json"; expect fail "no reviewer and no tag ruleset" "requires no reviewer and there is no" -- env_check registry-release
fresh; no_reviewer; STUB_FAIL='rulesets\?targets=tag'; export STUB_FAIL; expect fail "no reviewer, the rulesets cannot be read" "rulesets cannot be read" -- env_check registry-release
fresh; no_reviewer; STUB_FAIL='rulesets/27309841'; export STUB_FAIL; expect fail "no reviewer, a ruleset cannot be read" "ruleset 27309841 cannot be read" -- env_check registry-release
fresh; no_reviewer; edit ruleset-27309841.json '.enforcement = "evaluate"'; expect fail "the tag ruleset only evaluates" "there is no" -- env_check registry-release
fresh; no_reviewer; edit ruleset-27309841.json '.conditions.ref_name.exclude = ["refs/tags/deckpilot.*"]'; expect fail "the tag ruleset excludes tags" "there is no" -- env_check registry-release
fresh; no_reviewer; edit ruleset-27309841.json '.conditions.ref_name.include = ["refs/tags/v*"]'; expect fail "the tag ruleset covers other tags only" "there is no" -- env_check registry-release
fresh; no_reviewer; edit ruleset-27309841.json '.rules |= map(select(.type != "creation"))'; expect fail "the tag ruleset allows creating tags" "there is no" -- env_check registry-release
fresh; no_reviewer; edit ruleset-27309841.json '.current_user_can_bypass = "always"'; expect fail "the workflow token can bypass the tag ruleset" "there is no" -- env_check registry-release
fresh; no_reviewer; edit ruleset-27309841.json '.conditions.ref_name.include = ["refs/tags/*--v*"]'; expect pass "a tag ruleset on *--v* only also authorizes" "authorized by ruleset" -- env_check registry-release

echo "== check-environment.sh registry-timestamp"
fresh; expect pass "main's ruleset authorizes" 'authorized by ruleset "Protect main"' -- env_check registry-timestamp
fresh; edit policies-registry-timestamp.json '.branch_policies[0] = {name: "*--v*", type: "tag"}'; expect fail "deploys from tags" "expected exactly 'branch:main'" -- env_check registry-timestamp
fresh; edit policies-registry-timestamp.json '.branch_policies[0].name = "*"'; expect fail "deploys from any branch" "deploys from 'branch:\*'" -- env_check registry-timestamp
fresh; edit ruleset-27309840.json '.rules |= map(select(.type != "pull_request"))'; expect fail "main accepts direct pushes" "there is no" -- env_check registry-timestamp
fresh; edit ruleset-27309840.json '.rules |= map(select(.type != "required_status_checks"))'; expect fail "main requires no status checks" "there is no" -- env_check registry-timestamp
fresh; edit env-registry-timestamp.json '.can_admins_bypass = true'; expect fail "administrators can bypass registry-timestamp" "administrators can bypass" -- env_check registry-timestamp
fresh; expect fail "an unknown environment" "usage" -- env_check github-pages
fresh; expect fail "no repository" "GITHUB_REPOSITORY" -- env GITHUB_REPOSITORY= "$root/tools/check-environment.sh" registry-release

echo "== check-environment.sh official-release"
# The registry-release answers, deploying from the official release tags.
official() {
  cp "$work/api/env-registry-release.json" "$work/api/env-official-release.json"
  jq '.branch_policies = [{name: "deckpilot.*--v*", type: "tag"}]' "$work/api/policies-registry-release.json" \
    > "$work/api/policies-official-release.json"
}
fresh; official; expect pass "a required reviewer authorizes official-release" "1 required reviewer" -- env_check official-release
fresh; official; edit env-official-release.json '.protection_rules |= map(select(.type != "required_reviewers"))'; expect fail "official-release: no ruleset stands in for a reviewer" "needs a required reviewer" -- env_check official-release
fresh; official; edit policies-official-release.json '.branch_policies[0].name = "*--v*"'; expect fail "official-release deploys from every release tag" "expected exactly 'tag:deckpilot" -- env_check official-release
fresh; official; edit policies-official-release.json '.branch_policies += [{name: "main", type: "branch"}]'; expect fail "official-release deploys from main too" "expected exactly" -- env_check official-release
fresh; official; edit env-official-release.json '.can_admins_bypass = true'; expect fail "administrators can bypass official-release" "administrators can bypass" -- env_check official-release
fresh; expect fail "official-release does not exist" "does not exist" -- env_check official-release
fresh; official; expect fail "REQUIRE_REVIEWERS=2 with one reviewer" "asks for at least 2" -- env REQUIRE_REVIEWERS=2 "$root/tools/check-environment.sh" official-release
fresh; official; edit env-official-release.json '(.protection_rules[] | select(.type == "required_reviewers") | .reviewers) += [{type: "User", reviewer: {login: "second", id: 2}}]'; expect pass "REQUIRE_REVIEWERS=2 with two reviewers" "2 required reviewer" -- env REQUIRE_REVIEWERS=2 "$root/tools/check-environment.sh" official-release
fresh; official; expect fail "REQUIRE_PREVENT_SELF_REVIEW and self-review allowed" "allows self-review" -- env REQUIRE_PREVENT_SELF_REVIEW=true "$root/tools/check-environment.sh" official-release
fresh; official; edit env-official-release.json '(.protection_rules[] | select(.type == "required_reviewers")).prevent_self_review = true'; expect pass "REQUIRE_PREVENT_SELF_REVIEW and self-review prevented" "required reviewer" -- env REQUIRE_PREVENT_SELF_REVIEW=true "$root/tools/check-environment.sh" official-release
# Signing from an unprotected branch or another tag: the environment's
# policy refuses it before a signing job (sign-a, sign-b) can start.
fresh; official; edit policies-official-release.json '.branch_policies = [{name: "main", type: "branch"}]'; expect fail "official-release deploys from a branch (signing from main)" "deploys from 'branch:main', expected exactly 'tag:deckpilot" -- env_check official-release
fresh; official; edit policies-official-release.json '.branch_policies = [{name: "*", type: "branch"}]'; expect fail "official-release deploys from any branch (an unprotected branch)" "deploys from 'branch:\*'" -- env_check official-release
fresh; official; edit policies-official-release.json '.branch_policies = [{name: "deckpilot.*", type: "tag"}]'; expect fail "official-release deploys from a non-release tag (deckpilot.*)" "deploys from 'tag:deckpilot.\*'," -- env_check official-release
fresh; official; edit policies-official-release.json '.branch_policies = [{name: "deckpilot.*--v*", type: "branch"}]'; expect fail "official-release deploys from branches named like release tags" "deploys from 'branch:deckpilot" -- env_check official-release
fresh; official; edit policies-official-release.json '.branch_policies = []'; expect fail "official-release has no deployment policy" "deploys from 'nothing'" -- env_check official-release
fresh; official; edit env-official-release.json '.deployment_branch_policy = null'; expect fail "official-release deploys from every branch and tag" "selected branches and tags" -- env_check official-release
fresh; official; edit env-official-release.json '.deployment_branch_policy.protected_branches = true | .deployment_branch_policy.custom_branch_policies = false'; expect fail "official-release deploys from protected branches" "selected branches and tags" -- env_check official-release
# The owner's settings today: one reviewer, self-review prevented, the
# variables asking for two reviewers: the gate fails closed.
fresh; official; edit env-official-release.json '(.protection_rules[] | select(.type == "required_reviewers")).prevent_self_review = true'; expect fail "the owner's settings with one reviewer: fails closed" "asks for at least 2" -- env REQUIRE_REVIEWERS=2 REQUIRE_PREVENT_SELF_REVIEW=true "$root/tools/check-environment.sh" official-release
fresh; official; edit env-official-release.json '(.protection_rules[] | select(.type == "required_reviewers")) |= (.prevent_self_review = true | .reviewers += [{type: "User", reviewer: {login: "second", id: 2}}])'; expect pass "the owner's settings with two reviewers" "2 required reviewer" -- env REQUIRE_REVIEWERS=2 REQUIRE_PREVENT_SELF_REVIEW=true "$root/tools/check-environment.sh" official-release
fresh; official; expect fail "a malformed REQUIRE_REVIEWERS" "must be 1 to 6" -- env REQUIRE_REVIEWERS=two "$root/tools/check-environment.sh" official-release
fresh; no_reviewer; expect fail "REQUIRE_REVIEWERS is never met by a ruleset" "asks for at least 1" -- env REQUIRE_REVIEWERS=1 "$root/tools/check-environment.sh" registry-release

echo "== check-required-checks.sh"
runs_merge="check-runs-$MERGE.json" runs_head="check-runs-$HEAD.json"
fresh; expect pass "every required check passed (Submission rules on the head of #38)" "head of #38 .* Submission rules" -- checks "$MERGE"
fresh; edit "$runs_merge" '(.check_runs[] | select(.name == "Validate registry")).conclusion = "failure"'; expect fail "a required check failed on the commit" '"Validate registry" is failure' -- checks "$MERGE"
fresh; edit "$runs_merge" '(.check_runs[] | select(.name == "Build examples")).conclusion = "cancelled"'; expect fail "a required check was cancelled" '"Build examples" is cancelled' -- checks "$MERGE"
fresh; edit "$runs_merge" '(.check_runs[] | select(.name == "Build examples")) |= (.status = "in_progress" | .conclusion = null)'; expect fail "a required check still running (no wait)" "still running" -- checks "$MERGE"
fresh; edit "$runs_merge" '.check_runs |= map(select(.name != "Repository hygiene"))'; expect pass "a check absent on the commit but green on the PR head" "head of #38 .* Repository hygiene" -- checks "$MERGE"
fresh; edit "$runs_head" '(.check_runs[] | select(.name == "Submission rules")).conclusion = "failure"'; expect fail "Submission rules failed on the PR head" 'Submission rules" did not run on .* is .*failure' -- checks "$MERGE"
fresh; edit "$runs_head" '.check_runs |= map(select(.name != "Submission rules"))'; expect fail "Submission rules never ran on the PR head" "is 'absent'" -- checks "$MERGE"
fresh; echo '[]' > "$work/api/pulls-$MERGE.json"; expect fail "no merged pull request for the commit" "not the merge commit of exactly one merged pull request" -- checks "$MERGE"
fresh; edit "pulls-$MERGE.json" '.[0].merged_at = null'; expect fail "the pull request is not merged" "not the merge commit" -- checks "$MERGE"
fresh; edit "pulls-$MERGE.json" '.[0].base.ref = "dev"'; expect fail "the pull request merged elsewhere" "not the merge commit" -- checks "$MERGE"
fresh; edit "pulls-$MERGE.json" ". + [.[0]]"; expect fail "two pull requests claim the commit" "not the merge commit" -- checks "$MERGE"
fresh; edit "$runs_merge" '.check_runs += [(.check_runs[] | select(.name == "Validate registry") | .id += 1 | .conclusion = "failure")]'; expect fail "the newest run of a check failed (re-run)" '"Validate registry" is failure' -- checks "$MERGE"
fresh; edit "$runs_merge" '.check_runs += [(.check_runs[] | select(.name == "Validate registry") | .id -= 1000000 | .conclusion = "failure")]'; expect pass "an older failed run, then a newer success" "every required check" -- checks "$MERGE"
fresh; edit rules-main.json 'map(select(.type != "required_status_checks"))'; expect fail "main's rules require no check" "require no status check" -- checks "$MERGE"
fresh; STUB_FAIL='rules/branches/main'; export STUB_FAIL; expect fail "the branch rules cannot be read" "branch rules of main cannot be read" -- checks "$MERGE"
fresh; STUB_FAIL="commits/$MERGE/check-runs"; export STUB_FAIL; expect fail "the check runs cannot be read" "check runs of $MERGE cannot be read" -- checks "$MERGE"
fresh; STUB_FAIL="commits/$MERGE/pulls"; export STUB_FAIL; expect fail "the pull requests cannot be read" "pull requests of $MERGE cannot be read" -- checks "$MERGE"
fresh; expect fail "an unknown commit" "check runs of 0{40} cannot be read" -- checks 0000000000000000000000000000000000000000
fresh; expect fail "not a commit id" "usage" -- checks main

echo
echo "$passed passed, $failed failed"
[ "$failed" = 0 ]
