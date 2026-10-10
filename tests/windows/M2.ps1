param([ValidateSet('six','ten')] [string]$Mode = 'six', [int]$Seconds = 180, [int]$MinActions = 1)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '../..')
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
New-Item -ItemType Directory -Force logs | Out-Null
$side = if ($Mode -eq 'six') { 'right' } else { 'left' }
$points = if ($Mode -eq 'six') { 'tests/fixtures/m2-basic.json' } else { 'tests/fixtures/m2-ten-left.json' }
$engineArgs = @('--standalone','--points',$points,'--keys','tests/fixtures/m2-keys.json','--target-exe','baauto-receiver.exe','--max-run-secs',"$Seconds",'--dump-ring',"logs/m2-$Mode-ring.csv")
if ($Mode -eq 'ten') { $engineArgs += @('--settings','tests/fixtures/m2-left-hand.json') }
$receiver = Start-Process 'target/release/baauto-receiver.exe' -ArgumentList @('--hand',$Mode,'--side',$side,'--output',"logs/m2-$Mode-receiver.csv") -PassThru
Write-Host 'Switch to receiver; try W, 2, Down, hold z, q in every slot and outside; Alt+Tab while holding z. Return and release z. Do not restart actions after focus-loss: this standalone session is invalidated.'
Write-Host "Run separate sessions for focus-loss and 1000-action measurements. Session ends after $Seconds seconds."
$engine = Start-Process 'target/release/baauto-engine.exe' -ArgumentList $engineArgs -PassThru -Wait -NoNewWindow
if ($engine.ExitCode -ne 0) { throw 'Engine failed; preserve logs for investigation' }
$receiver.CloseMainWindow() | Out-Null
if (!$receiver.WaitForExit(10000)) { throw 'Close receiver with Alt+F4 to flush CSV, then inspect logs' }
$rows = Import-Csv "logs/m2-$Mode-ring.csv"
$outputs = @($rows | Where-Object { $_.kind -eq '2' -and [long]$_.send_end -gt 0 })
if ($outputs.Count -lt $MinActions) { throw 'Insufficient interactions; repeat the test' }
if (@($outputs | Where-Object { $_.inserted -ne $_.expected }).Count -gt 0) { throw 'Short send' }
if (@($rows | Where-Object kind -eq '4').Count -ne 1) { throw 'Missing session stop acknowledgement' }
Write-Host "Recorded $($outputs.Count) output batches. Verify receiver Down/Up pairing and foreground behavior manually. QPC latency analysis is in M5; no measurements are fabricated by this script."
