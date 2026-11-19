$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$root = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location -LiteralPath $root
$log = Join-Path $root 'local-build.log'
Start-Transcript -Path $log -Force | Out-Null
$overridePath = Join-Path $root 'desktop\src-tauri\tauri.local-build.conf.json'
try {
    function Run($exe, [string[]]$arguments) {
        Write-Host "`n> $exe $($arguments -join ' ')" -ForegroundColor Cyan
        & $exe @arguments
        if ($LASTEXITCODE -ne 0) { throw "Command failed (exit $LASTEXITCODE): $exe $($arguments -join ' ')" }
    }
    Write-Host 'YuE2 Studio - LOCAL Windows Portable Builder' -ForegroundColor Green
    Write-Host "Repository: $root"
    foreach ($file in @('app\package-lock.json','desktop\package-lock.json','desktop\src-tauri\tauri.conf.json')) {
        if (-not (Test-Path -LiteralPath (Join-Path $root $file))) { throw "Repository file missing: $file. Put both build scripts in the YuE2-Studio repository root." }
    }
    foreach ($tool in @('node.exe','npm.cmd','cargo.exe','rustc.exe','git.exe')) {
        if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) { throw "Missing command: $tool" }
    }
    $branch = (& git branch --show-current).Trim()
    Write-Host "Current Git branch: $branch" -ForegroundColor Yellow
    if ($branch -ne 'feature/dj-mode-and-download-popup') {
        $reply = Read-Host 'This is NOT the DJ-mode feature branch. Continue anyway? (y/N)'
        if ($reply -notmatch '^[Yy]$') { throw 'Cancelled. Switch to feature/dj-mode-and-download-popup in GitHub Desktop first.' }
    }
    Run 'npm.cmd' @('--prefix','app','ci','--no-audit','--no-fund')
    Run 'npm.cmd' @('--prefix','desktop','ci','--no-audit','--no-fund')
    $resourceRoot = Join-Path $root 'desktop\src-tauri\resources'
    $serverExe = Join-Path $resourceRoot 'yue2-cpp\yue-server.exe'
    $vstRoot = Join-Path $resourceRoot 'vst-host'
    if (-not ((Test-Path -LiteralPath $serverExe) -and (Test-Path -LiteralPath $vstRoot))) {
        Write-Host "`nDownloading upstream native runtime (not compiling CUDA/Vulkan engine)..." -ForegroundColor Yellow
        $headers = @{ 'User-Agent'='YuE2-local-builder'; 'Accept'='application/vnd.github+json' }
        $release = Invoke-RestMethod -Uri 'https://api.github.com/repos/timoncool/YuE2-Studio/releases/latest' -Headers $headers
        $asset = $release.assets | Where-Object { $_.name -match 'portable.*windows.*x64\.zip$' } | Select-Object -First 1
        if (-not $asset) { throw 'Latest upstream release has no portable Windows x64 ZIP. Native engine must be staged manually.' }
        $tempDir = Join-Path $env:TEMP ('yue2-native-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $tempDir -Force | Out-Null
        try {
            $zip = Join-Path $tempDir 'upstream.zip'
            $extract = Join-Path $tempDir 'extracted'
            Write-Host "Downloading $($asset.name) from upstream release $($release.tag_name)..."
            Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip
            Expand-Archive -LiteralPath $zip -DestinationPath $extract -Force
            foreach ($name in @('yue2-cpp','vst-host')) {
                $source = Get-ChildItem -LiteralPath $extract -Directory -Recurse | Where-Object { $_.Name -eq $name -and $_.Parent.Name -eq 'resources' } | Select-Object -First 1
                if (-not $source) { throw "Upstream portable archive is missing resources/$name" }
                $dest = Join-Path $resourceRoot $name
                New-Item -ItemType Directory -Path $resourceRoot -Force | Out-Null
                if (Test-Path -LiteralPath $dest) { Remove-Item -LiteralPath $dest -Recurse -Force }
                Copy-Item -LiteralPath $source.FullName -Destination $dest -Recurse -Force
            }
        } finally {
            if (Test-Path -LiteralPath $tempDir) { Remove-Item -LiteralPath $tempDir -Recurse -Force -ErrorAction SilentlyContinue }
        }
    } else {
        Write-Host 'Existing native runtime found; skipping upstream download.' -ForegroundColor Green
    }
    if (-not (Test-Path -LiteralPath $serverExe)) { throw "Native server missing: $serverExe" }
    if (-not (Test-Path -LiteralPath $vstRoot)) { throw "VST host directory missing: $vstRoot" }
    Run 'npm.cmd' @('--prefix','app','run','test')
    Run 'npm.cmd' @('--prefix','app','run','build')
    $override = @{
        bundle = @{
            createUpdaterArtifacts = $false
            resources = @{
                'resources/yue2-cpp' = 'resources/yue2-cpp'
                'resources/vst-host' = 'resources/vst-host'
            }
        }
        plugins = @{ updater = @{ endpoints = @() } }
    }
    $override | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $overridePath -Encoding utf8
    Push-Location (Join-Path $root 'desktop')
    try { Run 'npx.cmd' @('tauri','build','--no-bundle','--config',$overridePath) }
    finally { Pop-Location }
    $config = Get-Content -LiteralPath (Join-Path $root 'desktop\src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
    $version = [string]$config.version
    $binary = [string]$config.mainBinaryName
    if (-not $binary) { throw 'mainBinaryName is missing from desktop/src-tauri/tauri.conf.json' }
    $exe = Join-Path $root "desktop\src-tauri\target\release\$binary.exe"
    if (-not (Test-Path -LiteralPath $exe)) { throw "Compiled desktop EXE not found: $exe" }
    $dist = Join-Path $root 'LOCAL-BUILDS'
    $portableName = "YuE2-Studio-$version-portable-windows-x64"
    $stage = Join-Path $dist $portableName
    $archive = Join-Path $dist "$portableName.zip"
    New-Item -ItemType Directory -Path $dist -Force | Out-Null
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force }
    New-Item -ItemType Directory -Path (Join-Path $stage 'resources') -Force | Out-Null
    Copy-Item -LiteralPath $exe -Destination $stage
    foreach ($name in @('yue2-cpp','vst-host')) {
        Copy-Item -LiteralPath (Join-Path $resourceRoot $name) -Destination (Join-Path $stage 'resources') -Recurse -Force
    }
    New-Item -ItemType File -Path (Join-Path $stage 'portable.flag') -Force | Out-Null
    if (Test-Path -LiteralPath $archive) { Remove-Item -LiteralPath $archive -Force }
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $archive -CompressionLevel Optimal
    Write-Host "`nBUILD COMPLETE!" -ForegroundColor Green
    Write-Host "Portable folder: $stage" -ForegroundColor Green
    Write-Host "Portable ZIP:    $archive" -ForegroundColor Green
    Write-Host 'Run the EXE inside the portable folder to test it.'
    Start-Process explorer.exe -ArgumentList "`"$dist`""
} catch {
    Write-Host "`nBUILD FAILED: $($_.Exception.Message)" -ForegroundColor Red
    Write-Host "Log: $log" -ForegroundColor Yellow
} finally {
    Remove-Item -LiteralPath $overridePath -Force -ErrorAction SilentlyContinue
    Stop-Transcript | Out-Null
    Read-Host 'Press Enter to close this window'
}
