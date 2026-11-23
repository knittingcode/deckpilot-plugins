# Shared by the tests that run the real signing pipeline offline
# (tools/tests/test-storefront-site.sh, test-registry-freshness.sh
# --with-cli). Source it after setting `root` (this repository) and `work`
# (an empty scratch directory). Needs the pinned tools in $root/.work/bin
# (tools/build-cli.sh). Nothing here ever sees a real key: every key is a
# throwaway made in $work.
#
#   site_repo DIR [TOOLS_REF]   a scratch copy of this repository in DIR (its
#                               tools/ from the working tree, or from git
#                               TOOLS_REF), committed, whose keys/ are a new
#                               throwaway root-signed key set authorizing
#                               deckpilot, and whose registry/ holds one
#                               plugin, deckpilot.example-default 0.1.0,
#                               with its package in DIR.packages
#   site_storefront DIR         adds a listing (icon, screenshot) and an
#                               editorial document (a banner, a collection)
#   site_publish DIR MODE OUT   tools/publish-site.sh MODE of DIR against the
#                               loopback site, keys from DIR.keys
#   site_serve OUT              serves OUT/site/v1 as the live /v1/
#   site_files DIR              "<path> <sha256>" of every file of DIR, sorted
#   site_clone SRC DST REF      DST: SRC with the same keys, registry and
#                               packages, but tools/ of git REF (a baseline)
#   site_verify_published DIR EXPECTED_TIMESTAMP LOG [ENV=VALUE...]
#                               tools/verify-published.sh of DIR against the
#                               loopback site; release downloads are served
#                               from DIR.packages (a curl stand-in on PATH)
#   site_normalized DIR         every file of DIR/site/v1 as "<path> <digest>",
#                               where a signature counts by its presence and
#                               the signing times, the index's source commit
#                               and the timestamp's pin of the index (which
#                               follow from them) are left out
#
# The loopback "Pages" is started by site_start and stopped by site_stop.

site_cli=$root/.work/bin/deckpilot-registry
site_schema=$root/.work/bin/schema-check
[ -x "$site_cli" ] && [ -x "$site_schema" ] || {
  echo "FAIL no pinned deckpilot-registry and schema-check in $root/.work/bin: run tools/build-cli.sh first"
  exit 1
}
site_www=$work/www
site_pid=""

site_start() {
  mkdir -p "$site_www/v1"
  local port
  port=$(python3 -I -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')
  (cd "$site_www" && exec python3 -I -m http.server --bind 127.0.0.1 "$port" > /dev/null 2>&1) &
  site_pid=$!
  site_base=http://127.0.0.1:$port
  for _ in $(seq 1 50); do curl -s -o /dev/null "$site_base/" && break; sleep 0.1; done
}

site_stop() {
  if [ -n "$site_pid" ]; then kill "$site_pid" 2>/dev/null || true; wait "$site_pid" 2>/dev/null || true; fi
  site_pid=""
}

site_serve() {
  rm -rf "$site_www/v1"
  cp -R "$1/site/v1" "$site_www/v1"
}

site_files() {
  (cd "$1" && find . -type f | LC_ALL=C sort | while read -r f; do printf '%s %s\n' "${f#./}" "$(shasum -a 256 "$f" | cut -d' ' -f1)"; done)
}

# A minimal plugin ABI 1 module: memory, dp_abi_version, dp_alloc, dp_invoke.
site_module() {
  python3 -I - "$1" << 'PY'
import sys
def vec(items): return bytes([len(items)]) + b"".join(items)
def sec(i, body): return bytes([i, len(body)]) + body
I32, I64 = 0x7F, 0x7E
types = vec([bytes([0x60, 0, 1, I32]), bytes([0x60, 1, I32, 1, I32]), bytes([0x60, 2, I32, I32, 1, I64])])
def exp(name, kind, idx): n = name.encode(); return bytes([len(n)]) + n + bytes([kind, idx])
exports = vec([exp("memory", 2, 0), exp("dp_abi_version", 0, 0), exp("dp_alloc", 0, 1), exp("dp_invoke", 0, 2)])
def body(code): b = b"\0" + code + b"\x0b"; return bytes([len(b)]) + b
code = vec([body(b"\x41\x01"), body(b"\x41\x80\x08"), body(b"\x42\x00")])
open(sys.argv[1], "wb").write(b"\0asm\1\0\0\0" + sec(1, types) + sec(3, vec([b"\0", b"\1", b"\2"]))
                             + sec(5, vec([bytes([0, 1])])) + sec(7, exports) + sec(10, code))
PY
}

site_repo() {
  local dir=$1 ref=${2:-} keys=$1.keys stage=$1.stage
  rm -rf "$dir" "$keys" "$stage" "$dir.packages" "$dir.work"
  mkdir -p "$dir" "$keys" "$stage"
  if [ -n "$ref" ]; then
    git -C "$root" archive "$ref" tools | tar -x -C "$dir"
  else
    tar -C "$root" --exclude=.work -cf - tools | tar -x -C "$dir"
  fi
  tar -C "$root" --exclude=target -cf - official schemas | tar -x -C "$dir"
  # The pinned tools, shared (publish-site.sh needs them in $WORK/bin).
  mkdir -p "$dir.work/bin"
  ln -s "$site_cli" "$dir.work/bin/deckpilot-registry"
  ln -s "$site_schema" "$dir.work/bin/schema-check"

  # Throwaway keys: a root-signed key set that authorizes deckpilot.
  local role
  for role in root index timestamp; do
    "$site_cli" keys generate --role "$role" --out "$keys/$role.key" --no-password > /dev/null
  done
  "$site_cli" keyset init --dir "$stage" --root "$keys/root.key.pub" --index "$keys/index.key.pub" \
    --timestamp "$keys/timestamp.key.pub" --first-party > /dev/null
  "$site_cli" keyset sign --dir "$stage" --key "$keys/root.key" > /dev/null
  mkdir -p "$dir/keys"
  cp "$stage/roots.json" "$dir/keys/roots.json"
  cp "$stage/site/v1/keys/1.json" "$stage/site/v1/keys/1.json.minisig" "$dir/keys/"

  # One first-party plugin, recorded as add-version records it.
  local id=deckpilot.example-default version=0.1.0 plugin=$stage/plugin
  mkdir -p "$plugin" "$stage/registry/publishers" "$stage/registry/plugins/$id"
  cp "$root/official/test-fixtures/example-default/manifest.json" "$root/official/test-fixtures/example-default/LICENSE" "$plugin/"
  [ ! -d "$root/official/test-fixtures/example-default/assets" ] ||
    cp -R "$root/official/test-fixtures/example-default/assets" "$plugin/"
  site_module "$stage/plugin.wasm"
  "$site_cli" package --plugin-dir "$plugin" --module "$stage/plugin.wasm" --out "$stage/$id-$version.dpplugin" --first-party > /dev/null
  cp "$root/registry/publishers/knittingcode.json" "$stage/registry/publishers/"
  jq -n --arg id "$id" '{id: $id, name: "Example", summary: "A throwaway test plugin.", description: "A throwaway test plugin.",
    license: "Apache-2.0", source_repository: "knittingcode/deckpilot-plugins", categories: ["other"]}' \
    > "$stage/registry/plugins/$id/plugin.json"
  "$site_cli" add-version --dir "$stage" --package "$stage/$id-$version.dpplugin" \
    --plugin-detail "$stage/registry/plugins/$id/plugin.json" --source-repository knittingcode/deckpilot-plugins \
    --tag "$id--v$version" --commit "$(printf '%040d' 1)" --builder 1.0.0 --first-party \
    --now 2026-11-22T00:00:00Z > /dev/null
  cp -R "$stage/registry" "$dir/registry"
  mkdir -p "$dir.packages"
  cp -R "$stage/releases/." "$dir.packages/"

  git -C "$dir" init -q
  git -C "$dir" add -A
  git -C "$dir" -c user.name=t -c user.email=t@t commit -q -m "scratch registry"
}

site_storefront() {
  local dir=$1 id=deckpilot.example-default
  local d=$dir/registry/plugins/$id
  mkdir -p "$d/media" "$dir/editorial/media"
  python3 -I - "$root/tools/tests" "$d/media" "$dir/editorial/media" << 'PY'
import sys
sys.path.insert(0, sys.argv[1])
import images
open(f"{sys.argv[2]}/icon.png", "wb").write(images.png(512, 512))
open(f"{sys.argv[2]}/main.png", "wb").write(images.png(1280, 800))
open(f"{sys.argv[3]}/banner.png", "wb").write(images.png(1800, 600))
PY
  jq -n --arg id "$id" '{listing_version: 1, id: $id,
    promo: {en: "A throwaway listing"},
    description_md: {en: "Shows **one** key.\n\n- for tests only"},
    icon: {file: "media/icon.png"},
    screenshots: [{file: "media/main.png", alt: {en: "One key on a deck"}, platforms: ["macos", "windows", "linux"]}],
    links: {support: "https://github.com/knittingcode/deckpilot-plugins/issues"},
    release_notes: {"0.1.0": {en: "First release."}}}' > "$d/listing.json"
  jq -n --arg id "$id" '{editorial_version: 1,
    sections: [
      {id: "launch", status: "published", kind: "banner_carousel", order: 10,
       items: [{id: "example-banner", title: {en: "Try the example"}, body: {en: "One key, for tests."},
                artwork: {file: "media/banner.png", alt: {en: "A deck with one key"}},
                target: {type: "plugin", id: $id}, cta: {en: "See plugin"}}]},
      {id: "row", status: "published", kind: "collection_row", order: 20, layout: "cards",
       title: {en: "Examples"}, plugins: [$id]},
      {id: "later", status: "draft", kind: "catalog", order: 30, title: {en: "Draft, never signed"}}]}' \
    > "$dir/editorial/editorial.json"
  git -C "$dir" add -A
  git -C "$dir" -c user.name=t -c user.email=t@t commit -q -m "storefront"
}

# site_publish DIR MODE OUT [ENV=VALUE...]: publish-site.sh MODE against the
# loopback site with DIR's throwaway keys; output and errors in OUT.log.
site_publish() {
  local dir=$1 mode=$2 out=$3
  shift 3
  local args=(--out "$out" --base-url "$site_base")
  [ "$mode" = timestamp ] || args+=(--packages "$dir.packages")
  env -u REGISTRY_STOREFRONT_ENABLED -u REGISTRY_MIN_STATE -u GITHUB_STEP_SUMMARY \
    DECKPILOT_PLUGINS_WORK="$dir.work" REGISTRY_FRESHNESS_WAIT=0 REGISTRY_FRESHNESS_ATTEMPTS=1 \
    REGISTRY_INDEX_KEY="$(cat "$dir.keys/index.key")" REGISTRY_TIMESTAMP_KEY="$(cat "$dir.keys/timestamp.key")" \
    "$@" "$dir/tools/publish-site.sh" "$mode" "${args[@]}" > "$out.log" 2>&1
}

site_clone() {
  local src=$1 dst=$2 ref=$3
  rm -rf "$dst" "$dst.keys" "$dst.packages" "$dst.work"
  cp -R "$src" "$dst"
  cp -R "$src.keys" "$dst.keys"
  cp -R "$src.packages" "$dst.packages"
  mkdir -p "$dst.work/bin"
  ln -s "$site_cli" "$dst.work/bin/deckpilot-registry"
  ln -s "$site_schema" "$dst.work/bin/schema-check"
  rm -rf "$dst/tools"
  git -C "$root" archive "$ref" tools | tar -x -C "$dst"
  git -C "$dst" add -A
  git -C "$dst" -c user.name=t -c user.email=t@t commit -q -m "baseline tools" --allow-empty
}

site_normalized() {
  (cd "$1/site/v1" && find . -type f | LC_ALL=C sort | while read -r f; do
    f=${f#./}
    case $f in
      *.minisig) printf '%s signature\n' "$f" ;;
      timestamp.json) printf '%s %s\n' "$f" "$(jq -cS 'del(.signed_at, .expires, .index.sha256, .index.length)' "$f" | shasum -a 256 | cut -c1-64)" ;;
      index/*.json) printf '%s %s\n' "$f" "$(jq -cS 'del(.generated_at, .expires, .source_commit)' "$f" | shasum -a 256 | cut -c1-64)" ;;
      *) printf '%s %s\n' "$f" "$(shasum -a 256 "$f" | cut -c1-64)" ;;
    esac
  done)
}

site_verify_published() {
  local dir=$1 expected=$2 log=$3 real
  shift 3
  real=$(command -v curl)
  mkdir -p "$dir.bin"
  cat > "$dir.bin/curl" << SHIM
#!/usr/bin/env bash
# Release downloads from $dir.packages; everything else is the real curl.
prefix=https://github.com/knittingcode/deckpilot-plugins/releases/download/
out= url=
args=("\$@")
while [ \$# -gt 0 ]; do
  case \$1 in -o) out=\$2; shift 2 ;; https://*|http://*) url=\$1; shift ;; *) shift ;; esac
done
if [ -n "\$url" ] && [ "\${url#"\$prefix"}" != "\$url" ]; then
  cp "$dir.packages/\${url#"\$prefix"}" "\$out"
  exit
fi
exec "$real" "\${args[@]}"
SHIM
  chmod +x "$dir.bin/curl"
  env -u REGISTRY_STOREFRONT_ENABLED -u GITHUB_STEP_SUMMARY PATH="$dir.bin:$PATH" DECKPILOT_PLUGINS_WORK="$dir.work" "$@" \
    "$dir/tools/verify-published.sh" "$site_base" "$expected" > "$log" 2>&1
}
