#!/usr/bin/env bash
# resolve-tag.sh TAG: the publication a release tag asks for (publish.yml).
#
#   <plugin-id>--v<version>   publish that version: registry/plugins/<id>/
#                              versions/<version>.json must exist at the
#                              tagged commit, and its package URL must be
#                              .../releases/download/<TAG>/<id>-<version>.dpplugin
#   registry--v<anything>     re-sign the index only (revocations, yanks,
#                              listing changes); no new package
#
# The tagged commit must be on main (reviewed). Every other recorded version
# must already have its release asset, so the new index never points at a
# missing download. Prints key=value lines for $GITHUB_OUTPUT.
. "$(dirname "$0")/lib/common.sh"
tag=${1:?usage: resolve-tag.sh TAG}
[[ $tag =~ ^[a-z][a-z0-9.-]{3,56}--v[0-9A-Za-z.+-]{1,32}$ ]] || die "$tag is not a publication tag (<id>--v<version>)"
id=${tag%%--v*}
version=${tag#*--v}
# The plugins DeckPilot bundles are never registry publications, whatever
# publish.yml's tag filter says: official-release.yml releases them.
bundled=$(python3 -I -c '
import importlib.util, sys
spec = importlib.util.spec_from_file_location("c", sys.argv[1])
m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
print("\n".join(sorted(m.BUNDLED_PLUGIN_IDS)))' "$REPO_ROOT/tools/check-official.py") || die "the bundled plugin ids cannot be read"
[ -n "$bundled" ] || die "the bundled plugin ids are empty"
if grep -qxF -- "$id" <<<"$bundled"; then
  die "$id is a plugin DeckPilot bundles: the registry never publishes it (official-release.yml releases it)"
fi
commit=$(git -C "$REPO_ROOT" rev-parse --verify "refs/tags/$tag^{commit}") || die "no tag $tag"
git -C "$REPO_ROOT" merge-base --is-ancestor "$commit" origin/main ||
  die "$tag ($commit) is not on main; only reviewed commits are published"

prefix=https://github.com/knittingcode/deckpilot-plugins/releases/download
if [ "$id" = registry ]; then
  kind=registry
else
  kind=plugin
  record=registry/plugins/$id/versions/$version.json
  git -C "$REPO_ROOT" cat-file -e "$commit:$record" 2>/dev/null || die "$record does not exist at $tag"
  url=$(git -C "$REPO_ROOT" show "$commit:$record" | jq -r .package.url)
  [ "$url" = "$prefix/$tag/$id-$version.dpplugin" ] || die "$record names $url, not the asset of $tag"
fi

# Every other version must be downloadable already.
while read -r path; do
  [[ $path =~ ^registry/plugins/[^/]+/versions/[^/]+\.json$ ]] || continue
  [ "$kind" = plugin ] && [ "$path" = "$record" ] && continue
  other=$(git -C "$REPO_ROOT" show "$commit:$path" | jq -r .package.url)
  [[ $other == "$prefix/"* ]] || die "$path: bad package URL"
  code=$(curl -sS -o /dev/null -I -L --proto '=https' -w '%{http_code}' "$other")
  [ "$code" = 200 ] || die "$path: $other is not released yet (HTTP $code); publish its own tag first"
done < <(git -C "$REPO_ROOT" ls-tree -r --name-only "$commit" -- registry/plugins)

echo "tag=$tag"
echo "kind=$kind"
echo "commit=$commit"
echo "id=$id"
echo "version=$version"
