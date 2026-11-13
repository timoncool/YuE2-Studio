param(
    [Parameter(Mandatory = $true)]
    [ValidateNotNullOrEmpty()]
    [string]$OutputDirectory,
    [ValidateSet('universal', 'sm_89')]
    [string]$CudaArchitecture = 'universal',
    [ValidateSet('cuda13', 'cuda12', 'cpu')]
    [string]$Build = 'cuda13'
)

# Builds HOT-Step's tools at the pinned commit.
#   cuda13 (default): one package - the adapter trainer (ace-train, staged as
#     music-train.exe), the audio-to-MIDI transcriber (ace-midi, staged as
#     music-midi.exe) and the captioner (ace-caption) - on one set of ggml
#     libraries, zipped for the release asset that the training pack and Audio
#     to MIDI both download into one folder; and the captioner's own asset.
#   cuda12: the transcriber alone on CUDA 12.9 (MSVC 14.44), for the cards and
#     drivers CUDA 13 has no code for.
#   cpu: the transcriber alone without CUDA, for machines without an NVIDIA card.
# Its ggml carries HOT-Step's patches, so it never shares a folder with the engine's.

$PSDefaultParameterValues['*:ErrorAction'] = 'Stop'
$ErrorActionPreference = 'Continue'

$repoRoot = Split-Path -Parent $PSScriptRoot
$source = Get-Content -Raw (Join-Path $repoRoot 'engines\music-train-source.json') | ConvertFrom-Json
$midi = Get-Content -Raw (Join-Path $repoRoot 'engines\music-midi-source.json') | ConvertFrom-Json
foreach ($field in 'repository', 'commit', 'release_tag', 'asset', 'cuda_architectures') {
    if ($source.$field -ne $midi.$field) { throw "engines\music-midi-source.json and music-train-source.json name different $field ($($midi.$field) / $($source.$field)): the tools are one package." }
}
$buildRoot = if ($env:YUE_ENGINE_BUILD_ROOT) { $env:YUE_ENGINE_BUILD_ROOT } else { $env:TEMP }
$worktree = Join-Path $buildRoot "hotstep-$($source.commit.Substring(0, 8))"

function Get-VcVars64 {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) { throw 'vswhere.exe was not found; install Visual Studio C++ Build Tools.' }
    $installationPath = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($installationPath)) { throw 'No Visual Studio C++ build installation was found.' }
    $vcvars = Join-Path $installationPath.Trim() 'VC\Auxiliary\Build\vcvars64.bat'
    if (-not (Test-Path $vcvars)) { throw "vcvars64.bat is missing: $vcvars" }
    return $vcvars
}

$cuda12Root = 'D:\Projects\_tools\cuda-12.9'
if ($Build -eq 'cuda13' -and -not (Get-Command nvcc -ErrorAction SilentlyContinue)) { throw 'The trainer is CUDA only: nvcc is required.' }
if ($Build -eq 'cuda12' -and -not (Test-Path (Join-Path $cuda12Root 'bin\nvcc.exe'))) { throw "CUDA 12.9 is not at $cuda12Root." }
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) { throw 'Ninja is required on PATH.' }

if (-not (Test-Path (Join-Path $worktree '.git'))) {
    git clone $source.repository $worktree
    if ($LASTEXITCODE -ne 0) { throw 'Could not clone HOT-Step-CPP.' }
}
git -C $worktree fetch origin $source.commit
if ($LASTEXITCODE -ne 0) { throw "Could not fetch HOT-Step-CPP commit $($source.commit)." }
git -C $worktree checkout --detach $source.commit
if ($LASTEXITCODE -ne 0) { throw "Could not check out HOT-Step-CPP commit $($source.commit)." }
# ggml carries the training patches; the VST3 SDK is only needed for the
# build tree to configure.
git -C $worktree submodule update --init --recursive engine/ggml engine/vendor/vst3sdk
if ($LASTEXITCODE -ne 0) { throw 'Could not initialise the HOT-Step submodules.' }

# Both tools load each ggml backend as a library, with a build of ggml-cpu for
# every processor generation: without the CUDA runtime, on a card the build has
# no code for, or on a processor without AVX2 they start on what is there
# instead of stopping with "no backend available" or 0xc000001d. Vulkan is not
# built: the transcriber's Vulkan path writes wrong notes (HOT-Step 3e7a0778 and
# 91e92a8a alike), and the trainer computes on CUDA. Machine code for every
# card, no PTX; no NCCL. tinyBLAS and CUDA graphs are on, as llama.cpp ships
# them: standalone ggml leaves both off.
$common = '-DGGML_VULKAN=OFF -DGGML_BACKEND_DL=ON -DGGML_CPU_ALL_VARIANTS=ON -DGGML_NATIVE=OFF -DGGML_CCACHE=OFF -DGGML_LLAMAFILE=ON -DGGML_CUDA_GRAPHS=ON'
switch ($Build) {
    'cuda13' {
        # BF16 training needs Ampere or newer; transcription runs on Turing too.
        $architectures = switch ($CudaArchitecture) {
            'universal' { $source.cuda_architectures }
            'sm_89' { '89-real' }
        }
        $flags = "-DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF `"-DCMAKE_CUDA_ARCHITECTURES=$architectures`""
        $setup = "call `"$(Get-VcVars64)`" >nul"
        $tools = @($source.target, $midi.target, 'ace-caption')
        $buildDirectory = "build-tools-$CudaArchitecture"
    }
    'cuda12' {
        $flags = "-DGGML_CUDA=ON -DGGML_CUDA_NCCL=OFF -DCUDAToolkit_ROOT=$($cuda12Root -replace '\\', '/') -DCMAKE_CUDA_COMPILER=$($cuda12Root -replace '\\', '/')/bin/nvcc.exe -DCMAKE_CUDA_FLAGS=-Wno-deprecated-gpu-targets `"-DCMAKE_CUDA_ARCHITECTURES=$($midi.cuda12_architectures)`""
        $setup = "call `"$(Get-VcVars64)`" -vcvars_ver=14.44 >nul && set `"CUDA_PATH=$cuda12Root`" && set `"PATH=$cuda12Root\bin;!PATH!`""
        $tools = @($midi.target)
        $buildDirectory = 'build-midi-cuda12'
    }
    'cpu' {
        $flags = '-DGGML_CUDA=OFF'
        $setup = "call `"$(Get-VcVars64)`" >nul"
        $tools = @($midi.target)
        $buildDirectory = 'build-midi-cpu'
    }
}
$symbols = '-DCMAKE_MSVC_DEBUG_INFORMATION_FORMAT=ProgramDatabase -DCMAKE_EXE_LINKER_FLAGS=/DEBUG -DCMAKE_SHARED_LINKER_FLAGS=/DEBUG'
# settings come from this script alone, not from a cache an earlier run left
$configure = "$setup && set `"VSLANG=1033`" && cmake --fresh -S . -B `"$buildDirectory`" -G Ninja -DCMAKE_BUILD_TYPE=Release $common $flags $symbols"
Push-Location (Join-Path $worktree $source.source_dir)
try { & cmd.exe /d /v:on /s /c $configure | Out-Host } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'Configuring the HOT-Step tools failed.' }
# every processor variant is a target of its own: ggml-cpu-haswell, ggml-cpu-alderlake...
$ninja = Get-Content -Raw (Join-Path $worktree "$($source.source_dir)\$buildDirectory\build.ninja")
$variants = [regex]::Matches($ninja, '(?m)^build (ggml-cpu-[a-z0-9_]+): phony') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique
if ($variants.Count -eq 0) { throw 'The configured build has no processor variants of ggml-cpu.' }
$cudaTarget = if ($Build -eq 'cpu') { '' } else { 'ggml-cuda' }
$targets = "$($tools -join ' ') $cudaTarget $($variants -join ' ')"
$command = "$setup && set `"VSLANG=1033`" && cmake --build `"$buildDirectory`" --target $targets --parallel 8"
Push-Location (Join-Path $worktree $source.source_dir)
try { & cmd.exe /d /v:on /s /c $command | Out-Host } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw 'The HOT-Step tools build failed.' }

$binDirectory = Join-Path $worktree "$($source.source_dir)\$buildDirectory"

$output = [System.IO.Path]::GetFullPath($OutputDirectory)
if ($output -eq [System.IO.Path]::GetPathRoot($output)) { throw 'OutputDirectory must be a specific child directory, not a drive root.' }
if (Test-Path $output) { Remove-Item -Recurse -Force $output }
New-Item -ItemType Directory -Force -Path $output | Out-Null
$shipped = @{ $source.target = $source.shipped_as; $midi.target = $midi.shipped_as; 'ace-caption' = 'ace-caption.exe' }
foreach ($tool in $tools) {
    $built = Join-Path $binDirectory "$tool.exe"
    if (-not (Test-Path $built)) { throw "The build completed without $tool.exe." }
    Copy-Item $built (Join-Path $output $shipped[$tool]) -Force
}
Get-ChildItem -Path $binDirectory -Filter 'ggml*.dll' -File | Copy-Item -Destination $output -Force
Copy-Item (Join-Path $worktree "$($source.source_dir)\LICENSE") (Join-Path $output 'LICENSE-HOT-Step.txt') -Force

$stamp = [pscustomobject]@{ commit = $source.commit; build = $Build; cuda_architecture = $CudaArchitecture; runtimes = @($tools | ForEach-Object { $shipped[$_] }) }
[System.IO.File]::WriteAllText((Join-Path $output 'runtime.json'), ($stamp | ConvertTo-Json), (New-Object System.Text.UTF8Encoding($false)))

$assetName = switch ($Build) { 'cuda13' { $source.asset } 'cuda12' { $midi.cuda12_asset } 'cpu' { $midi.cpu_asset } }
$zip = Join-Path (Split-Path -Parent $output) $assetName
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path "$output\*" -DestinationPath $zip -Force
[pscustomobject]@{ zip = $zip; bytes = (Get-Item $zip).Length; sha256 = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLowerInvariant(); commit = $source.commit } | ConvertTo-Json -Compress

if ($Build -eq 'cuda13') {
    # the captioner's own asset: ggml without the CUDA backend, which it finds beside the trainer when that is installed
    $captioner = "$output-caption"
    if (Test-Path $captioner) { Remove-Item -Recurse -Force $captioner }
    New-Item -ItemType Directory -Force -Path $captioner | Out-Null
    $caption = Join-Path $binDirectory 'ace-caption.exe'
    if (-not (Test-Path $caption)) { throw 'The build completed without ace-caption.exe.' }
    Copy-Item $caption $captioner -Force
    Get-ChildItem -Path $binDirectory -Filter 'ggml*.dll' -File | Where-Object { $_.Name -ne 'ggml-cuda.dll' } | Copy-Item -Destination $captioner -Force
    Copy-Item (Join-Path $worktree "$($source.source_dir)\LICENSE") (Join-Path $captioner 'LICENSE-HOT-Step.txt') -Force
    $captionZip = Join-Path (Split-Path -Parent $output) 'ace-caption-windows-x64.zip'
    if (Test-Path $captionZip) { Remove-Item -Force $captionZip }
    Compress-Archive -Path "$captioner\*" -DestinationPath $captionZip -Force
    [pscustomobject]@{ zip = $captionZip; bytes = (Get-Item $captionZip).Length; sha256 = (Get-FileHash $captionZip -Algorithm SHA256).Hash.ToLowerInvariant(); commit = $source.commit } | ConvertTo-Json -Compress
}
