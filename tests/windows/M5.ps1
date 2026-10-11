param([string]$PresentMon = '', [switch]$Interactive)
$ErrorActionPreference='Stop'
function Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed: $LASTEXITCODE" }
}
Checked 'cargo' @('fmt','--check')
Checked 'cargo' @('clippy','--workspace','--all-targets','--','-D','warnings')
Checked 'cargo' @('test','--workspace')
Checked 'cargo' @('build','--release','--workspace','--features','bench-experiments,test-hooks')
if (-not $Interactive) { Write-Host 'Interactive DXGI/receiver/PresentMon checks remain pending; use -Interactive.'; exit }
$out=Join-Path $env:TEMP ('baauto-m5-'+[guid]::NewGuid())
New-Item -ItemType Directory $out | Out-Null
$receiver=Start-Process './target/release/baauto-receiver.exe' -ArgumentList '--output',"$out/receiver.csv" -PassThru
try {
    Write-Host 'Focus receiver, then press W when capture test has baseline.'
    Checked 'cargo' @('test','-p','baauto-bench','--test','windows_capture','--','--ignored','--nocapture','--test-threads=1')
    $args=@('measure','--points','tests/fixtures/m2-basic.json','--keys','tests/fixtures/m2-keys.json','--out',"$out/measure",'--seconds','60','--exercise-receiver')
    if ($PresentMon) { $args+=@('--presentmon',$PresentMon) }
    Write-Host 'Return focus to receiver before measurement Arm.'
    Checked './target/release/baauto-bench.exe' $args
    if ($PresentMon) {
        Checked './target/release/baauto-bench.exe' @('align',"$out/measure/presentmon.csv","$out/measure/ring.csv","$out/aligned.csv")
        Write-Host 'A/B takes multiple rounds and 1000 stop cycles per variant. Keep receiver foreground.'
        Checked './target/release/baauto-bench.exe' @('ab','--points','tests/fixtures/m2-basic.json','--keys','tests/fixtures/m2-keys.json','--out',"$out/ab",'--seconds','60','--exercise-receiver','--presentmon',$PresentMon)
    }
    Write-Host "Evidence directory: $out. Compare receiver QPC, DXGI QPC and ring, then complete docs/bench-howto.md checklist."
} finally {
    if (-not $receiver.HasExited) { Stop-Process -Id $receiver.Id }
}
