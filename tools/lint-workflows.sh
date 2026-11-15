#!/usr/bin/env bash
# Lints .github/workflows with actionlint (with shellcheck when installed)
# and zizmor (offline audits). Both are downloaded from their GitHub
# releases and checked against the sha256 pinned here before they run.
. "$(dirname "$0")/lib/common.sh"

ACTIONLINT_VERSION=1.7.12
ZIZMOR_VERSION=1.32.0
case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)
    actionlint_asset=actionlint_${ACTIONLINT_VERSION}_linux_amd64.tar.gz
    actionlint_sha=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8
    zizmor_asset=zizmor-x86_64-unknown-linux-gnu.tar.gz
    zizmor_sha=bb110cc765eca9cd7efb2e284a956be5974c60027b0970f16397d86208a9957b ;;
  Darwin-arm64)
    actionlint_asset=actionlint_${ACTIONLINT_VERSION}_darwin_arm64.tar.gz
    actionlint_sha=aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f
    zizmor_asset=zizmor-aarch64-apple-darwin.tar.gz
    zizmor_sha=6efed18c4539a663fd3c851a2be9533fd8456c4e646d9416206da059154a0d3c ;;
  *) die "no pinned linter binaries for $(uname -s)-$(uname -m)" ;;
esac

dir=$WORK/lint
mkdir -p "$dir"
fetch() { # fetch URL FILE SHA256
  [ -f "$2" ] && [ "$(shasum -a 256 "$2" | cut -d' ' -f1)" = "$3" ] && return
  curl -fsSL --proto '=https' -o "$2" "$1"
  [ "$(shasum -a 256 "$2" | cut -d' ' -f1)" = "$3" ] || { rm -f "$2"; die "$2: sha256 mismatch"; }
}
fetch "https://github.com/rhysd/actionlint/releases/download/v$ACTIONLINT_VERSION/$actionlint_asset" \
  "$dir/$actionlint_asset" "$actionlint_sha"
fetch "https://github.com/zizmorcore/zizmor/releases/download/v$ZIZMOR_VERSION/$zizmor_asset" \
  "$dir/$zizmor_asset" "$zizmor_sha"
tar -xzf "$dir/$actionlint_asset" -C "$dir" actionlint
tar -xzf "$dir/$zizmor_asset" -C "$dir"

cd "$REPO_ROOT"
note "actionlint $ACTIONLINT_VERSION"
"$dir/actionlint" -no-color
note "zizmor $ZIZMOR_VERSION"
"$dir/zizmor" --offline --no-progress --format plain .github/workflows
