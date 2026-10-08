#!/bin/bash
# Builds YuE2 Studio for Linux x86-64: a .deb and an AppImage with the pinned
# yue2.cpp engine on Vulkan bundled. Model weights are downloaded in the app.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
engine="$root/desktop/src-tauri/resources/yue2-cpp"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"

if [ ! -x "$engine/yue-server" ] || ! grep -q "$commit" "$engine/runtime.json" 2>/dev/null; then
    rm -rf "$engine"
    "$root/scripts/build-yue-runtime-linux.sh" "$engine"
fi

cd "$root"
npm --prefix app ci --no-audit --no-fund
npm --prefix desktop ci --no-audit --no-fund
cd desktop
npm exec tauri build -- --config src-tauri/tauri.linux-release.conf.json --bundles deb,appimage

find src-tauri/target/release/bundle -name '*.deb' -o -name '*.AppImage'
