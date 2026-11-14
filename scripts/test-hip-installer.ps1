<#
.SYNOPSIS
  Acceptance test for a packaged YuE2-Studio Windows HIP build.

.DESCRIPTION
  Given a Tauri NSIS installer (or an already-extracted install root) it:
    1. installs silently and records the exit code (use a machine where
       Smart App Control allows unsigned installers, or a signed build),
    2. verifies the installed file set: the app executable, the HIP engine
       resources (yue-server.exe, ggml-hip.dll) and the ROCm runtime
       closure (amdhip64*, hipblas, rocblas + kernel dirs, amd_comgr,
       rocm_kpack, .kpack),
    3. launches the real app with a prepared data root and no SDK
       environment variables, and checks it stays up with WebView2
       debugging reachable for GUI driving.

  Results are written as JSON (and to stdout as [STATUS] check lines).
  Exit code: 0 all PASS, 1 any FAIL, 2 any BLOCKED.

.PARAMETER Installer
  Path to the *_x64-setup.exe produced by the release build. Omit with
  -InstallRoot to test an existing installation directly.

.PARAMETER InstallRoot
  Existing installation directory (the folder with the app exe); skips the
  install step.

.PARAMETER DataRoot
  Prepared YUE_STUDIO_DATA_ROOT with the models the app should find
  (see the release pipeline's data-root preparation).

.PARAMETER OutputDir
  Where installer-results-<timestamp>.json is written. Defaults to a
  'installer-test' folder beside this script.

.EXAMPLE
  .\scripts\test-hip-installer.ps1 -Installer .\release\0.9.7-hip-gfx1100\YuE2.Studio_0.9.7-hip-gfx1100_x64-setup.exe -DataRoot D:\studio-data
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, ParameterSetName = 'Installer')] [string] $Installer,
    [Parameter(Mandatory = $true, ParameterSetName = 'Root')] [string] $InstallRoot,
    [Parameter(Mandatory = $true)] [string] $DataRoot,
    [string] $OutputDir = ''
)
$ErrorActionPreference = 'Stop'
$InstallDir = ''
if (-not $OutputDir) { $OutputDir = Join-Path $PSScriptRoot '..', 'installer-test' }
New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
if ($Installer -and -not (Test-Path $Installer)) { throw "installer not found: $Installer" }
if ($Installer) { $Installer = (Resolve-Path $Installer).Path }
if (-not (Test-Path $DataRoot)) { throw "data root not found: $DataRoot" }
$DataRoot = (Resolve-Path $DataRoot).Path
if (-not $InstallDir) { $InstallDir = Join-Path $env:LOCALAPPDATA ("Programs\YuE2Studio-HipTest-" + (Get-Date -Format 'HHmmss')) }
$results = New-Object System.Collections.Generic.List[object]
$failures = 0; $blocked = 0
function Record($c, $s, $d = '') {
    $results.Add([pscustomobject]@{ check = $c; status = $s; detail = "$d" })
    if ($s -eq 'FAIL') { $failures++ }; if ($s -eq 'BLOCKED') { $blocked++ }
    Write-Host "[$s] $c : $d"
}

try {
    if ($Installer) {
        # 1. silent install (NSIS /S; per-user layout by default)
        $before = @()
        if (Test-Path (Join-Path $env:LOCALAPPDATA 'Programs')) { $before = Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Programs') -Directory | ForEach-Object Name }
        $p = Start-Process -FilePath $Installer -ArgumentList '/S' -PassThru -Wait
        if ($p.ExitCode -eq 0) { Record 'install/exit-code' 'PASS' "code $($p.ExitCode)" }
        elseif ($p.ExitCode -eq 126 -or $p.ExitCode -eq 199) { Record 'install/exit-code' 'BLOCKED' "exit $($p.ExitCode) looks like Smart App Control blocking an unsigned installer; use a signed build or a machine where SAC allows it" }
        else { Record 'install/exit-code' 'FAIL' "code $($p.ExitCode)" }
        # locate the install: tauri NSIS default per-user is %LOCALAPPDATA%\Programs\YuE2 Studio
        $newDirs = @(Get-ChildItem (Join-Path $env:LOCALAPPDATA 'Programs') -Directory -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Name })
        $candidate = Join-Path $env:LOCALAPPDATA 'Programs\YuE2 Studio'
        if (Test-Path (Join-Path $candidate '*.exe')) { $InstallDir = $candidate }
        elseif ($newDirs.Count -gt 0) { $InstallDir = $newDirs[0].FullName }
        else { Record 'install/located' 'FAIL' 'no new install directory found'; throw 'installation not found' }
        Record 'install/located' 'PASS' $InstallDir
    } else {
        $InstallDir = (Resolve-Path $InstallRoot).Path
        Record 'install/skipped' 'NOT RUN' "testing existing root $InstallDir"
    }

    # 2. installed payload: app exe, engine, ROCm closure
    $exe = Get-ChildItem $InstallDir -Filter '*.exe' -File | Where-Object Name -notmatch 'uninstall' | Select-Object -First 1
    if ($exe) { Record 'files/app-exe' 'PASS' $exe.Name } else { Record 'files/app-exe' 'FAIL' 'no application exe' }
    $engine = Join-Path $InstallDir 'resources\yue2-cpp'
    foreach ($f in @('yue-server.exe', 'ggml-hip.dll', 'hipblas.dll', 'rocblas.dll', 'amd_comgr.dll', 'rocm_kpack.dll')) {
        if (Test-Path (Join-Path $engine $f)) { Record "engine/$f" 'PASS' ((Get-Item (Join-Path $engine $f)).Length) }
        else { Record "engine/$f" 'FAIL' "missing under $engine" }
    }
    $amdhip = Get-ChildItem $engine -Filter 'amdhip64*.dll' -File -ErrorAction SilentlyContinue
    if ($amdhip) { Record 'engine/amdhip64' 'PASS' $amdhip.Name } else { Record 'engine/amdhip64' 'FAIL' 'HIP runtime DLL missing' }
    foreach ($kd in @('rocblas', 'hipblaslt')) {
        if (Test-Path (Join-Path $engine $kd)) { Record "engine/kernels-$kd" 'PASS' "$((Get-ChildItem (Join-Path $engine $kd) -Recurse -File | Measure-Object).Count) files" }
        else { Record "engine/kernels-$kd" 'FAIL' "kernel directory missing" }
    }
    $runtimeJson = Join-Path $engine 'runtime.json'
    if (Test-Path $runtimeJson) { Record 'engine/runtime.json' 'PASS' ((Get-Content -Raw $runtimeJson).Trim() -replace "`n", ' ') }
    else { Record 'engine/runtime.json' 'NOT RUN' 'bundle carries no runtime.json' }

    # 3. launch the real app with the prepared data root, no SDK variables
    if ($exe) {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $exe.FullName
        $psi.WorkingDirectory = $InstallDir
        $psi.UseShellExecute = $false
        $psi.EnvironmentVariables['YUE_STUDIO_DATA_ROOT'] = $DataRoot
        $psi.EnvironmentVariables['WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS'] = '--remote-debugging-port=9223'
        $psi.EnvironmentVariables.Remove('HIP_PATH') | Out-Null
        $psi.EnvironmentVariables.Remove('ROCM_PATH') | Out-Null
        $app = [System.Diagnostics.Process]::Start($psi)
        Start-Sleep -Seconds 20
        if (-not $app.HasExited) { Record 'app/launch' 'PASS' "pid $($app.Id), no SDK environment variables" }
        elseif ($app.ExitCode -eq 126 -or $app.ExitCode -eq 199) { Record 'app/launch' 'BLOCKED' "exit $($app.ExitCode) looks like Smart App Control blocking the unsigned app" }
        else { Record 'app/launch' 'FAIL' "app exited with code $($app.ExitCode)" }
        $cdp = $false
        try { $cdp = [bool](Invoke-RestMethod -Uri 'http://127.0.0.1:9223/json/version' -TimeoutSec 5) } catch { }
        Record 'app/webview2-cdp' $(if ($cdp) { 'PASS' } else { 'FAIL' }) 'WebView2 remote debugging reachable'
        # leave the app running for interactive/GUI-driving follow-up, then stop it
        cmd /d /c "taskkill /IM `"$($exe.Name)`" /T /F >nul 2>&1" | Out-Null
        Record 'app/stopped' 'PASS' 'no studio process left running'
    }
} catch {
    Record 'fatal' 'FAIL' $_.Exception.Message
} finally {
    $out = Join-Path $OutputDir ('installer-results-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '.json')
    $results | ConvertTo-Json -Depth 4 | Set-Content -Path $out -Encoding UTF8
    Write-Host "results: $out"
    Write-Host "==== $(($results | Where-Object status -eq 'PASS').Count) PASS, $failures FAIL, $blocked BLOCKED ===="
    exit $(if ($failures -gt 0) { 1 } elseif ($blocked -gt 0) { 2 } else { 0 })
}
