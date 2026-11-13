param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputDirectory,
    [ValidateSet('auto', 'cuda', 'hip', 'vulkan', 'all')]
    [string]$RuntimeBackend = 'auto',
    [ValidateSet('universal', 'native', 'sm_89')]
    [string]$CudaArchitecture = 'universal',
    # CUDA 13 dropped Maxwell, Pascal and Volta, so a universal all-backends
    # build adds a second ggml-cuda from a CUDA 12 toolkit: the redist archives
    # unpacked into one folder, or an installed toolkit.
    [string]$Cuda12Root = $env:CUDA_PATH_V12_9,
    # The ROCm Windows SDK the HIP build compiles with: the HIP SDK installer's
    # root, or an extracted TheRock dist. The runtime DLLs the ggml HIP backend
    # imports ship beside the engine, so players need no SDK of their own.
    [string]$RocmPath = $(if ($env:ROCM_PATH) { $env:ROCM_PATH } elseif ($env:HIP_PATH) { $env:HIP_PATH })
)

# Cmdlet failures stop the build; native tools report progress on stderr, so
# for them the exit code is the verdict.
$PSDefaultParameterValues['*:ErrorAction'] = 'Stop'
$ErrorActionPreference = 'Continue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$engineSource = Get-Content -Raw (Join-Path $repoRoot 'engines\yue2-cpp-source.json') | ConvertFrom-Json
# The ggml build tree is deep; a short root keeps it under MAX_PATH.
$engineBuildRoot = if ($env:YUE_ENGINE_BUILD_ROOT) { $env:YUE_ENGINE_BUILD_ROOT } else { $env:TEMP }
# One checkout for every pinned commit: moving it to a new commit leaves the
# build directories in place, so Ninja recompiles only what the commit changed
# instead of all of ggml and its CUDA kernels.
$engineWorktree = Join-Path $engineBuildRoot 'yue2-engine'
$shippedTargets = @('yue-server')

function Test-CudaToolchain {
    $nvcc = Get-Command nvcc -ErrorAction SilentlyContinue
    return [bool]$nvcc
}

function Test-HipToolchain {
    if ([string]::IsNullOrWhiteSpace($RocmPath) -or -not (Test-Path (Join-Path $RocmPath 'bin\hipcc.exe'))) { return $false }
    return $true
}

function Assert-HipSdk {
    if ([string]::IsNullOrWhiteSpace($RocmPath)) {
        throw 'The HIP build requires a ROCm Windows SDK: pass -RocmPath or set ROCM_PATH/HIP_PATH (the HIP SDK install root or an extracted TheRock dist).'
    }
    if (-not (Test-Path (Join-Path $RocmPath 'bin\hipcc.exe'))) { throw "The HIP build requires hipcc.exe under '$RocmPath\bin'." }
}

function Assert-VulkanSdk {
    $sdk = $env:VULKAN_SDK
    $glslc = Get-Command glslc -ErrorAction SilentlyContinue
    if ([string]::IsNullOrWhiteSpace($sdk) -or -not (Test-Path (Join-Path $sdk 'Include\vulkan\vulkan.h')) -or -not $glslc) {
        throw 'The Vulkan build requires a Vulkan SDK with headers, vulkan-1.lib and glslc.'
    }
}

function Resolve-RuntimeBackend {
    switch ($RuntimeBackend) {
        'auto' {
            if (Test-CudaToolchain) { return 'cuda' }
            if (Test-HipToolchain) { return 'hip' }
            if ($env:VULKAN_SDK) { Assert-VulkanSdk; return 'vulkan' }
            throw 'No native toolchain found. Install the CUDA Toolkit, a ROCm Windows SDK or a Vulkan SDK.'
        }
        'cuda' { if (-not (Test-CudaToolchain)) { throw 'The CUDA build requires nvcc.' } }
        'hip' { Assert-HipSdk }
        'vulkan' { Assert-VulkanSdk }
        'all' {
            if (-not (Test-CudaToolchain)) { throw 'The all-backends build requires nvcc.' }
            Assert-VulkanSdk
        }
    }
    return $RuntimeBackend
}

function Get-VcVars64 {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { throw 'vswhere.exe was not found; install Visual Studio C++ Build Tools.' }
    $installationPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installationPath)) {
        throw 'No Visual Studio C++ build installation was found.'
    }
    $vcvars = Join-Path $installationPath.Trim() 'VC\Auxiliary\Build\vcvars64.bat'
    if (-not (Test-Path $vcvars)) { throw "vcvars64.bat is missing: $vcvars" }
    return $vcvars
}

function Sync-PinnedSource {
    if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) { throw 'CMake is required on PATH.' }
    if (-not (Test-Path (Join-Path $engineWorktree '.git'))) {
        git clone --recurse-submodules $engineSource.repository $engineWorktree
        if ($LASTEXITCODE -ne 0) { throw 'Could not clone yue2.cpp.' }
    }
    git -C $engineWorktree fetch origin $engineSource.commit
    if ($LASTEXITCODE -ne 0) { throw "Could not fetch yue2.cpp commit $($engineSource.commit)." }
    git -C $engineWorktree checkout --detach $engineSource.commit
    if ($LASTEXITCODE -ne 0) { throw "Could not check out yue2.cpp commit $($engineSource.commit)." }
    git -C $engineWorktree submodule update --init --recursive
    if ($LASTEXITCODE -ne 0) { throw 'Could not initialise yue2.cpp submodules.' }
}

function Invoke-CMakeBuild([string]$backend) {
    # GGML_NATIVE=OFF plus an explicit architecture list: a binary that runs on
    # other people's CPUs and on Turing through Blackwell. Every consumer card
    # gets device code: PTX needs a driver as new as this toolkit, and an older
    # one fails the first kernel with "PTX was compiled with an unsupported toolchain".
    $cudaArch = switch ($CudaArchitecture) {
        'universal' { '-DGGML_NATIVE=OFF "-DCMAKE_CUDA_ARCHITECTURES=75-real;80-real;86-real;89-real;90-real;120a-real"' }
        'native' { '-DCMAKE_CUDA_ARCHITECTURES=native' }
        'sm_89' { '-DCMAKE_CUDA_ARCHITECTURES=89' }
    }
    $backendFlags = switch ($backend) {
        'cuda' { "-DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF $cudaArch" }
        # The ROCm SDK's hipcc is the C++ compiler for the whole tree: Windows
        # CMake has no HIP language, so ggml-hip compiles the CUDA sources
        # through hipcc's HIP compatibility layer. The dist must carry device
        # code for the card: a gfx110X dist for a 7900 XTX-class Radeon.
        'hip' { '-DGGML_NATIVE=OFF -DGGML_HIP=ON -DGGML_CUDA=OFF -DGGML_VULKAN=OFF -DGGML_HIP_RCCL=OFF -DGGML_BACKEND_DL=ON' }
        'vulkan' { '-DGGML_NATIVE=OFF -DGGML_VULKAN=ON' }
        'all' { "-DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DGGML_VULKAN=ON -DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF $cudaArch" }
    }
    $ccache = if (Get-Command ccache -ErrorAction SilentlyContinue) { '-DGGML_CCACHE=ON' } else { '-DGGML_CCACHE=OFF' }
    # Program databases beside the binaries turn a crash offset into a line.
    $symbols = '-DCMAKE_MSVC_DEBUG_INFORMATION_FORMAT=ProgramDatabase -DCMAKE_EXE_LINKER_FLAGS=/DEBUG -DCMAKE_SHARED_LINKER_FLAGS=/DEBUG'
    $buildDirectoryName = "build-$backend-$CudaArchitecture"
    # With runtime-loaded backends nothing links against ggml-cuda, ggml-vulkan
    # or the CPU variants, so naming yue-server alone would skip them: the
    # all-backends build builds the whole tree, as upstream's buildall does.
    $targets = if ($backend -eq 'all') { '' } elseif ($backend -eq 'hip') { '--target ggml-hip yue-server' } else { ($shippedTargets | ForEach-Object { "--target $_" }) -join ' ' }
    $parallelism = [Math]::Max(1, [Environment]::ProcessorCount)
    $vcvars = Get-VcVars64
    # Ninja drives nvcc and cl directly, so the build does not depend on the
    # CUDA MSBuild integration being installed into this Visual Studio.
    if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) { throw 'Ninja is required on PATH.' }
    # VSLANG=1033: Ninja reads header dependencies from cl's /showIncludes, which a
    # localised Visual Studio prints in its own language; without it an edited
    # header would not rebuild anything.
    # --fresh: a cache from an earlier configure keeps what this one does not pass.
    # The HIP build swaps the compiler: hipcc drives clang for host and device,
    # with MSVC's headers and libraries from vcvars as the ABI.
    $hipEnv = if ($backend -eq 'hip') { "set `"ROCM_PATH=$RocmPath`" && set `"PATH=$RocmPath\bin;%PATH%`" && set `"CC=clang.exe`" && set `"CXX=hipcc.exe`" && " } else { '' }
    $command = "set `"VSLANG=1033`" && call `"$vcvars`" >nul && $hipEnv`cmake --fresh -S . -B `"$buildDirectoryName`" -G Ninja -DCMAKE_BUILD_TYPE=Release $ccache $symbols $backendFlags && cmake --build `"$buildDirectoryName`" $targets --parallel $parallelism"
    Push-Location $engineWorktree
    try { & cmd.exe /d /s /c $command | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw "yue2.cpp $backend build failed." }
    return $buildDirectoryName
}

# The CUDA 12 backend: ggml-cuda alone, from the same source, for the cards
# CUDA 13 no longer targets and for drivers older than CUDA 13. Device code
# for every architecture, including Turing and newer for those old drivers.
function Invoke-Cuda12Build {
    if ([string]::IsNullOrWhiteSpace($Cuda12Root)) {
        throw 'The universal CUDA runtime requires a CUDA 12 toolkit. Set -Cuda12Root or CUDA_PATH_V12_9 to its root directory.'
    }
    $nvcc = Join-Path $Cuda12Root 'bin\nvcc.exe'
    if (-not (Test-Path $nvcc)) { throw "The CUDA 12 backend needs a CUDA 12 toolkit; nvcc.exe is missing under '$Cuda12Root'. Set -Cuda12Root or CUDA_PATH_V12_9." }
    $root = $Cuda12Root.Replace('\', '/')
    $buildDirectoryName = 'build-cuda12-universal'
    # -Wno-deprecated-gpu-targets silences the notice that CUDA 12 is the last
    # toolkit for Maxwell, Pascal and Volta.
    $cudaFlags = '-Wno-deprecated-gpu-targets'
    $flags = "-DGGML_NATIVE=OFF -DGGML_BACKEND_DL=ON -DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF -DGGML_VULKAN=OFF `"-DCMAKE_CUDA_ARCHITECTURES=52-real;60-real;61-real;70-real;75-real;80-real;86-real;89-real;90-real;120a-real`" `"-DCMAKE_CUDA_COMPILER=$root/bin/nvcc.exe`" `"-DCUDAToolkit_ROOT=$root`" `"-DCMAKE_CUDA_FLAGS=$cudaFlags`""
    $ccache = if (Get-Command ccache -ErrorAction SilentlyContinue) { '-DGGML_CCACHE=ON' } else { '-DGGML_CCACHE=OFF' }
    $symbols = '-DCMAKE_MSVC_DEBUG_INFORMATION_FORMAT=ProgramDatabase -DCMAKE_EXE_LINKER_FLAGS=/DEBUG -DCMAKE_SHARED_LINKER_FLAGS=/DEBUG'
    $parallelism = [Math]::Max(1, [Environment]::ProcessorCount)
    $vcvars = Get-VcVars64
    # nvcc 12.9 knows MSVC up to 14.4x (Visual Studio 2022); its front end
    # crashes on the headers of 14.5x. Visual Studio 2026 installs the 2022
    # toolset beside its own as the component
    # Microsoft.VisualStudio.Component.VC.14.44.17.14.x86.x64.
    $toolsRoot = Join-Path (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $vcvars))) 'Tools\MSVC'
    $toolset = Get-ChildItem -Path $toolsRoot -Directory | Where-Object { $_.Name -match '^14\.[34]\d\.' } | Sort-Object { [version]$_.Name } | Select-Object -Last 1
    if (-not $toolset) { throw "The CUDA 12 backend needs an MSVC 14.3x/14.4x toolset beside this Visual Studio; add the component Microsoft.VisualStudio.Component.VC.14.44.17.14.x86.x64." }
    $vcvarsVersion = ($toolset.Name -split '\.')[0..1] -join '.'
    # --fresh: the toolset is part of the configuration, and a cache from
    # another one keeps its compiler. Unchanged objects are not rebuilt.
    $command = "set `"VSLANG=1033`" && set `"CUDA_PATH=$Cuda12Root`" && call `"$vcvars`" -vcvars_ver=$vcvarsVersion >nul && cmake --fresh -S . -B `"$buildDirectoryName`" -G Ninja -DCMAKE_BUILD_TYPE=Release $ccache $symbols $flags && cmake --build `"$buildDirectoryName`" --target ggml-cuda --parallel $parallelism"
    Push-Location $engineWorktree
    try { & cmd.exe /d /s /c $command | Out-Host } finally { Pop-Location }
    if ($LASTEXITCODE -ne 0) { throw 'yue2.cpp CUDA 12 backend build failed.' }
    $dll = Get-ChildItem -Path (Join-Path $engineWorktree $buildDirectoryName) -Recurse -Filter 'ggml-cuda.dll' -File | Select-Object -First 1
    if (-not $dll) { throw 'The CUDA 12 build completed without ggml-cuda.dll.' }
    return $dll.DirectoryName
}

Sync-PinnedSource
$backend = Resolve-RuntimeBackend
$buildDirectoryName = Invoke-CMakeBuild $backend
$shipsTwoCudaBuilds = $backend -eq 'all' -and $CudaArchitecture -eq 'universal'
$cuda12Directory = if ($shipsTwoCudaBuilds) { Invoke-Cuda12Build } else { $null }
$binDirectory = @(
    (Join-Path $engineWorktree "$buildDirectoryName\Release"),
    (Join-Path $engineWorktree "$buildDirectoryName\bin\Release"),
    (Join-Path $engineWorktree $buildDirectoryName)
) | Where-Object { Test-Path (Join-Path $_ 'yue-server.exe') } | Select-Object -First 1
if (-not $binDirectory) { throw 'The yue2.cpp build completed without yue-server.exe.' }

$resolvedOutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if ($resolvedOutputDirectory -eq [System.IO.Path]::GetPathRoot($resolvedOutputDirectory)) {
    throw 'OutputDirectory must be a specific child directory, not a drive root.'
}
New-Item -ItemType Directory -Force -Path $resolvedOutputDirectory | Out-Null
foreach ($target in $shippedTargets) {
    Copy-Item (Join-Path $binDirectory "$target.exe") $resolvedOutputDirectory -Force
    $pdb = Join-Path $binDirectory "$target.pdb"
    if (Test-Path $pdb) { Copy-Item $pdb $resolvedOutputDirectory -Force }
}
# GGML_BACKEND_DL builds backends separately: yue-server can link successfully
# even if the actual HIP module was never built. Fail packaging in that case.
if ($backend -eq 'hip' -and -not (Test-Path (Join-Path $binDirectory 'ggml-hip.dll'))) {
    throw 'HIP build produced yue-server.exe but no ggml-hip.dll; native GPU inference would be unavailable.'
}
Get-ChildItem -Path $binDirectory -Filter '*.dll' -File | Copy-Item -Destination $resolvedOutputDirectory -Force
if ($backend -eq 'hip' -and -not (Test-Path (Join-Path $resolvedOutputDirectory 'ggml-hip.dll'))) {
    throw 'The staged HIP engine does not contain ggml-hip.dll.'
}

# The HIP runtime travels with the engine as the ROCm license allows: the
# backend imports amdhip64, hipblas and rocblas, none of which a display
# driver installs. The set below is the verified closure for gfx1100: the
# direct imports plus amd_comgr (hip needs it to see the device at all),
# rocm_kpack, the rocblas/hipblaslt kernel libraries and the .kpack
# directory, all resolved relative to the DLLs' own folder.
if ($backend -eq 'hip') {
    $rocmBin = Join-Path $RocmPath 'bin'
    foreach ($library in @('amdhip64*.dll', 'hipblas.dll', 'rocblas.dll', 'rocsolver.dll', 'libhipblaslt.dll', 'origami.dll', 'amd_comgr.dll', 'rocm_kpack.dll')) {
        $found = Get-ChildItem -Path $rocmBin -Filter $library -File -ErrorAction SilentlyContinue
        if (-not $found) { throw "The HIP runtime library '$library' was not found under '$rocmBin'." }
        $found | Copy-Item -Destination $resolvedOutputDirectory -Force
    }
    foreach ($dataDir in @('rocblas', 'hipblaslt')) {
        $src = Join-Path $rocmBin $dataDir
        if (Test-Path $src) { Copy-Item $src -Destination $resolvedOutputDirectory -Recurse -Force }
    }
    $kpack = Join-Path $RocmPath '.kpack'
    if (Test-Path $kpack) { Copy-Item $kpack -Destination $resolvedOutputDirectory -Recurse -Force }
}

# Each CUDA backend in a folder of its own, none beside the executable: the
# studio names the one the card and its driver run in YUE_CUDA_BACKEND.
if ($shipsTwoCudaBuilds) {
    foreach ($build in @(@{ Folder = 'cuda13'; Source = $binDirectory }, @{ Folder = 'cuda12'; Source = $cuda12Directory })) {
        $folder = Join-Path $resolvedOutputDirectory $build.Folder
        New-Item -ItemType Directory -Force -Path $folder | Out-Null
        $source = Join-Path $build.Source 'ggml-cuda.dll'
        if (-not (Test-Path $source)) { throw "ggml-cuda.dll is missing from the $($build.Folder) build." }
        Copy-Item $source $folder -Force
    }
    # The CUDA 12 backend imports the CUDA runtime as a DLL, where CUDA 13's
    # cudart.lib links it in: it goes beside the executable, where the loader
    # resolves the backend's imports, as NVIDIA's redistribution terms allow.
    $cudart = Join-Path $Cuda12Root 'bin\cudart64_12.dll'
    if (-not (Test-Path $cudart)) { throw "cudart64_12.dll is missing from $Cuda12Root." }
    Copy-Item $cudart $resolvedOutputDirectory -Force
    Remove-Item (Join-Path $resolvedOutputDirectory 'ggml-cuda.dll') -Force
}

# The Visual C++ runtime travels with the engine, app-local as Microsoft's
# redistribution terms allow, so a clean machine needs no system install.
$vsRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent (Get-VcVars64))))
$redist = Get-ChildItem -Path (Join-Path $vsRoot 'VC\Redist\MSVC') -Directory | Where-Object { $_.Name -match '^\d' } | Sort-Object { [version]$_.Name } | Select-Object -Last 1
if (-not $redist) { throw 'The Visual C++ redistributable folder was not found in this Visual Studio.' }
$crt = Get-ChildItem -Path (Join-Path $redist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.CRT' | Select-Object -First 1
$openmp = Get-ChildItem -Path (Join-Path $redist.FullName 'x64') -Directory -Filter 'Microsoft.VC*.OpenMP' | Select-Object -First 1
foreach ($library in @((Join-Path $crt.FullName 'msvcp140.dll'), (Join-Path $crt.FullName 'msvcp140_1.dll'), (Join-Path $crt.FullName 'vcruntime140.dll'), (Join-Path $crt.FullName 'vcruntime140_1.dll'), (Join-Path $openmp.FullName 'vcomp140.dll'))) {
    if (-not (Test-Path $library)) { throw "Visual C++ runtime library missing: $library" }
    Copy-Item $library $resolvedOutputDirectory -Force
}

# What was staged, so a release can tell whether it must rebuild.
$stamp = [pscustomobject]@{
    commit = $engineSource.commit
    backend = $backend
    cuda_architecture = $CudaArchitecture
    cuda_builds = if ($shipsTwoCudaBuilds) { @('cuda13', 'cuda12') } else { @() }
    runtime = 'yue-server.exe'
}
[System.IO.File]::WriteAllText((Join-Path $resolvedOutputDirectory 'runtime.json'), ($stamp | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))
$stamp | ConvertTo-Json -Compress
