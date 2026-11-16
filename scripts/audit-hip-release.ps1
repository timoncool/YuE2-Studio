<#
.SYNOPSIS
  No-GPU release gate: HIP asset/update isolation, source stamp and signatures.

.DESCRIPTION
  -RequireTrustedSignatures is for a publisher's release runner, not unsigned CI.
  A Tauri .sig file is checked for presence here; cryptographic Ed25519
  verification additionally requires the configured Tauri updater public key.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ReleaseDir,
    [Parameter(Mandatory = $true)][ValidatePattern('^gfx[0-9a-z]+(,gfx[0-9a-z]+)*$')][string]$HipArch,
    [switch]$RequireTrustedSignatures,
    [string]$ReportPath = ''
)
$ErrorActionPreference = 'Stop'
$results = [System.Collections.Generic.List[object]]::new()
$failed = 0

function Check([string]$name, [bool]$ok, [string]$detail) {
    $status = if ($ok) { 'PASS' } else { 'FAIL' }
    $script:results.Add([pscustomobject]@{ name = $name; status = $status; detail = $detail })
    if (-not $ok) { $script:failed++ }
    Write-Host "[$status] $name - $detail"
}

$releasePath = [System.IO.Path]::GetFullPath($ReleaseDir)
if (-not (Test-Path -LiteralPath $releasePath -PathType Container)) { throw "release folder missing: $releasePath" }
$tag = '-hip-' + ($HipArch -replace ',', '-')
$manifest = Join-Path $releasePath "latest$tag.json"
Check 'manifest/exists' (Test-Path -LiteralPath $manifest -PathType Leaf) $manifest

$installer = $null
if (Test-Path -LiteralPath $manifest -PathType Leaf) {
    $j = Get-Content -Raw -LiteralPath $manifest | ConvertFrom-Json
    $entry = $j.platforms.'windows-x86_64'
    $url = [string]$entry.url
    $name = [System.Uri]::UnescapeDataString(([Uri]$url).Segments[-1])
    Check 'manifest/isolated-asset' ($name.Contains($tag) -and $name.EndsWith('.exe')) $name
    Check 'manifest/tauri-signature-present' (-not [string]::IsNullOrWhiteSpace([string]$entry.signature)) 'Tauri updater signature field'
    $installer = Join-Path $releasePath $name
    Check 'installer/exists' (Test-Path -LiteralPath $installer -PathType Leaf) $installer
    Check 'installer/signature-sidecar' (Test-Path -LiteralPath "$installer.sig" -PathType Leaf) "$installer.sig"
}
$zips = @(Get-ChildItem -LiteralPath $releasePath -File -Filter "*$tag*portable-windows-x64.zip")
Check 'portable/flavor-isolation' ($zips.Count -eq 1) ("count=$($zips.Count)")
$portableDir = Get-ChildItem -LiteralPath $releasePath -Directory -Filter 'YuE2-Studio-*-portable' | Select-Object -First 1
Check 'portable/staged' ($null -ne $portableDir) 'portable directory generated'
$engineDir = if ($portableDir) { Join-Path $portableDir.FullName 'resources\yue2-cpp' } else { $null }
if ($engineDir -and (Test-Path $engineDir)) {
    foreach ($file in @('yue-server.exe','ggml-hip.dll','hipblas.dll','rocblas.dll','amd_comgr.dll')) {
        Check "engine/$file" (Test-Path -LiteralPath (Join-Path $engineDir $file) -PathType Leaf) $file
    }
    $stampFile = Join-Path $engineDir 'runtime.json'
    Check 'engine/runtime-stamp' (Test-Path -LiteralPath $stampFile -PathType Leaf) $stampFile
    if (Test-Path -LiteralPath $stampFile -PathType Leaf) {
        $stamp = Get-Content -LiteralPath $stampFile -Raw | ConvertFrom-Json
        Check 'engine/hip-targets' ($stamp.backend -eq 'hip' -and $stamp.hip_arch -eq $HipArch) "backend=$($stamp.backend); arch=$($stamp.hip_arch)"
    }
} else {
    Check 'engine/staged' $false "No engine bundle: $engineDir"
}

if ($RequireTrustedSignatures) {
    $peFiles = @()
    if ($portableDir) {
        $peFiles += @(Get-ChildItem -LiteralPath $portableDir.FullName -Recurse -File |
            Where-Object { $_.Extension -in @('.exe','.dll') })
    }
    if ($installer -and (Test-Path -LiteralPath $installer -PathType Leaf)) {
        $peFiles += @(Get-Item -LiteralPath $installer)
    }
    Check 'signatures/pe-files-found' ($peFiles.Count -gt 0) "count=$($peFiles.Count)"
    foreach ($pe in $peFiles) {
        $sig = Get-AuthenticodeSignature -LiteralPath $pe.FullName
        Check "signature/$($pe.Name)" ($sig.Status -eq 'Valid') ("$($sig.Status); signer=$($sig.SignerCertificate.Subject)")
    }
    # A Tauri updater .sig is Ed25519, independent of the Authenticode PE
    # signatures. Its presence is not a cryptographic validity assertion.
}

if (-not $ReportPath) { $ReportPath = Join-Path $releasePath 'hip-release-audit.json' }
$report = [pscustomobject]@{
    hip_arch = $HipArch
    trusted_signatures_required = [bool]$RequireTrustedSignatures
    failures = $failed
    checks = @($results.ToArray())
}
$report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $ReportPath -Encoding UTF8
Write-Host "HIP release audit: $ReportPath, $failed failures"
if ($failed -gt 0) { exit 1 }
exit 0
