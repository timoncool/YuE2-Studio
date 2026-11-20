#!/bin/bash
# Linux counterpart of build-yue-runtime.ps1: builds the pinned yue2.cpp
# `yue-server` with the Vulkan backend and a build of ggml-cpu for every
# processor generation, each loaded as a library, and copies it with its
# libraries into OUTPUT_DIRECTORY. The libraries are found beside the binary
# ($ORIGIN), wherever the package puts them.
set -euo pipefail

destination="$(realpath -m "${1:?usage: build-yue-runtime-linux.sh OUTPUT_DIRECTORY}")"
[[ "$destination" != / ]] || { echo "[ERROR] runtime output must be a dedicated directory" >&2; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
repository="$(sed -n 's/.*"repository": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"
commit="$(sed -n 's/.*"commit": "\(.*\)".*/\1/p' "$root/engines/yue2-cpp-source.json")"
build_root="${YUE_ENGINE_BUILD_ROOT:-${TMPDIR:-/tmp}}"
worktree="$build_root/yue2-engine"
cuda_profile="${YUE_CUDA_PROFILE:-universal}"
case "$cuda_profile" in
    universal) ;;
    sm89) destination="$destination-dev-sm89" ;;
    *) echo "[ERROR] YUE_CUDA_PROFILE must be universal or sm89" >&2; exit 1 ;;
esac

mkdir -p "$build_root"
exec 9>"$build_root/yue2-engine.build.lock"
flock 9

available_cpus=$(nproc)
if [[ -r /sys/fs/cgroup/cpu.max ]] && read -r quota period </sys/fs/cgroup/cpu.max && [[ "$quota" != max ]]; then
    quota_cpus=$((quota / period))
    (( quota_cpus >= available_cpus )) || available_cpus=$quota_cpus
fi
read -r total_memory available_memory < <(
    awk '/^MemTotal:/ { total=$2 } /^MemAvailable:/ { available=$2 } END { print total, available }' /proc/meminfo
)
if [[ -r /sys/fs/cgroup/memory.max ]] && read -r memory_limit </sys/fs/cgroup/memory.max && [[ "$memory_limit" != max ]]; then
    read -r memory_used </sys/fs/cgroup/memory.current
    (( memory_limit / 1024 >= total_memory )) || total_memory=$((memory_limit / 1024))
    remaining=$(((memory_limit - memory_used) / 1024))
    (( remaining >= available_memory )) || available_memory=$remaining
fi
cpu_jobs=$((available_cpus * 80 / 100))
reserve=$((total_memory / 5))
memory_jobs=$(((available_memory - reserve) / 1048576))
jobs=$((cpu_jobs < memory_jobs ? cpu_jobs : memory_jobs))
(( jobs > 0 )) || jobs=1
if [[ -n "${YUE_BUILD_JOBS:-}" ]]; then
    if ! [[ "$YUE_BUILD_JOBS" =~ ^[1-9][0-9]*$ ]] || (( YUE_BUILD_JOBS > jobs )); then
        echo "[ERROR] YUE_BUILD_JOBS must be between 1 and $jobs on this host" >&2
        exit 1
    fi
    jobs=$YUE_BUILD_JOBS
fi
printf '[build] Ninja -j%s, effective CPU=%s, estimated available RAM=%s MiB, reserved RAM=%s MiB\n' \
    "$jobs" "$available_cpus" "$((available_memory / 1024))" "$((reserve / 1024))"

if command -v ccache >/dev/null 2>&1; then
    export CCACHE_DIR="${CCACHE_DIR:-$build_root/yue2-ccache}"
    export CMAKE_C_COMPILER_LAUNCHER="${CMAKE_C_COMPILER_LAUNCHER-ccache}"
    export CMAKE_CXX_COMPILER_LAUNCHER="${CMAKE_CXX_COMPILER_LAUNCHER-ccache}"
    export CMAKE_CUDA_COMPILER_LAUNCHER="${CMAKE_CUDA_COMPILER_LAUNCHER-ccache}"
fi

tool_versions() {
    local tool
    for tool in "$@"; do
        # Compiler environment variables also accept arguments, recorded by the caller.
        tool=${tool%% *}
        command -v "$tool"
        "$tool" --version
    done
}

build_cuda() {
    local major=$1 nvcc=$2 architectures=$3
    shift 3
    if [[ "$cuda_profile" == sm89 ]]; then architectures=89-real; fi
    nvcc=$(realpath "$nvcc")
    local nvcc_version
    nvcc_version=$("$nvcc" --version)
    if [[ "$nvcc_version" != *"release $major."* ]]; then
        echo "[ERROR] CUDA $major build selected an incompatible toolkit at $nvcc" >&2
        return 1
    fi
    local key build_dir
    local args=(
        -G Ninja -DCMAKE_BUILD_TYPE=Release
        -DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF -DGGML_CUDA_FA_ALL_QUANTS=ON
        "-DCMAKE_CUDA_ARCHITECTURES=$architectures" "-DCMAKE_CUDA_COMPILER=$nvcc"
        "-DCMAKE_CUDA_COMPILER_LAUNCHER=${CMAKE_CUDA_COMPILER_LAUNCHER:-}"
        '-DCMAKE_BUILD_RPATH=$ORIGIN:$ORIGIN/..' '-DCMAKE_INSTALL_RPATH=$ORIGIN:$ORIGIN/..'
        -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON "$@"
    )
    # CMake caches compiler paths and initial flags. A different toolchain needs a fresh directory.
    key=$(
        {
            printf '%s\0' "${args[@]}" "${CC:-}" "${CXX:-}" "${CUDAHOSTCXX:-}" \
                "${CFLAGS:-}" "${CXXFLAGS:-}" "${CUDAFLAGS:-}" "${LDFLAGS:-}" \
                "${CMAKE_TOOLCHAIN_FILE:-}" "${CMAKE_PREFIX_PATH:-}" \
                "${CMAKE_C_COMPILER_LAUNCHER:-}" "${CMAKE_CXX_COMPILER_LAUNCHER:-}"
            if [[ -f "${CMAKE_TOOLCHAIN_FILE:-}" ]]; then sha256sum "$CMAKE_TOOLCHAIN_FILE"; fi
            tool_versions cmake ninja "${CC:-cc}" "${CXX:-c++}" "${CUDAHOSTCXX:-${CXX:-c++}}" "$nvcc"
        } | sha256sum
    )
    build_dir="$worktree/build-linux-cuda$major-$cuda_profile-${key:0:16}"
    printf '[build] CUDA%s %s (%s) in %s\n' "$major" "$cuda_profile" "$architectures" "$build_dir"
    cmake -S "$worktree" -B "$build_dir" "${args[@]}"
    cmake --build "$build_dir" --config Release --target ggml-cuda -j "$jobs"
    local cuda
    cuda="$(find "$build_dir" -name 'libggml-cuda.so' -type f -print -quit)"
    [ -n "$cuda" ] || { echo "[ERROR] the CUDA $major build completed without libggml-cuda.so" >&2; exit 1; }
    mkdir -p "$output/cuda$major"
    cp "$cuda" "$output/cuda$major/"
    # Strip the local toolkit's library directory from the package RUNPATH.
    patchelf --set-rpath '$ORIGIN:$ORIGIN/..' "$output/cuda$major/libggml-cuda.so"
}

mkdir -p "$(dirname "$destination")"
output=$(mktemp -d "$(dirname "$destination")/.yue2-runtime.XXXXXXXX")
trap 'rm -rf -- "$output"' EXIT

if [ ! -d "$worktree/.git" ]; then
    git clone "$repository" "$worktree"
fi
git -C "$worktree" fetch --quiet origin "$commit" || git -C "$worktree" fetch --quiet origin
git -C "$worktree" checkout --quiet "$commit"
git -C "$worktree" submodule update --init --recursive

# Keep CPU/Vulkan independent of CUDA and of Ninja logs from previous tool versions.
cpu_toolchain=$(
    {
        tool_versions cmake ninja "${CC:-cc}" "${CXX:-c++}"
        printf '%s\0' "${CC:-}" "${CXX:-}" "${CFLAGS:-}" "${CXXFLAGS:-}" "${LDFLAGS:-}" \
            "${CMAKE_TOOLCHAIN_FILE:-}" "${CMAKE_PREFIX_PATH:-}" "${VULKAN_SDK:-}" \
            "${CMAKE_C_COMPILER_LAUNCHER:-}" "${CMAKE_CXX_COMPILER_LAUNCHER:-}"
        if [[ -f "${CMAKE_TOOLCHAIN_FILE:-}" ]]; then sha256sum "$CMAKE_TOOLCHAIN_FILE"; fi
    } | sha256sum
)
cpu_build="$worktree/build-linux-vulkan-${cpu_toolchain:0:16}"
cmake -S "$worktree" -B "$cpu_build" -G Ninja -DCMAKE_BUILD_TYPE=Release \
    -DGGML_NATIVE=OFF -DGGML_VULKAN=ON -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON \
    -DCMAKE_BUILD_RPATH='$ORIGIN' -DCMAKE_INSTALL_RPATH='$ORIGIN' -DCMAKE_BUILD_WITH_INSTALL_RPATH=ON
# the backends are modules nothing links against, so each is a target of its own
variants="$(sed -n 's/^build \(ggml-cpu-[a-z0-9_]*\): phony.*/\1/p' "$cpu_build/build.ninja" | sort -u | tr '\n' ' ')"
[ -n "$variants" ] || { echo "[ERROR] the configured build has no processor variants of ggml-cpu" >&2; exit 1; }
targets="yue-server ggml-vulkan $variants"
cmake --build "$cpu_build" --config Release --target $targets -j "$jobs"

mkdir -p "$output"
server="$(find "$cpu_build" -maxdepth 2 -type f -name yue-server | head -n 1)"
[ -n "$server" ] || { echo "[ERROR] the build completed without yue-server" >&2; exit 1; }
cp "$server" "$output/"
# -a keeps the version symlinks; the binary finds them through $ORIGIN.
find "$cpu_build" -maxdepth 2 -name 'lib*.so*' -exec cp -a {} "$output/" \;
# The CUDA backend, when this machine has the CUDA 13 toolkit: ggml-cuda alone, loaded by the engine from cuda13/ when
# the studio names it, device code for every architecture CUDA 13 targets, no PTX. Its CUDA runtime and cuBLAS are not
# shipped: the studio downloads NVIDIA's wheels into one folder the engine and the audio.cpp server share, and names it
# in LD_LIBRARY_PATH.
backend="vulkan"
cuda13_nvcc="${CUDA13_ROOT:+$CUDA13_ROOT/bin/nvcc}"
if [[ -z "$cuda13_nvcc" ]]; then cuda13_nvcc=$(command -v nvcc || true); fi
if [[ -n "$cuda13_nvcc" ]]; then
    build_cuda 13 "$cuda13_nvcc" '75-real;80-real;86-real;89-real;90-real;120a-real'
    backend="cuda13,vulkan"
fi
# CUDA 12.9 also supports the Maxwell, Pascal and Volta cards dropped by CUDA 13.
cuda12_root="${CUDA12_ROOT:-/usr/local/cuda-12.9}"
if [ -x "$cuda12_root/bin/nvcc" ]; then
    build_cuda 12 "$cuda12_root/bin/nvcc" '52-real;60-real;61-real;70-real;75-real;80-real;86-real;89-real;90-real;120a-real' \
        "-DCUDAToolkit_ROOT=$cuda12_root" -DCMAKE_CUDA_FLAGS=-Wno-deprecated-gpu-targets
    backend="cuda12,$backend"
fi
if [[ "$cuda_profile" == universal ]]; then
    printf '{"commit":"%s","backend":"%s","platform":"linux"}\n' "$commit" "$backend" > "$output/runtime.json"
else
    printf '{"commit":"%s","backend":"%s","platform":"linux","cuda_profile":"%s"}\n' \
        "$commit" "$backend" "$cuda_profile" > "$output/runtime.json"
fi
for binary in "$output"/yue-server "$output"/lib*.so* "$output"/cuda13/*.so* "$output"/cuda12/*.so*; do
    [ -f "$binary" ] && [ ! -L "$binary" ] || continue
    # every entry of the path, not the line: CMake appends the toolkit's folder after $ORIGIN
    if readelf -d "$binary" | grep -iE 'runpath|rpath' | grep -qE '(\[|:)/'; then
        echo "[ERROR] $binary keeps a build-machine library path:" >&2
        readelf -d "$binary" | grep -iE 'runpath|rpath' >&2
        exit 1
    fi
done
# Publish only after validation. A reused destination can contain obsolete GPU modules.
mkdir -p "$destination"
rm -f -- "$destination/runtime.json" "$destination"/libggml-cuda.so* "$destination"/libggml-hip.so*
rm -rf -- "$destination/cuda12" "$destination/cuda13" "$destination/dev-sm89"
for item in "$output"/*; do
    [[ "$item" == "$output/runtime.json" ]] || cp -a -- "$item" "$destination/"
done
mv -- "$output/runtime.json" "$destination/runtime.json"
echo "yue-server ($commit) built into $destination"
