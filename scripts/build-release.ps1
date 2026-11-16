param(
    [ValidatePattern('^\d+\.\d+\.\d+([\-+][0-9A-Za-z.-]+)?$')]
    [string]$Version,
    [string]$ReleaseNotes = "",
    [ValidateSet('auto', 'cuda', 'hip', 'vulkan', 'all')]
    [string]$RuntimeBackend = 'all',
    # The CUDA 12 toolkit of the engine's second CUDA backend.
    [string]$Cuda12Root = $env:CUDA_PATH_V12_9,
    # gfx target(s) of the opt-in HIP engine: one id or a comma list. Only
    # read for -RuntimeBackend hip; every artifact of that flavor is named
    # after it so a HIP release can never overwrite the default Windows one.
    [string]$HipArch = $(if ($env:HIP_ARCHS) { $env:HIP_ARCHS } else { 'gfx1100' })
)

$ErrorActionPreference = 'Stop'

# The version is written in desktop/src-tauri/tauri.conf.json and nowhere else.
$configuredVersion = (Get-Content -Raw (Join-Path $PSScriptRoot '..\desktop\src-tauri\tauri.conf.json') | ConvertFrom-Json).version
if (-not $Version) { $Version = $configuredVersion }
elseif ($Version -ne $configuredVersion) { throw "desktop/src-tauri/tauri.conf.json says $configuredVersion, not $($Version): change the version there" }

$whereTheKeyIs = @'
The updater signing key lives on the release machine:
  %USERPROFILE%\.tauri\yue2-studio.key, its .pub, and its password in
  yue2-studio.key.password beside them. This script reads all three.
Set TAURI_SIGNING_PRIVATE_KEY / TAURI_SIGNING_PRIVATE_KEY_PASSWORD /
TAURI_UPDATER_PUBKEY to override (CI). The public half must match
desktop/src-tauri/tauri.conf.json, or every update is rejected. Losing the
private key ends automatic updates for everyone already running the studio.
'@

$keyFile = Join-Path $env:USERPROFILE '.tauri\yue2-studio.key'
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and (Test-Path $keyFile)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -Raw $keyFile).Trim()
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_UPDATER_PUBKEY) -and (Test-Path "$keyFile.pub")) {
    $env:TAURI_UPDATER_PUBKEY = (Get-Content -Raw "$keyFile.pub").Trim()
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) -and (Test-Path "$keyFile.password")) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -Raw "$keyFile.password").Trim()
}
foreach ($name in 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_UPDATER_PUBKEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD') {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) {
        # An encrypted key with no password does not fail, it hangs on stdin.
        throw "$name is empty.`n`n$whereTheKeyIs"
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $repoRoot 'desktop'
$tauriRoot = Join-Path $desktopRoot 'src-tauri'
$templatePath = Join-Path $tauriRoot 'tauri.release.conf.template.json'
$releaseConfigPath = Join-Path $tauriRoot 'tauri.release.conf.json'
# The default Windows package stays exactly what it was; a HIP flavor lands
# in its own folder so neither can overwrite the other's files.
$flavorTag = if ($RuntimeBackend -eq 'hip') { '-hip-' + ($HipArch -replace ',', '-') } else { '' }
$manifestName = if ($flavorTag) { "latest$flavorTag.json" } else { 'latest.json' }
$authenticodeThumbprint = $env:YUE_AUTHENTICODE_CERT_THUMBPRINT
if ($RuntimeBackend -eq 'hip' -and $env:YUE_REQUIRE_SIGNED_RELEASE -eq '1' -and [string]::IsNullOrWhiteSpace($authenticodeThumbprint)) {
    throw 'Signed public HIP releases require YUE_AUTHENTICODE_CERT_THUMBPRINT and a trusted code-signing certificate; unsigned CI builds remain permitted.'
}
$releaseDir = Join-Path $repoRoot "release\$Version$flavorTag"
$engineResourceRoot = Join-Path $tauriRoot 'resources\yue2-cpp'
$vstResourceRoot = Join-Path $tauriRoot 'resources\vst-host'
$engineSource = Get-Content -Raw (Join-Path $repoRoot 'engines\yue2-cpp-source.json') | ConvertFrom-Json

Push-Location $repoRoot
try {
    # Native tools write progress to stderr; exit codes are the verdict.
    $ErrorActionPreference = 'Continue'

    # The engine is rebuilt only when the staged one is not the pinned commit
    # for this backend: a CUDA build of ggml is the slow part of a release.
    $stampPath = Join-Path $engineResourceRoot 'runtime.json'
    $stamp = if (Test-Path $stampPath) { Get-Content -Raw $stampPath | ConvertFrom-Json } else { $null }
    $stagedIsCurrent = $stamp -and $stamp.commit -eq $engineSource.commit -and $stamp.backend -eq $RuntimeBackend -and $stamp.cuda_architecture -eq 'universal' -and ($RuntimeBackend -ne 'all' -or @($stamp.cuda_builds).Count -eq 2) -and ($RuntimeBackend -ne 'hip' -or $stamp.hip_arch -eq $HipArch) -and (Test-Path (Join-Path $engineResourceRoot 'yue-server.exe')) -and ($RuntimeBackend -ne 'hip' -or ((Test-Path (Join-Path $engineResourceRoot 'ggml-hip.dll')) -and (Test-Path (Join-Path $engineResourceRoot 'amd_comgr.dll'))))
    if (-not $stagedIsCurrent) {
        & (Join-Path $PSScriptRoot 'build-yue-runtime.ps1') -OutputDirectory $engineResourceRoot -RuntimeBackend $RuntimeBackend -CudaArchitecture universal -Cuda12Root $Cuda12Root -HipArch $HipArch
        if (-not $?) { throw "The nested build script failed; release packaging stopped." }
        if ($LASTEXITCODE -ne 0) { throw "the engine runtime build failed with exit code $LASTEXITCODE" }
    }

    # The VST host follows the trainer's HOT-Step commit; rebuilt only when that moves.
    $trainSource = Get-Content -Raw (Join-Path $repoRoot 'engines\music-train-source.json') | ConvertFrom-Json
    $vstStampPath = Join-Path $vstResourceRoot 'runtime.json'
    $vstStamp = if (Test-Path $vstStampPath) { Get-Content -Raw $vstStampPath | ConvertFrom-Json } else { $null }
    if (-not ($vstStamp -and $vstStamp.commit -eq $trainSource.commit -and (Test-Path (Join-Path $vstResourceRoot 'vst-host.exe')))) {
        & (Join-Path $PSScriptRoot 'build-vst-host.ps1') -OutputDirectory $vstResourceRoot
        if (-not $?) { throw "The nested build script failed; release packaging stopped." }
        if ($LASTEXITCODE -ne 0) { throw "the VST host build failed with exit code $LASTEXITCODE" }
    }

    # After the engine: one test reads the staged bundle's import tables and
    # fails if it names a library neither shipped nor downloaded on first start.
    cargo test --workspace
    if ($LASTEXITCODE -ne 0) { throw "cargo test failed with exit code $LASTEXITCODE" }
    npm --prefix (Join-Path $repoRoot 'app') run test
    if ($LASTEXITCODE -ne 0) { throw "the interface tests failed with exit code $LASTEXITCODE" }

    $config = Get-Content -Raw $templatePath
    $config = $config.Replace('__TAURI_UPDATER_PUBKEY__', $env:TAURI_UPDATER_PUBKEY)
    $config = $config.Replace('__RELEASE_VERSION__', $Version)
    # The shipped Tauri updater endpoint must match the HIP-specific manifest.
    $defaultUpdater = 'https://github.com/timoncool/YuE2-Studio/releases/latest/download/latest.json'
    if ($flavorTag) {
        if (-not $config.Contains($defaultUpdater)) { throw 'Expected default updater endpoint was not found in the release template.' }
        $config = $config.Replace($defaultUpdater, "https://github.com/timoncool/YuE2-Studio/releases/latest/download/$manifestName")
    }
    if (-not $config.Contains("/$manifestName")) { throw "Release config does not point to $manifestName." }

    # Sign first-party PE files before Tauri bundles them; Tauri itself signs
    # the app and NSIS installer before producing the updater Ed25519 .sig.
    # Never alter/re-sign an installer after its updater signature is created.
    if ($RuntimeBackend -eq 'hip' -and -not [string]::IsNullOrWhiteSpace($authenticodeThumbprint)) {
        & (Join-Path $PSScriptRoot 'sign-hip-engine.ps1') -EngineRoot $engineResourceRoot -AdditionalProjectRoot $vstResourceRoot -Thumbprint $authenticodeThumbprint
        if (-not $?) { throw 'Signing the staged HIP engine failed.' }
        if ($LASTEXITCODE -ne 0) { throw "HIP engine signing exited with $LASTEXITCODE." }
    }
    if (-not [string]::IsNullOrWhiteSpace($authenticodeThumbprint)) {
        if ($authenticodeThumbprint -notmatch '^[0-9a-fA-F]{40}$') {
            throw 'YUE_AUTHENTICODE_CERT_THUMBPRINT must be a 40-character SHA1 certificate thumbprint.'
        }
        $configObject = $config | ConvertFrom-Json
        $configObject.bundle | Add-Member -NotePropertyName windows -NotePropertyValue ([pscustomobject]@{
            certificateThumbprint = $authenticodeThumbprint
            digestAlgorithm = 'sha256'
            timestampUrl = 'http://timestamp.digicert.com'
        }) -Force
        $config = $configObject | ConvertTo-Json -Depth 32
    }
    [System.IO.File]::WriteAllText($releaseConfigPath, $config, (New-Object System.Text.UTF8Encoding($false)))

    Push-Location $desktopRoot
    try {
        npm exec tauri build -- --config $releaseConfigPath --bundles nsis
        if ($LASTEXITCODE -ne 0) { throw "tauri build failed with exit code $LASTEXITCODE" }
    }
    finally {
        Pop-Location
    }

    New-Item -ItemType Directory -Force -Path $releaseDir | Out-Null
    $bundleRoot = Join-Path $tauriRoot 'target\release\bundle'
    $nsisInstaller = Get-ChildItem -Recurse -File (Join-Path $bundleRoot 'nsis') -Filter "*_$($Version)_x64-setup.exe" | Select-Object -First 1
    if (-not $nsisInstaller) { throw "the NSIS installer for $Version is missing" }
    $signaturePath = "$($nsisInstaller.FullName).sig"
    if (-not (Test-Path $signaturePath)) { throw "the updater signature is missing: $signaturePath" }
    $signature = (Get-Content -Raw $signaturePath).Trim()

    # GitHub replaces spaces in asset names with dots; the manifest must name
    # the asset as it will be served. A HIP flavor also carries its tag, so
    # its installer can be uploaded beside the default one, never over it.
    # (Literal .Replace: a regex here would treat the version's dots as
    # wildcards and mangle the name.)
    $assetName = $nsisInstaller.Name -replace ' ', '.'
    if ($flavorTag) { $assetName = $assetName.Replace("_${Version}_", "_$Version$flavorTag`_") }
    Copy-Item $nsisInstaller.FullName (Join-Path $releaseDir $assetName) -Force
    Copy-Item $signaturePath (Join-Path $releaseDir "$assetName.sig") -Force

    # Portable: the same executable, the engine beside it, and the marker that
    # keeps every byte of data inside the folder.
    $binaryName = (Get-Content -Raw (Join-Path $tauriRoot 'tauri.conf.json') | ConvertFrom-Json).mainBinaryName
    $portableRoot = Join-Path $releaseDir "YuE2-Studio-$Version-portable"
    if (Test-Path $portableRoot) { Remove-Item -Recurse -Force $portableRoot }
    New-Item -ItemType Directory -Force -Path $portableRoot | Out-Null
    Copy-Item (Join-Path $tauriRoot "target\release\$binaryName.exe") (Join-Path $portableRoot "$binaryName.exe") -Force
    New-Item -ItemType Directory -Force -Path (Join-Path $portableRoot 'resources') | Out-Null
    Copy-Item $engineResourceRoot (Join-Path $portableRoot 'resources\yue2-cpp') -Recurse -Force
    Get-ChildItem (Join-Path $portableRoot 'resources\yue2-cpp') -Filter '*.pdb' | Remove-Item -Force
    Copy-Item $vstResourceRoot (Join-Path $portableRoot 'resources\vst-host') -Recurse -Force
    New-Item -ItemType File -Path (Join-Path $portableRoot 'portable.flag') -Force | Out-Null
    $portableZip = Join-Path $releaseDir "YuE2-Studio-$Version$flavorTag-portable-windows-x64.zip"
    if (Test-Path $portableZip) { Remove-Item -Force $portableZip }
    Compress-Archive -Path "$portableRoot\*" -DestinationPath $portableZip -Force

    $latest = [ordered]@{
        version = $Version
        notes = $ReleaseNotes
        pub_date = (Get-Date).ToUniversalTime().ToString('o')
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{
                signature = $signature
                url = "https://github.com/timoncool/YuE2-Studio/releases/download/v$Version/$assetName"
            }
        }
    }
    # A byte-order mark in front of the JSON makes the updater reject it. The
    # HIP flavor writes a tag-named manifest: publishing it can never replace
    # the default channel's latest.json, and default-channel publishing can
    # never serve a HIP installer.
    [System.IO.File]::WriteAllText(
        (Join-Path $releaseDir $manifestName),
        ($latest | ConvertTo-Json -Depth 8),
        (New-Object System.Text.UTF8Encoding($false))
    )
    if ($RuntimeBackend -eq 'hip') {
        & (Join-Path $PSScriptRoot 'audit-hip-release.ps1') -ReleaseDir $releaseDir -HipArch $HipArch -RequireTrustedSignatures:($env:YUE_REQUIRE_SIGNED_RELEASE -eq '1')
        if (-not $?) { throw 'HIP release audit failed.' }
        if ($LASTEXITCODE -ne 0) { throw "HIP release audit exited with $LASTEXITCODE." }
    }
    Get-ChildItem $releaseDir -File | Select-Object Name, Length | Format-Table | Out-Host
}
catch {
    Write-Error $_ -ErrorAction Continue
    exit 1
}
finally {
    Remove-Item -LiteralPath $releaseConfigPath -Force -ErrorAction SilentlyContinue
    Pop-Location
}
