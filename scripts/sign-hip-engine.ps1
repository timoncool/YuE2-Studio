<#
.SYNOPSIS
  Authenticode-sign only YuE2/GGML-built PE files inside a staged HIP engine.

.DESCRIPTION
  Requires a real, CA-trusted publisher certificate in the Windows cert store
  and Microsoft's signtool.exe. The AMD-supplied ROCm DLLs are deliberately
  left unchanged; their publisher, signatures and license remain AMD's.
  Run BEFORE Tauri packages the application and creates updater signatures.
  Never re-sign an installer after Tauri emits its .sig sidecar.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$EngineRoot,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$Thumbprint,
    [string]$AdditionalProjectRoot = '',
    [string]$SignTool = 'signtool.exe',
    [string]$TimestampUrl = 'http://timestamp.digicert.com'
)
$ErrorActionPreference = 'Stop'

if (-not (Test-Path -LiteralPath $EngineRoot -PathType Container)) {
    throw "HIP engine directory not found: $EngineRoot"
}
$root = (Resolve-Path -LiteralPath $EngineRoot).Path
$tool = Get-Command $SignTool -ErrorAction SilentlyContinue
if (-not $tool) { throw "signtool.exe not found: provide -SignTool or install Windows SDK Signing Tools." }

$engine = Join-Path $root 'yue-server.exe'
$backend = Join-Path $root 'ggml-hip.dll'
foreach ($required in @($engine, $backend)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing HIP engine PE: $required" }
}

$files = @(
    Get-Item -LiteralPath $engine
    Get-ChildItem -LiteralPath $root -Filter 'ggml*.dll' -File
)
if ($files.Count -lt 2) { throw 'HIP engine and GGML DLL set were not found.' }
# The bundled VST host is another first-party executable in the Studio
# portable/install payload. Include it when the publisher stages that host;
# never re-sign AMD/third-party vendor libraries.
if (-not [string]::IsNullOrWhiteSpace($AdditionalProjectRoot)) {
    $vstHost = Join-Path $AdditionalProjectRoot 'vst-host.exe'
    if (-not (Test-Path -LiteralPath $vstHost -PathType Leaf)) {
        throw "First-party VST host is missing from $AdditionalProjectRoot."
    }
    $files += Get-Item -LiteralPath $vstHost
}

foreach ($file in $files) {
    & $tool.Source sign /sha1 $Thumbprint /fd sha256 /tr $TimestampUrl /td sha256 $file.FullName
    if ($LASTEXITCODE -ne 0) { throw "Authenticode signing failed: $($file.Name) ($LASTEXITCODE)" }
    & $tool.Source verify /pa /all $file.FullName
    if ($LASTEXITCODE -ne 0) { throw "Authenticode validation failed: $($file.Name) ($LASTEXITCODE)" }
    Write-Host "[signed-and-verified] $($file.Name)"
}
Write-Host "Signed $($files.Count) first-party engine files. AMD vendor DLLs unchanged."
exit 0
