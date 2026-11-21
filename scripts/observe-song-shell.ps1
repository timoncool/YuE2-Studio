param([string]$Needle, [string]$Receipt, [string]$Expected)
$ErrorActionPreference = 'Stop'
Write-Output "Loading Windows UI Automation"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Write-Output "Opening the desktop automation root"
$desktop = [System.Windows.Automation.AutomationElement]::RootElement
$children = [System.Windows.Automation.TreeScope]::Children
$descendants = [System.Windows.Automation.TreeScope]::Descendants
$any = [System.Windows.Automation.Condition]::TrueCondition
$shellNames = 'explorer', 'ShellExperienceHost', 'StartMenuExperienceHost', 'ShellHost'
$nameProperty = [System.Windows.Automation.AutomationElement]::NameProperty
$contains = [System.Windows.Automation.PropertyCondition]::new($nameProperty, $Needle)
$visible = @{}
$observed = @()
$deadline = [DateTime]::UtcNow.AddSeconds(18)
New-Item -ItemType File -Path "$Receipt.ready" -Force | Out-Null
Write-Output "Observer ready for ${Expected}: $Needle"
do {
  $shells = @(Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Name -in $shellNames } | Select-Object -ExpandProperty Id)
  foreach ($window in $desktop.FindAll($children, $any)) {
    try {
      $windowInfo = $window.Current
      if ($windowInfo.ProcessId -notin $shells) { continue }
      $visible["$($windowInfo.ProcessId)/$($windowInfo.Name)/$($windowInfo.ClassName)"] = $true
      $element = if ($windowInfo.Name -eq $Needle) { $window } else { $window.FindFirst($descendants, $contains) }
      if ($element -and !$element.Current.IsOffscreen) {
        $observed += @{ shellPid=$windowInfo.ProcessId; window=$windowInfo.Name; class=$windowInfo.ClassName; message=$element.Current.Name; offscreen=$element.Current.IsOffscreen }
        break
      }
    } catch [System.Windows.Automation.ElementNotAvailableException] {}
  }
  if ($observed.Count) { break }
  Start-Sleep -Milliseconds 175
} while ([DateTime]::UtcNow -lt $deadline)
@{ expected=$Expected; needle=$Needle; observed=$observed; availableShellWindows=@($visible.Keys) } |
  ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $Receipt -Encoding UTF8
if (($Expected -eq 'visible') -ne [bool]$observed.Count) { exit 1 }
