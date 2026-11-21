#!/bin/bash
# Builds an unsigned (ad-hoc signed) YuE2 Studio .dmg for Apple Silicon:
# the pinned yue2.cpp engine with Metal is bundled into the app. Model
# weights are not part of it; they are downloaded in the app on first start.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
# Optional runtime-only cache for offline setup. Its contents stay outside the installer.
node "$root/scripts/check-native-runtime-cache.mjs"
engine="$root/desktop/src-tauri/resources/yue2-cpp"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"

if [ ! -x "$engine/yue-server" ] || [ "$(cat "$engine/runtime.commit" 2>/dev/null)" != "$commit" ]; then
    rm -rf "$engine"
    "$root/scripts/build-yue-runtime.sh" "$engine"
    echo "$commit" > "$engine/runtime.commit"
fi

midi="$root/desktop/src-tauri/resources/music-midi"
midi_commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/music-midi-source.json")"
if [ ! -x "$midi/music-midi" ] || [ ! -x "$midi/music-caption" ] || [ "$(cat "$midi/runtime.commit" 2>/dev/null)" != "$midi_commit" ]; then
    rm -rf "$midi"
    "$root/scripts/build-midi-runtime.sh" "$midi"
    echo "$midi_commit" > "$midi/runtime.commit"
fi

cd "$root"
npm --prefix app install
npm --prefix desktop install
cd desktop
# Finder's DMG layout automation can keep the temporary image busy after the
# layout is set. CI mode skips that cosmetic step so the image can be detached.
CI=true npm exec tauri build -- --config src-tauri/tauri.macos-release.conf.json --bundles dmg
bundle="$(node "$root/scripts/release-target-dir.mjs")/bundle"
# Tauri removes the intermediate .app in DMG-only builds. The release check mounts the image.
find "$bundle/dmg" -name '*.dmg'
