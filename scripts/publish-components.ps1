<#
Publish the component catalogue: assemble docs/components/1/components.json from
distribution/components-src/*.json, check every new or changed file address
(size and SHA-256 of the bytes actually served), raise the sequence above both the
committed and the published catalogue, sign it with the studio's updater key and verify it.
The pair in docs/ is replaced only once the new one is signed and verified.

  scripts/publish-components.ps1            # check, sign, leave the result for a commit
  scripts/publish-components.ps1 -SkipFetch # sign without downloading (data unchanged)

The site serves docs/ of main: the catalogue reaches studios when the commit is on main.
The key is the one build-release.ps1 uses: %USERPROFILE%\.tauri\yue2-studio.key and
its .password beside it, or TAURI_SIGNING_PRIVATE_KEY(_PASSWORD).
#>
param([switch]$SkipFetch)
$ErrorActionPreference = 'Stop'
# Windows PowerShell turns a native program's stderr into a terminating error under Stop; its exit code decides instead.
function Invoke-Native([string]$Failure, [scriptblock]$Command) {
    $ErrorActionPreference = 'Continue'
    & $Command 2>&1 | ForEach-Object { Write-Host "$_" }
    if ($LASTEXITCODE -ne 0) { throw "[ERROR] $Failure" }
}

$root = Split-Path -Parent $PSScriptRoot
$relative = 'docs/components/1/components.json'
$catalogue = Join-Path $root $relative
$publishedUrl = 'https://timoncool.github.io/YuE2-Studio/components/1/components.json'

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

$committed = git -C $root ls-tree --name-only HEAD -- $relative
$old = if ($committed) { git -C $root show "HEAD:$relative" | Out-String | ConvertFrom-Json } else { $null }
$live = $null
try { $live = Invoke-RestMethod -Uri $publishedUrl -Headers @{ 'Cache-Control' = 'no-cache' } -TimeoutSec 30 } catch {
    if ($_.Exception.Response.StatusCode.value__ -ne 404) { throw "[ERROR] the published catalogue could not be read: $($_.Exception.Message)" }
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("components-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $next = Join-Path $work 'components.json'
    Invoke-Native 'the catalogue tables do not assemble' { python (Join-Path $root 'distribution\components-src\build_catalogue.py') $next }

    $data = Get-Content -Raw -Encoding utf8 $next | ConvertFrom-Json
    if (-not $SkipFetch) {
        foreach ($property in $data.files.PSObject.Properties) {
            $file = $property.Value
            $before = if ($old) { $old.files.($property.Name) } else { $null }
            if ($before -and $before.url -eq $file.url -and $before.sha256 -eq $file.sha256 -and $before.bytes -eq $file.bytes) { continue }
            Write-Output "[..] checking $($property.Name)"
            $temp = Join-Path $work 'download'
            try {
                Invoke-Native "$($property.Name): $($file.url) is not reachable" { curl.exe -fsSL --retry 3 -o $temp $file.url }
                $length = (Get-Item $temp).Length
                $sha = (Get-FileHash -Algorithm SHA256 $temp).Hash.ToLowerInvariant()
                if ($length -ne $file.bytes -or $sha -ne $file.sha256) {
                    throw "[ERROR] $($property.Name): served $length bytes, SHA $sha; the table says $($file.bytes), $($file.sha256)"
                }
            } finally { Remove-Item -LiteralPath $temp -Force -ErrorAction SilentlyContinue }
        }
    }

    $sequence = 1 + (@(0, $(if ($old) { [int64]$old.sequence } else { 0 }), $(if ($live) { [int64]$live.sequence } else { 0 })) | Measure-Object -Maximum).Maximum
    Invoke-Native 'could not stamp the sequence' { python -c "import json,sys,datetime;p=sys.argv[1];d=json.load(open(p,encoding='utf-8'));d['sequence']=int(sys.argv[2]);d['published']=datetime.date.today().isoformat();open(p,'w',encoding='utf-8',newline='\n').write(json.dumps(d,ensure_ascii=False,indent=1)+'\n')" $next $sequence }

    Push-Location (Join-Path $root 'desktop')
    try { Invoke-Native 'signing failed' { npx tauri signer sign $next } } finally { Pop-Location }
    if (-not (Test-Path "$next.sig")) { throw '[ERROR] no signature was written' }

    $backup = Join-Path $work 'previous'
    New-Item -ItemType Directory -Path $backup | Out-Null
    foreach ($name in 'components.json', 'components.json.sig') {
        $current = Join-Path (Split-Path -Parent $catalogue) $name
        if (Test-Path $current) { Copy-Item -LiteralPath $current -Destination $backup }
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $catalogue) | Out-Null
    Copy-Item -LiteralPath $next, "$next.sig" -Destination (Split-Path -Parent $catalogue) -Force
    Push-Location (Join-Path $root 'crates\music-server')
    try { Invoke-Native 'the signed catalogue does not verify' { cargo test --lib components:: --quiet } }
    catch {
        Get-ChildItem -LiteralPath $backup | Copy-Item -Destination (Split-Path -Parent $catalogue) -Force
        throw
    }
    finally { Pop-Location }
} finally { Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue }
Write-Output "[OK] components.json sequence $sequence signed; commit docs/components, and it is live once on main"
