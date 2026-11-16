#!/usr/bin/env bash
# build-landing.sh OUT: the static part of the registry tree on Pages:
# /icon.svg and /v1/README.txt. The signed registry, when there is one,
# replaces OUT/v1 (see .github/workflows/_site.yml and publish.yml). The
# human-facing pages are the marketplace website (website/), which
# tools/compose-site.py places around this tree without touching it.
. "$(dirname "$0")/lib/common.sh"
out=${1:?usage: build-landing.sh OUT}
rm -rf "$out"
mkdir -p "$out"
cp -R "$REPO_ROOT/pages/." "$out/"
cp "$REPO_ROOT/.github/assets/icon.svg" "$out/icon.svg"
find "$out" -type f | sort
