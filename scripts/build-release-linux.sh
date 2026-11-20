#!/bin/bash
# Builds YuE2 Studio for Linux x86-64: a .deb and an AppImage with the pinned
# yue2.cpp engine on Vulkan and CUDA 13 and the audio-to-MIDI transcriber bundled. Model weights are downloaded in the app.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
engine="$root/desktop/src-tauri/resources/yue2-cpp"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"

release_runtime_ready() {
    [[ -x "$engine/yue-server" && -f "$engine/libggml-vulkan.so" && -f "$engine/libggml-cpu-x64.so" && -f "$engine/runtime.json" ]] || return 1
    local backend cuda relative
    backend=$(sed -n "s/^{\"commit\":\"$commit\",\"backend\":\"\([^\"]*\)\",\"platform\":\"linux\"}$/\1/p" "$engine/runtime.json")
    case "$backend" in
        vulkan | cuda12,vulkan | cuda13,vulkan | cuda12,cuda13,vulkan) ;;
        *) return 1 ;;
    esac
    # Reuse a packaged backend without its build toolkit, but do not skip one available here.
    if [[ -n "${CUDA13_ROOT:-}" ]] || command -v nvcc >/dev/null; then
        [[ ",$backend," == *,cuda13,* ]] || return 1
    fi
    if [[ -x "${CUDA12_ROOT:-/usr/local/cuda-12.9}/bin/nvcc" ]]; then
        [[ ",$backend," == *,cuda12,* ]] || return 1
    fi
    # The Tauri resource mapping copies this tree recursively.
    for cuda in cuda12 cuda13; do
        if [[ ",$backend," == *",$cuda,"* ]]; then
            [[ ! -L "$engine/$cuda" && -f "$engine/$cuda/libggml-cuda.so" &&
                ! -L "$engine/$cuda/libggml-cuda.so" ]] || return 1
        fi
    done
    while IFS= read -r -d '' relative; do
        relative=${relative#"$engine/"}
        case "$relative" in
            cuda12 | cuda12/libggml-cuda.so)
                [[ ",$backend," == *,cuda12,* ]] || return 1 ;;
            cuda13 | cuda13/libggml-cuda.so)
                [[ ",$backend," == *,cuda13,* ]] || return 1 ;;
            yue-server | runtime.json | libggml.so* | libggml-base.so* | libggml-cpu-*.so* | libggml-vulkan.so*)
                [[ ! -d "$engine/$relative" ]] || return 1 ;;
            *) return 1 ;;
        esac
    done < <(find "$engine" -mindepth 1 -print0)
}

if ! release_runtime_ready; then
    rm -rf "$engine"
    YUE_CUDA_PROFILE=universal "$root/scripts/build-yue-runtime-linux.sh" "$engine"
fi
if ! release_runtime_ready; then
    echo "[ERROR] YuE2 Linux release staging has an unexpected profile or backend file" >&2
    exit 1
fi

midi="$root/desktop/src-tauri/resources/music-midi"
midi_commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/music-midi-source.json")"
if [ ! -x "$midi/music-midi" ] || [ ! -x "$midi/music-caption" ] || [ "$(cat "$midi/runtime.commit" 2>/dev/null)" != "$midi_commit" ]; then
    rm -rf "$midi"
    "$root/scripts/build-midi-runtime-linux.sh" "$midi"
fi

cd "$root"
npm --prefix app ci --no-audit --no-fund
npm --prefix desktop ci --no-audit --no-fund
cd desktop
# the display driver's libcuda and the CUDA libraries the studio downloads are never packed
export LINUXDEPLOY_EXCLUDED_LIBRARIES="libcuda.so*;libcudart.so*;libcublas.so*;libcublasLt.so*"
npm exec tauri build -- --config src-tauri/tauri.linux-release.conf.json --bundles deb,appimage

find src-tauri/target/release/bundle -name '*.deb' -o -name '*.AppImage'
