#!/usr/bin/env bash
# build-plugin.sh --recipe plugins/<id>/build.json --repository OWNER/NAME
#                 --commit SHA [--tag TAG] [--path DIR] --out FILE.dpplugin
#
# Builds one plugin package from its pinned source, the way the registry
# does: a checkout of exactly COMMIT (and, with --tag, the proof that TAG in
# OWNER/NAME resolves to COMMIT), `cargo build --locked` for the recipe's
# package, profile and pinned toolchain, then `deckpilot-registry package`
# (with `--first-party` when the recipe's id is a `deckpilot.*` plugin, so
# the first-party manifest rules apply; DeckPilot B6).
#
# This runs the plugin's build scripts and proc macros: run it only where
# no secret is available (validate.yml, the `build` job of publish.yml, or
# your own machine for your own plugin).
. "$(dirname "$0")/lib/common.sh"
require_cli

recipe= repository= commit= tag= path=. out=
while [ $# -gt 0 ]; do
  case $1 in
    --recipe) recipe=$2 ;;
    --repository) repository=$2 ;;
    --commit) commit=$2 ;;
    --tag) tag=$2 ;;
    --path) path=$2 ;;
    --out) out=$2 ;;
    *) die "unknown option $1" ;;
  esac
  shift 2
done
[ -n "$recipe" ] && [ -n "$repository" ] && [ -n "$commit" ] && [ -n "$out" ] ||
  die "--recipe, --repository, --commit and --out are required"

field() { jq -er --arg k "$1" '.[$k]' "$recipe"; }
toolchain=$(field toolchain)
workspace=$(field workspace)
package=$(field package)
profile=$(field profile)
artifact=$(field artifact)
# The recipe schema already restricts these; repeat it before using them in
# paths and command lines.
[[ $toolchain =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "toolchain $toolchain is not an exact version"
[[ $package =~ ^[A-Za-z0-9_-]{1,64}$ ]] || die "bad package $package"
[[ $profile =~ ^[A-Za-z0-9_-]{1,32}$ ]] || die "bad profile $profile"
[[ $artifact =~ ^[A-Za-z0-9_-]{1,64}\.wasm$ ]] || die "bad artifact $artifact"
safe_rel() { [[ $1 == . || ( $1 =~ ^[A-Za-z0-9._/-]{1,200}$ && $1 != /* && /$1/ != */../* && /$1/ != */./* ) ]]; }
safe_rel "$workspace" || die "bad workspace $workspace"
safe_rel "$path" || die "bad path $path"

build=$WORK/build
src=$build/src
target=$build/target
note "source $repository@$commit${tag:+ (tag $tag)}"
fetch_commit "$src" "$repository" "$commit"
if [ -n "$tag" ]; then
  [[ $tag =~ ^[A-Za-z0-9._/+-]{1,128}$ ]] || die "bad tag $tag"
  # A SHA alone can resolve through the fork network; the tag must exist in
  # the named repository and point at the commit.
  tagged=$(git ls-remote "https://github.com/$repository.git" "refs/tags/$tag^{}" "refs/tags/$tag" |
    awk 'NR==1 || /\^\{\}$/ {sha=$1} END {print sha}')
  [ "$tagged" = "$commit" ] || die "tag $tag of $repository is ${tagged:-missing}, not $commit"
fi

note "toolchain $toolchain ($WASM_TARGET)"
rustup toolchain install "$toolchain" --profile minimal --target "$WASM_TARGET" \
  --no-self-update >/dev/null 2>&1 || rustup toolchain install "$toolchain" \
  --profile minimal --target "$WASM_TARGET" --no-self-update

cargo_home=${CARGO_HOME:-$HOME/.cargo}
rm -rf "$target"
note "cargo build -p $package --profile $profile"
# Fixed path prefixes, so the module does not depend on where it was built.
(cd "$src/$workspace" &&
  CARGO_TARGET_DIR=$target \
  RUSTFLAGS="--remap-path-prefix=$src=/src --remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$target=/target" \
  cargo "+$toolchain" build --locked --target "$WASM_TARGET" --profile "$profile" -p "$package")

module=$target/$WASM_TARGET/$profile/$artifact
[ -f "$module" ] || die "the build produced no $artifact"
mkdir -p "$(dirname "$out")"
first_party=()
case $(field id) in deckpilot.*) first_party=(--first-party) ;; esac
"$CLI" package --plugin-dir "$src/$path" --module "$module" --out "$out" "${first_party[@]+"${first_party[@]}"}"
