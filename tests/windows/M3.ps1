$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '../..')
cargo build --release --features fault-injection,test-hooks
if ($LASTEXITCODE -ne 0) { throw 'Build failed' }
cargo test -p baauto-win-platform --features fault-injection,test-hooks --test windows_faults -- --ignored --nocapture --test-threads=1
if ($LASTEXITCODE -ne 0) { throw 'Fault acceptance failed; keep the temporary evidence folders printed by tests' }
Write-Host 'Remaining hardware-only checks: unhook@2 requires physical bound-key input (SendInput does not produce hardware Raw Input). Run receiver and headless app with --inject unhook@2, hold z, then physically press/release a bound key after hooks are removed. Expect Faulted, a fault CSV/JSON, LeftUp, and no automatic Arm.'
Write-Host 'Also check: kill app via Task Manager; Ctrl+F9 stop latency; 100 Arm/stop cycles handle counts; profile/keys/settings switching; emergency hotkey registration conflict.'
