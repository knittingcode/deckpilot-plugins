#!/usr/bin/env bash
# check-freshness.sh BASE_URL MIN_REMAINING_DAYS
#
# The monitor of refresh-timestamp.yml, run after every scheduled or
# dispatched refresh whether it re-signed or not. Fails (and GitHub then
# notifies the repository's workflow owner) when:
#   - the live timestamp expires in less than MIN_REMAINING_DAYS days: the
#     refreshes are failing or not running (a missed schedule, a disabled
#     workflow, a broken secret), long before clients see it expire;
#   - a published plugin release (`<id>--v<version>`, not a draft, not a
#     registry-only `registry--v…` tag) is not in the live index: its
#     publish.yml run failed or was cancelled (a newer run of the
#     registry-pages concurrency group replaces a pending one).
# Reads the live site anonymously and the releases with GH_TOKEN
# (contents: read). Verifies no signature: every deployment is verified by
# the workflow that made it; this watches what happens between them.
#
# Environment: GH_TOKEN, GITHUB_REPOSITORY; GH (default: gh); NOW (epoch
# seconds, tests only).
. "$(dirname "$0")/lib/common.sh"
base=${1:?usage: check-freshness.sh BASE_URL MIN_REMAINING_DAYS}
days=${2:?usage: check-freshness.sh BASE_URL MIN_REMAINING_DAYS}
gh=${GH:-gh}
[[ $days =~ ^[1-9][0-9]?$ ]] || die "bad MIN_REMAINING_DAYS $days"
repo=${GITHUB_REPOSITORY:-}
[[ $repo =~ ^[A-Za-z0-9-]+/[A-Za-z0-9._-]+$ ]] || die "GITHUB_REPOSITORY (owner/name) is not set"
proto='=https'
[[ $base =~ ^http://127\.0\.0\.1:[0-9]+$ ]] && proto='=http'
tmp=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/freshness.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

fetch() { curl -fsS --proto "$proto" --max-filesize 4194304 -o "$2" "$base/v1/$1" || die "cannot fetch /v1/$1"; }
problems=0
problem() { echo "::error::$*" >&2; summary "- **$***"; problems=$((problems + 1)); }

fetch timestamp.json "$tmp/timestamp.json"
expires=$(jq -er '.expires | sub("\\.[0-9]+Z$"; "Z") | fromdateiso8601' "$tmp/timestamp.json") ||
  die "the live timestamp has no valid expires"
now=${NOW:-$(date -u +%s)}
remaining=$((expires - now))
hours=$((remaining / 3600))
summary "Live timestamp: $(jq -c '{seq, signed_at, expires}' "$tmp/timestamp.json"); ${hours}h of validity left (minimum $((days * 24))h)."
if [ "$remaining" -le 0 ]; then
  problem "the live timestamp has EXPIRED: clients refuse installs and updates. Dispatch Refresh timestamp (docs/publishing.md, Recovery)"
elif [ "$remaining" -lt $((days * 86400)) ]; then
  problem "the live timestamp expires in ${hours}h, less than $days days: the daily refresh is failing or not running (docs/publishing.md, Recovery)"
fi

index=$(jq -er '.index.path | select(test("^index/[1-9][0-9]*\\.json$"))' "$tmp/timestamp.json") ||
  die "the live timestamp names no valid index"
fetch "$index" "$tmp/index.json"
while IFS=$'\t' read -r id detail; do
  [[ $id =~ ^[a-z][a-z0-9.-]{3,56}$ && $detail =~ ^plugins/[a-z][a-z0-9.-]{3,56}/[0-9a-f]{64}\.json$ ]] ||
    die "the live index has a bad entry $id $detail"
  fetch "$detail" "$tmp/detail.json"
  jq -r --arg id "$id" '.versions[].version | "\($id)--v\(.)"' "$tmp/detail.json"
done < <(jq -r '.plugins[] | [.id, .detail.path] | @tsv' "$tmp/index.json") | sort > "$tmp/listed"
"$gh" api --paginate "repos/$repo/releases?per_page=100" \
  --jq '.[] | select(.draft | not) | .tag_name | select(test("^[a-z][a-z0-9.-]{3,56}--v[0-9]") and (startswith("registry--") | not))' | sort > "$tmp/released" ||
  die "cannot list the releases"
missing=$(comm -23 "$tmp/released" "$tmp/listed")
for tag in $missing; do
  problem "the release $tag is not in the live index: its publication did not complete. Re-run publish.yml on the tag (docs/publishing.md, Recovery)"
done
summary "Published plugin releases in the live index: $(comm -12 "$tmp/released" "$tmp/listed" | wc -l | tr -d ' ') of $(wc -l < "$tmp/released" | tr -d ' ')."
[ "$problems" -eq 0 ] || exit 1
note "the live timestamp is valid for ${hours}h more; every published plugin release is listed"
