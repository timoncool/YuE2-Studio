param([ValidateSet('control','collect')][string]$Phase)
$ErrorActionPreference = 'Stop'
$evidence = Join-Path $env:QA_DIR 'evidence/print-control'
$directory = Join-Path $env:ProgramData 'StudioHostedPrintQA'
$destination = Join-Path $directory 'current.pdf'
$printer = 'Studio QA PDF'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
if ($Phase -eq 'control') {
    [DateTime]::UtcNow.ToString('o') | Set-Content "$evidence/start.txt"
    Start-Service Spooler
    wevtutil sl Microsoft-Windows-PrintService/Operational /e:true
    if ($LASTEXITCODE) { throw 'Could not enable PrintService Operational' }
    New-Item -ItemType Directory -Path $directory | Out-Null
    $driver = 'Microsoft Print To PDF'
    if (!(Get-PrinterDriver -Name $driver -ErrorAction SilentlyContinue)) { Add-PrinterDriver -Name $driver }
    Add-PrinterPort -Name $destination
    Add-Printer -Name $printer -PortName $destination -DriverName $driver
    Set-Printer -Name $printer -KeepPrintedJobs $true
    Set-PrintConfiguration -PrinterName $printer -PaperSize A4
    (New-Object -ComObject WScript.Network).SetDefaultPrinter($printer)
    Get-CimInstance Win32_Printer | Where-Object Name -eq $printer | Select-Object Name,Default,DriverName,PortName |
        ConvertTo-Json | Set-Content "$evidence/printer.json"
    Add-Type -AssemblyName System.Drawing
    $doc = [System.Drawing.Printing.PrintDocument]::new()
    $font = [System.Drawing.Font]::new('Arial', 24)
    $doc.DocumentName = 'Studio QA Windows control'
    $doc.PrinterSettings.PrinterName = $printer
    $doc.PrintController = [System.Drawing.Printing.StandardPrintController]::new()
    $doc.add_PrintPage({param($sender,$page) $page.Graphics.DrawString('Studio QA Windows control', $font, [System.Drawing.Brushes]::Black, 80, 80)})
    try { $doc.Print() } finally { $font.Dispose(); $doc.Dispose() }
    for ($i = 0; $i -lt 40; $i++) {
        if ((Test-Path -LiteralPath $destination) -and (Get-Content -LiteralPath $destination -Tail 1 -ErrorAction SilentlyContinue) -eq '%%EOF') {
            Move-Item -LiteralPath $destination -Destination "$evidence/windows-control.pdf"
            break
        }
        Start-Sleep -Milliseconds 500
    }
    @(Get-PrintJob -PrinterName $printer | Select-Object Id,DocumentName,JobStatus,SubmittedTime,TotalPages) |
        ConvertTo-Json -AsArray | Set-Content "$evidence/queue-after-control.json"
    "STUDIO_QA_FILE_PORT=$destination" >> $env:GITHUB_ENV
    if (!(Test-Path "$evidence/windows-control.pdf")) { throw 'Windows control produced no complete PDF' }
} else {
    if (!(Test-Path "$evidence/start.txt")) { exit 0 }
    if (Get-Printer -Name $printer -ErrorAction SilentlyContinue) {
        @(Get-PrintJob -PrinterName $printer | Select-Object Id,DocumentName,JobStatus,SubmittedTime,TotalPages) |
            ConvertTo-Json -AsArray | Set-Content "$evidence/queue-after-songbook.json"
    }
    $start = [datetime](Get-Content "$evidence/start.txt")
    foreach ($kind in 'Admin','Operational') {
        $channel = "Microsoft-Windows-PrintService/$kind"
        @(Get-WinEvent -FilterHashtable @{ LogName=$channel; StartTime=$start } -ErrorAction SilentlyContinue |
            Select-Object TimeCreated, Id, LevelDisplayName, Message, @{n='Xml';e={$_.ToXml()}}) |
            ConvertTo-Json -AsArray -Depth 4 | Set-Content "$evidence/printservice-$kind.json"
        wevtutil epl $channel "$evidence/printservice-$kind.evtx"
    }
    if (Test-Path -LiteralPath $destination) { Copy-Item -LiteralPath $destination -Destination "$evidence/file-port-output.pdf" }
}
