#!/usr/bin/env bash
# The caller puts this process in a fresh, loopback-only network namespace.
set -euo pipefail
binary=$(realpath "$1")
archive=$(realpath "$2")
export HOME="$PWD/offline-home" YUE_STUDIO_PORT=18791
export YUE_STUDIO_DATA_ROOT="$PWD/Studio portable"
mkdir -p "$HOME" "$YUE_STUDIO_DATA_ROOT/karaoke/runtime"
base="http://127.0.0.1:$YUE_STUDIO_PORT"
pid=
stop() {
  if [[ -n "$pid" ]]; then kill "$pid" 2>/dev/null || true; wait "$pid" || true; pid=; fi
}
trap stop EXIT
start() {
  "$binary" >"$HOME/service.log" 2>&1 &
  pid=$!
  for _ in {1..120}; do
    if curl --noproxy '*' -fs "$base/health" >/dev/null; then return; fi
    kill -0 "$pid" || { cat "$HOME/service.log"; return 1; }
    sleep .25
  done
  cat "$HOME/service.log"
  return 1
}
install_runtime() {
  curl --noproxy '*' -fsS -H 'Content-Type: application/json' \
    -d '{"asset_id":"onnxruntime"}' "$base/v1/karaoke/install" >/dev/null
  local result
  for _ in {1..120}; do
    result=$(curl --noproxy '*' -fsS "$base/v1/karaoke/status")
    if jq -e '.active_download | .asset_id == "onnxruntime" and .done' <<<"$result" >/dev/null; then
      jq -e '(.active_download.error == null) and any(.assets[]; .id == "onnxruntime" and .installed)' <<<"$result" >/dev/null || { echo "$result"; return 1; }
      jq '.active_download.downloaded_bytes' <<<"$result"
      return
    fi
    sleep .25
  done
  cat "$HOME/service.log"
  return 1
}

cp "$archive" "$YUE_STUDIO_DATA_ROOT/karaoke/runtime/onnxruntime.tgz"
start
install_runtime
stop
record="$YUE_STUDIO_DATA_ROOT/karaoke/runtime/onnxruntime.tgz.json"
jq -e '.sha256 == "bb169b7e374731730d2ae406fb39de7be4b315f413a9132efe896261c31bb503"' "$record" >/dev/null
jq '.url |= sub("timoncool/onnxruntime"; "microsoft/onnxruntime")' "$record" >"$record.old"
mv "$record.old" "$record"
mv "$YUE_STUDIO_DATA_ROOT" "$PWD/Перенесённая студия"
export YUE_STUDIO_DATA_ROOT="$PWD/Перенесённая студия"
test ! -e "$YUE_STUDIO_DATA_ROOT/karaoke/runtime/onnxruntime.tgz"
start
bytes=$(install_runtime)
test "$bytes" = 0
echo "PASS manual offline import and repeat install after relocating the legacy record (0 bytes)"
