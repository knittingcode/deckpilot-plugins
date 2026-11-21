#!/usr/bin/env bash
# The environment that holds registry signing secrets is protected as
# required, or the job that names it must not run. Run by a job that names
# no environment and reads no secret, before the job that does (publish.yml
# `gate`, _site.yml `timestamp-gate`). Read only; prints no secret.
#
#   tools/check-environment.sh registry-release     (release tags *--v* only)
#   tools/check-environment.sh registry-timestamp   (branch main only)
#   tools/check-environment.sh official-release     (tags deckpilot.*--v* only)
#
# Requires, failing closed on any error or unexpected answer:
#   - the environment exists and administrators cannot bypass it;
#   - selected deployment policies (not "protected branches"), exactly:
#       registry-release    tag *--v*
#       registry-timestamp  branch main
#       official-release    tag deckpilot.*--v*
#   - an authorization for every deployment, one of:
#     * a required reviewer;
#     * registry-release and official-release, tag authority: an active tag
#       ruleset on every tag the environment deploys from (~ALL, refs/tags/*,
#       refs/tags/*--v*, or for official-release also
#       refs/tags/deckpilot.*--v*), with no exclusion, restricts creation,
#       update and deletion, and this workflow's token cannot bypass it.
#       Creating the release tag is then the authorization; only the
#       ruleset's bypass actors (the repository administrator) can. For
#       official-release this is the single-maintainer model the owner
#       decided on 2026-11-21 (docs/release-governance.md): there is no
#       second person to review, the two publisher keys stay separate;
#     * registry-timestamp, branch authority: an active ruleset on main, with
#       no exclusion, requires pull requests and status checks and forbids
#       force pushes and deletion, and this token cannot bypass it.
#
# Optional stricter reviewer rules, for any environment, set by the owner as
# repository variables the workflow passes in. Unset by default (one
# maintainer, nobody else to review); meant for when a second maintainer
# joins (docs/release-governance.md, "Adding a second maintainer"):
#   REQUIRE_REVIEWERS=N        at least N reviewers listed on the environment
#                              (GitHub then needs ONE of them to approve; N >= 2
#                              with PREVENT_SELF_REVIEW means someone other than
#                              the person who started the run approves)
#   REQUIRE_PREVENT_SELF_REVIEW=true   the environment's "prevent self-review"
#                              is on: whoever triggers a run cannot approve it
# Once set, a reviewer requirement is never met by a ruleset.
#
# GitHub creates a missing environment, unprotected, the first time a job
# names it, so this check runs before that job and stops the run.
#
# Environment: GITHUB_REPOSITORY (owner/name); GH (default: gh).
set -euo pipefail

gh=${GH:-gh}
fail() { echo "::error::$*" >&2; exit 1; }
repo=${GITHUB_REPOSITORY:-}
name=${1:-}
[[ $repo =~ ^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$ ]] || fail "GITHUB_REPOSITORY (owner/name) is not set"
case "$name" in
  registry-release) want="tag:*--v*" target=tag ;;
  registry-timestamp) want="branch:main" target=branch ;;
  official-release) want="tag:deckpilot.*--v*" target=tag ;;
  *) fail "usage: check-environment.sh registry-release|registry-timestamp|official-release" ;;
esac

environment=$("$gh" api "repos/$repo/environments/$name") \
  || fail "the environment $name does not exist or cannot be read; nothing that names it may run"
[ "$(jq -r '.can_admins_bypass' <<< "$environment")" = false ] \
  || fail "administrators can bypass the protection of $name"
[ "$(jq -r '.deployment_branch_policy.custom_branch_policies // false' <<< "$environment")" = true ] \
  && [ "$(jq -r '.deployment_branch_policy.protected_branches // false' <<< "$environment")" = false ] \
  || fail "$name does not restrict deployments to selected branches and tags"
policies=$("$gh" api "repos/$repo/environments/$name/deployment-branch-policies" \
  --jq '[.branch_policies[] | "\(.type // "branch"):\(.name)"] | sort | join(",")') \
  || fail "the deployment policies of $name cannot be read"
[ "$policies" = "$want" ] || fail "$name deploys from '${policies:-nothing}', expected exactly '$want'"

reviewers=$(jq '[.protection_rules[]? | select(.type == "required_reviewers") | .reviewers[]?] | length' <<< "$environment") \
  || fail "the protection rules of $name cannot be read"
min=${REQUIRE_REVIEWERS:-}
self=${REQUIRE_PREVENT_SELF_REVIEW:-}
[[ -z $min || $min =~ ^[1-6]$ ]] || fail "REQUIRE_REVIEWERS must be 1 to 6, not '$min'"
[[ -z $self || $self == true || $self == false ]] || fail "REQUIRE_PREVENT_SELF_REVIEW must be true or false, not '$self'"
if [ -n "$min" ] && [ "$reviewers" -lt "$min" ]; then
  fail "$name lists $reviewers required reviewer(s); REQUIRE_REVIEWERS asks for at least $min"
fi
if [ "$self" = true ]; then
  [ "$(jq '[.protection_rules[]? | select(.type == "required_reviewers") | .prevent_self_review] | any' <<< "$environment")" = true ] \
    || fail "$name allows self-review; REQUIRE_PREVENT_SELF_REVIEW asks that the person who starts a run cannot approve it"
fi
if [ "$reviewers" -ge 1 ]; then
  echo "$name: $reviewers required reviewer(s); deploys $want only; administrators cannot bypass it"
  exit 0
fi

# No reviewer: the ruleset is the authorization.
if [ "$target" = tag ]; then
  # shellcheck disable=SC2016 # $env is a jq variable (--arg env)
  rule='.target == "tag" and .enforcement == "active"
    and ((.conditions.ref_name.include // []) | any(. == "~ALL" or . == "refs/tags/*" or . == "refs/tags/*--v*"
      or ($env == "official-release" and . == "refs/tags/deckpilot.*--v*")))
    and ((.conditions.ref_name.exclude // []) | length == 0)
    and ([.rules[]?.type] | contains(["creation", "update", "deletion"]))
    and .current_user_can_bypass == "never"'
  what="an active tag ruleset on every release tag restricting creation, update and deletion, without a bypass for this workflow"
else
  rule='.target == "branch" and .enforcement == "active"
    and ((.conditions.ref_name.include // []) | any(. == "~DEFAULT_BRANCH" or . == "refs/heads/main"))
    and ((.conditions.ref_name.exclude // []) | length == 0)
    and ([.rules[]?.type] | contains(["pull_request", "required_status_checks", "non_fast_forward", "deletion"]))
    and .current_user_can_bypass == "never"'
  what="an active ruleset on main requiring pull requests and status checks and forbidding force pushes and deletion, without a bypass for this workflow"
fi
ids=$("$gh" api "repos/$repo/rulesets?targets=$target&per_page=100" --jq '.[].id') \
  || fail "$name requires no reviewer and the $target rulesets cannot be read"
for id in $ids; do
  [[ $id =~ ^[0-9]+$ ]] || fail "unexpected ruleset id '$id'"
  ruleset=$("$gh" api "repos/$repo/rulesets/$id") || fail "ruleset $id cannot be read"
  if jq -e --arg env "$name" "$rule" <<< "$ruleset" > /dev/null; then
    echo "$name: no reviewer; deploys $want only; administrators cannot bypass it; authorized by ruleset \"$(jq -r .name <<< "$ruleset")\" ($what)"
    exit 0
  fi
done
fail "$name requires no reviewer and there is no $what"
