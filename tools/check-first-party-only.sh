#!/usr/bin/env bash
# check-first-party-only.sh DIR
#
# The production registry is not open to other publishers yet
# (docs/production-readiness.md §7, docs/third-party-readiness.md). Until
# it opens, nothing but first-party data may be signed: every publisher
# record (DIR/registry/publishers/*.json) and every plugin
# (DIR/registry/plugins/<id>) must belong to a namespace in
# first_party.namespaces of the newest key set (DIR/keys/<n>.json, signed
# by the offline roots). Run by tools/publish-site.sh before any key is
# written. Merging a submission and creating its tag is therefore not
# enough to publish a third-party plugin: opening the registry takes a
# reviewed change of this gate.
. "$(dirname "$0")/lib/common.sh"
dir=${1:?usage: check-first-party-only.sh DIR}
n=$(find "$dir/keys" -maxdepth 1 -name '[1-9]*.json' -exec basename {} .json \; 2>/dev/null | grep -E '^[1-9][0-9]*$' | sort -n | tail -1)
[ -n "$n" ] || die "$dir/keys holds no key set"
allowed=$(jq -er '.first_party.namespaces | select(type == "array" and length > 0) | .[]' "$dir/keys/$n.json") ||
  die "keys/$n.json names no first-party namespace"
is_allowed() { grep -qxF -- "$1" <<< "$allowed"; }
problems=0
for f in "$dir"/registry/publishers/*.json; do
  [ -f "$f" ] || continue
  ns=$(jq -er '.namespace | strings' "$f") || die "$f has no namespace"
  if ! is_allowed "$ns"; then
    echo "error: publisher $ns ($(basename "$f")) is not first-party; the registry is not open to other publishers" >&2
    problems=$((problems + 1))
  fi
done
for d in "$dir"/registry/plugins/*/; do
  [ -d "$d" ] || continue
  id=$(basename "$d")
  if ! is_allowed "${id%%.*}"; then
    echo "error: plugin $id is not in a first-party namespace; the registry is not open to other publishers" >&2
    problems=$((problems + 1))
  fi
done
[ "$problems" -eq 0 ] || die "refusing to sign: $problems non-first-party entr$([ "$problems" = 1 ] && echo y || echo ies) (docs/third-party-readiness.md)"
note "first-party only: every publisher and plugin is in $(tr '\n' ' ' <<< "$allowed")(keys/$n.json)"
