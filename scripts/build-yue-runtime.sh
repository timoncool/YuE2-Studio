#!/bin/bash
# macOS counterpart of build-yue-runtime.ps1: builds the pinned yue2.cpp
# `yue-server` with the Metal backend and copies it, with its ggml
# libraries, into OUTPUT_DIRECTORY.
set -euo pipefail

output="${1:?usage: build-yue-runtime.sh OUTPUT_DIRECTORY}"
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

cmake -S "$worktree" -B "$worktree/build" -DCMAKE_BUILD_TYPE=Release \
    -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON
cmake --build "$worktree/build" --config Release --target yue-server \
    -j "$(getconf _NPROCESSORS_ONLN)"

mkdir -p "$output"
cp "$worktree/build/yue-server" "$output/"
# -a keeps the version symlinks of the dylibs; the binary finds them via @rpath.
cp -a "$worktree"/build/*.dylib "$output/"
echo "yue-server ($commit) built into $output"
