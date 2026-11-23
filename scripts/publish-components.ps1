<#
Publish the component catalogue: assemble docs/components/components.json from
distribution/components-src/*.json, check every new or changed file address
(size and SHA-256 of the bytes actually served), raise the sequence, sign it with
the studio's updater key and verify the signature.

  scripts/publish-components.ps1            # check, sign, leave the result for a commit
  scripts/publish-components.ps1 -SkipFetch # sign without downloading (data unchanged)

The key is the one build-release.ps1 uses: %USERPROFILE%\.tauri\yue2-studio.key and
its .password beside it, or TAURI_SIGNING_PRIVATE_KEY(_PASSWORD).
#>
param([switch]$SkipFetch)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$catalogue = Join-Path $root 'docs\components\components.json'

$keyFile = Join-Path $env:USERPROFILE '.tauri\yue2-studio.key'
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and (Test-Path $keyFile)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -Raw $keyFile).Trim()
}
if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) -and (Test-Path "$keyFile.password")) {
    $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -Raw "$keyFile.password").Trim()
}
foreach ($name in 'TAURI_SIGNING_PRIVATE_KEY', 'TAURI_SIGNING_PRIVATE_KEY_PASSWORD') {
    if ([string]::IsNullOrWhiteSpace([Environment]::GetEnvironmentVariable($name))) { throw "[ERROR] $name is empty" }
}

$published = git -C $root ls-tree --name-only HEAD -- docs/components/components.json
$previous = if ($published) { git -C $root show "HEAD:docs/components/components.json" | Out-String } else { '' }
python (Join-Path $root 'distribution\components-src\build_catalogue.py')
if ($LASTEXITCODE -ne 0) { throw '[ERROR] the catalogue tables do not assemble' }

$data = Get-Content -Raw -Encoding utf8 $catalogue | ConvertFrom-Json
$old = if ($previous.Trim()) { $previous | ConvertFrom-Json } else { $null }
if (-not $SkipFetch) {
    foreach ($property in $data.files.PSObject.Properties) {
        $file = $property.Value
        $before = if ($old) { $old.files.($property.Name) } else { $null }
        if ($before -and $before.url -eq $file.url -and $before.sha256 -eq $file.sha256 -and $before.bytes -eq $file.bytes) { continue }
        Write-Output "[..] checking $($property.Name)"
        $temp = New-TemporaryFile
        try {
            curl.exe -fsSL --retry 3 -o $temp.FullName $file.url
            if ($LASTEXITCODE -ne 0) { throw "[ERROR] $($property.Name): $($file.url) is not reachable" }
            $length = (Get-Item $temp.FullName).Length
            $sha = (Get-FileHash -Algorithm SHA256 $temp.FullName).Hash.ToLowerInvariant()
            if ($length -ne $file.bytes -or $sha -ne $file.sha256) {
                throw "[ERROR] $($property.Name): served $length bytes, SHA $sha; the table says $($file.bytes), $($file.sha256)"
            }
        } finally { Remove-Item -LiteralPath $temp.FullName -Force }
    }
}

$sequence = if ($old) { [int64]$old.sequence + 1 } else { 1 }
python -c "import json,sys,datetime;p=sys.argv[1];d=json.load(open(p,encoding='utf-8'));d['sequence']=int(sys.argv[2]);d['published']=datetime.date.today().isoformat();open(p,'w',encoding='utf-8').write(json.dumps(d,ensure_ascii=False,indent=1)+'\n')" $catalogue $sequence
if ($LASTEXITCODE -ne 0) { throw '[ERROR] could not stamp the sequence' }

Remove-Item -LiteralPath "$catalogue.sig" -ErrorAction SilentlyContinue
Push-Location (Join-Path $root 'desktop')
try { npx tauri signer sign $catalogue | Out-Null } finally { Pop-Location }
if (-not (Test-Path "$catalogue.sig")) { throw '[ERROR] no signature was written' }

Push-Location (Join-Path $root 'crates\music-server')
try { cargo test --lib components:: --quiet } finally { Pop-Location }
if ($LASTEXITCODE -ne 0) { throw '[ERROR] the signed catalogue does not verify' }
Write-Output "[OK] components.json sequence $sequence signed; commit docs/components and push"
