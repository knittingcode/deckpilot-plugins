#!/usr/bin/env bash
# verify-live-registry.sh BASE_URL MANIFEST
#
# After a deployment: fetches every registry path of MANIFEST (written by
# tools/compose-site.py from the verified registry tree) from the live site,
# anonymously, and compares sha256 and size. Proves that what Pages serves
# under the registry paths is byte for byte what the pipeline verified,
# whatever the website around it contains. Pages can take a few minutes to
# serve a new deployment, so mismatches are retried before failing.
. "$(dirname "$0")/lib/common.sh"
base=${1:?usage: verify-live-registry.sh BASE_URL MANIFEST}
manifest=${2:?usage: verify-live-registry.sh BASE_URL MANIFEST}
[[ $base =~ ^https://[a-z0-9.-]+(/[A-Za-z0-9._-]+)*$ ]] || die "bad base URL $base"
tmp=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/live-registry.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

check() {
  local failed=0 path want_sha want_size got_sha got_size code
  while IFS=$'\t' read -r path want_sha want_size; do
    [[ $path =~ ^[A-Za-z0-9._/-]+$ ]] && [[ $path != *..* ]] || die "bad manifest path $path"
    code=$(curl -sS --proto '=https' --max-filesize 8388608 -o "$tmp/file" -w '%{http_code}' \
      -H 'Cache-Control: no-cache' "$base/$path") || code=000
    if [ "$code" != 200 ]; then
      note "/$path: HTTP $code"
      failed=1
      continue
    fi
    got_sha=$(shasum -a 256 "$tmp/file" | cut -d' ' -f1)
    got_size=$(wc -c <"$tmp/file" | tr -d ' ')
    if [ "$got_sha" != "$want_sha" ] || [ "$got_size" != "$want_size" ]; then
      note "/$path: live sha256 $got_sha ($got_size bytes), expected $want_sha ($want_size bytes)"
      failed=1
    fi
  done < <(jq -r 'to_entries[] | [.key, .value.sha256, (.value.size | tostring)] | @tsv' "$manifest")
  return $failed
}

count=$(jq 'length' "$manifest")
[ "$count" -ge 1 ] || die "the manifest lists no registry path"
for attempt in 1 2 3 4 5 6 7 8 9 10; do
  if check; then
    summary "Live registry: all $count path(s) under $base serve the verified bytes."
    note "all $count registry path(s) serve the verified bytes"
    exit 0
  fi
  [ "$attempt" -lt 10 ] || break
  note "attempt $attempt: the live site does not serve the composed registry yet; retrying"
  sleep 30
done
die "the live site does not serve the verified registry bytes"
