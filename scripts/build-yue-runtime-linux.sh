#!/bin/bash
# Linux counterpart of build-yue-runtime.ps1: builds the pinned yue2.cpp
# `yue-server` with the Vulkan backend and a build of ggml-cpu for every
# processor generation, each loaded as a library, and copies it with its
# libraries into OUTPUT_DIRECTORY. The libraries are found beside the binary
# ($ORIGIN), wherever the package puts them.
set -euo pipefail

output="${1:?usage: build-yue-runtime-linux.sh OUTPUT_DIRECTORY}"
root="$(cd "$(dirname "$0")/.." && pwd)"
repository="$(sed -n 's/.*"repository": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"
worktree="${YUE_ENGINE_BUILD_ROOT:-${TMPDIR:-/tmp}}/yue2-engine"

if [ ! -d "$worktree/.git" ]; then
    git clone "$repository" "$worktree"
fi
git -C "$worktree" fetch --quiet origin "$commit" || git -C "$worktree" fetch --quiet origin
git -C "$worktree" checkout --quiet "$commit"
git -C "$worktree" submodule update --init --recursive

cmake -S "$worktree" -B "$worktree/build-linux" -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DGGML_NATIVE=OFF -DGGML_VULKAN=ON -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON \
    -DCMAKE_BUILD_RPATH='$ORIGIN' -DCMAKE_INSTALL_RPATH='$ORIGIN' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON
# the backends are modules nothing links against, so each is a target of its own
variants="$(sed -n 's/^build \(ggml-cpu-[a-z0-9_]*\): phony.*/\1/p' "$worktree/build-linux/build.ninja" | sort -u | tr '\n' ' ')"
[ -n "$variants" ] || { echo "[ERROR] the configured build has no processor variants of ggml-cpu" >&2; exit 1; }
targets="yue-server ggml-vulkan $variants"
cmake --build "$worktree/build-linux" --config Release --target $targets -j "$(getconf _NPROCESSORS_ONLN)"

mkdir -p "$output"
server="$(find "$worktree/build-linux" -maxdepth 2 -type f -name yue-server | head -n 1)"
[ -n "$server" ] || { echo "[ERROR] the build completed without yue-server" >&2; exit 1; }
cp "$server" "$output/"
# -a keeps the version symlinks; the binary finds them through $ORIGIN.
find "$worktree/build-linux" -maxdepth 2 -name 'lib*.so*' -exec cp -a {} "$output/" \;
# The CUDA backend, when this machine has the CUDA 13 toolkit: ggml-cuda alone, loaded by the engine from cuda13/ when
# the studio names it, device code for every architecture CUDA 13 targets, no PTX. Its CUDA runtime and cuBLAS are not
# shipped: the studio downloads NVIDIA's wheels into one folder the engine and the audio.cpp server share, and names it
# in LD_LIBRARY_PATH.
backend="vulkan"
if command -v nvcc >/dev/null 2>&1; then
    rm -rf "$worktree/build-linux-cuda13"
    cmake -S "$worktree" -B "$worktree/build-linux-cuda13" -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF -DGGML_CUDA_FA_ALL_QUANTS=ON \
        "-DCMAKE_CUDA_ARCHITECTURES=75-real;80-real;86-real;89-real;90-real;120a-real" \
        -DCMAKE_BUILD_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_INSTALL_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON
    cmake --build "$worktree/build-linux-cuda13" --config Release --target ggml-cuda -j "$(getconf _NPROCESSORS_ONLN)"
    cuda="$(find "$worktree/build-linux-cuda13" -name 'libggml-cuda.so' -type f | head -n 1)"
    [ -n "$cuda" ] || { echo "[ERROR] the CUDA build completed without libggml-cuda.so" >&2; exit 1; }
    mkdir -p "$output/cuda13"
    cp "$cuda" "$output/cuda13/"
    # CMake appends the toolkit's library folder of this machine to the RUNPATH
    patchelf --set-rpath '$ORIGIN:$ORIGIN/..' "$output/cuda13/libggml-cuda.so"
    backend="cuda13,vulkan"
fi
# The CUDA 12 backend, when the CUDA 12.9 toolkit is there too (CUDA12_ROOT, else /usr/local/cuda-12.9): the cards
# CUDA 13 dropped (Maxwell, Pascal, Volta) and drivers from 525, the same table as on Windows.
cuda12_root="${CUDA12_ROOT:-/usr/local/cuda-12.9}"
if [ -x "$cuda12_root/bin/nvcc" ]; then
    rm -rf "$worktree/build-linux-cuda12"
    cmake -S "$worktree" -B "$worktree/build-linux-cuda12" -G Ninja -DCMAKE_BUILD_TYPE=Release \
        -DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF -DGGML_CUDA_FA_ALL_QUANTS=ON \
        "-DCMAKE_CUDA_COMPILER=$cuda12_root/bin/nvcc" "-DCUDAToolkit_ROOT=$cuda12_root" -DCMAKE_CUDA_FLAGS=-Wno-deprecated-gpu-targets \
        "-DCMAKE_CUDA_ARCHITECTURES=52-real;60-real;61-real;70-real;75-real;80-real;86-real;89-real;90-real;120a-real" \
        -DCMAKE_BUILD_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_INSTALL_RPATH='$ORIGIN:$ORIGIN/..' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON
    cmake --build "$worktree/build-linux-cuda12" --config Release --target ggml-cuda -j "$(getconf _NPROCESSORS_ONLN)"
    cuda="$(find "$worktree/build-linux-cuda12" -name 'libggml-cuda.so' -type f | head -n 1)"
    [ -n "$cuda" ] || { echo "[ERROR] the CUDA 12 build completed without libggml-cuda.so" >&2; exit 1; }
    mkdir -p "$output/cuda12"
    cp "$cuda" "$output/cuda12/"
    patchelf --set-rpath '$ORIGIN:$ORIGIN/..' "$output/cuda12/libggml-cuda.so"
    backend="cuda12,$backend"
fi
printf '{"commit":"%s","backend":"%s","platform":"linux"}\n' "$commit" "$backend" > "$output/runtime.json"
for binary in "$output"/yue-server "$output"/lib*.so* "$output"/cuda13/*.so* "$output"/cuda12/*.so*; do
    [ -f "$binary" ] && [ ! -L "$binary" ] || continue
    # every entry of the path, not the line: CMake appends the toolkit's folder after $ORIGIN
    if readelf -d "$binary" | grep -iE 'runpath|rpath' | grep -qE '(\[|:)/'; then
        echo "[ERROR] $binary keeps a build-machine library path:" >&2
        readelf -d "$binary" | grep -iE 'runpath|rpath' >&2
        exit 1
    fi
done
echo "yue-server ($commit) built into $output"
