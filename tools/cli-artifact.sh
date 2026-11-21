#!/usr/bin/env bash
# cli-artifact.sh pack DIR | install DIR REGISTRY_SHA256 SCHEMA_SHA256
#
# The pinned tools (tools/build-cli.sh) move from an unprivileged job to a
# job that holds signing keys as a workflow artifact, so NOTHING compiles
# where a key is: no build script of a locked dependency runs in, or
# lingers into, a job whose steps get a secret.
#
# pack DIR      (the unprivileged `cli` job, right after tools/build-cli.sh
#               and nothing else) copies .work/bin/deckpilot-registry and
#               schema-check into DIR and prints `registry=<sha256>` and
#               `schema=<sha256>` for the job's outputs.
# install DIR REGISTRY_SHA256 SCHEMA_SHA256
#               (the signing job) installs the two binaries from the
#               downloaded artifact into .work/bin only if their sha256 are
#               the ones the `cli` job reported.
. "$(dirname "$0")/lib/common.sh"
what=${1:-}
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }

case $what in
  pack)
    dir=${2:?usage: cli-artifact.sh pack DIR}
    require_cli
    mkdir -p "$dir"
    cp "$CLI" "$dir/deckpilot-registry"
    cp "$SCHEMA_CHECK" "$dir/schema-check"
    printf 'registry=%s\nschema=%s\n' "$(sha "$dir/deckpilot-registry")" "$(sha "$dir/schema-check")"
    ;;
  install)
    dir=${2:?} registry=${3:-} schema=${4:-}
    [[ $registry =~ ^[0-9a-f]{64}$ ]] && [[ $schema =~ ^[0-9a-f]{64}$ ]] \
      || die "the cli job reported no sha256 of the tools"
    [ -f "$dir/deckpilot-registry" ] && [ -f "$dir/schema-check" ] || die "$dir does not hold the tools"
    [ "$(sha "$dir/deckpilot-registry")" = "$registry" ] \
      || die "deckpilot-registry is not the binary the cli job built ($(sha "$dir/deckpilot-registry"), expected $registry)"
    [ "$(sha "$dir/schema-check")" = "$schema" ] \
      || die "schema-check is not the binary the cli job built"
    mkdir -p "$(dirname "$CLI")"
    install -m 0755 "$dir/deckpilot-registry" "$CLI"
    install -m 0755 "$dir/schema-check" "$SCHEMA_CHECK"
    "$CLI" help | head -1
    ;;
  *) die "usage: cli-artifact.sh pack DIR | install DIR REGISTRY_SHA256 SCHEMA_SHA256" ;;
esac
