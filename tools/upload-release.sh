#!/usr/bin/env bash
# upload-release.sh TAG ID VERSION PACKAGES_DIR   (publish.yml, job `release`)
#
# Attaches the package of ID VERSION to the GitHub Release of TAG, which the
# owner created (GITHUB_TOKEN cannot create tags here, and this never tries:
# `--verify-tag` refuses a missing tag). The package must match the sha256
# and size pinned in the reviewed version record. A missing release is
# created as a draft, the asset uploaded, then published, so an immutable
# release is complete before it becomes visible. Never marked latest;
# a pre-release version is a GitHub pre-release. Re-running is a no-op once
# the identical asset is there; another asset under the same name fails.
. "$(dirname "$0")/lib/common.sh"
tag=$1 id=$2 version=$3 packages=$4
record=$REPO_ROOT/registry/plugins/$id/versions/$version.json
name=$id-$version.dpplugin
file=$packages/$tag/$name
[ -f "$record" ] || die "no record $record"
[ -f "$file" ] || die "no package $tag/$name in this run's build"
sha=$(jq -r .package.sha256 "$record")
size=$(jq -r .package.size "$record")
[ "$(shasum -a 256 "$file" | cut -d' ' -f1)" = "$sha" ] || die "$name does not match the pinned sha256"
[ "$(wc -c <"$file" | tr -d ' ')" = "$size" ] || die "$name does not match the pinned size"

repo=${GITHUB_REPOSITORY:-knittingcode/deckpilot-plugins}
pre=()
[[ $version == *-* ]] && pre=(--prerelease)
if ! gh release view "$tag" -R "$repo" >/dev/null 2>&1; then
  note "creating a draft release for the existing tag $tag"
  gh release create "$tag" -R "$repo" --verify-tag --draft --latest=false "${pre[@]+"${pre[@]}"}" \
    --title "$id $version" \
    --notes "Plugin package of \`$id\` $version for the DeckPilot plugin registry.

sha256 \`$sha\`, $size bytes. Install it from DeckPilot (Settings > Extensions > Plugins); the client checks it against the signed registry index."
fi
existing=$(gh release view "$tag" -R "$repo" --json assets -q ".assets[] | select(.name == \"$name\") | .digest")
if [ -z "$existing" ]; then
  gh release upload "$tag" -R "$repo" "$file"
elif [ "$existing" != "sha256:$sha" ]; then
  die "the release of $tag already has a different $name ($existing)"
fi
if [ "$(gh release view "$tag" -R "$repo" --json isDraft -q .isDraft)" = true ]; then
  gh release edit "$tag" -R "$repo" --draft=false --latest=false
fi

url=https://github.com/knittingcode/deckpilot-plugins/releases/download/$tag/$name
check=$(mktemp)
for attempt in 1 2 3 4 5 6; do
  if curl -fsSL --proto '=https' -o "$check" "$url" &&
    [ "$(shasum -a 256 "$check" | cut -d' ' -f1)" = "$sha" ]; then
    note "published: $url"
    summary "Released \`$name\`: sha256 \`$sha\`, $size bytes, $url"
    exit 0
  fi
  sleep $((attempt * 10))
done
die "$url does not serve the pinned package"
