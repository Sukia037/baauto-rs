param([switch]$Interactive, [switch]$Screenshots)
$ErrorActionPreference='Stop'
function Checked([string]$Command, [string[]]$Arguments) {
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Command failed: $LASTEXITCODE" }
}
Checked 'cargo' @('fmt','--all','--check')
Checked 'cargo' @('clippy','--workspace','--all-targets','--all-features','--','-D','warnings')
Checked 'cargo' @('test','--workspace')
Checked 'cargo' @('build','--release','--locked','--workspace')
if ($Screenshots) {
    $data=Join-Path $env:TEMP ('baauto-m6-ui-'+[guid]::NewGuid())
    $out='docs/screenshots/windows-current-dpi'
    foreach ($dark in @($false,$true)) {
        foreach ($tab in 0..4) {
            $a=@('--preview','--data-dir',$data,'--qa-dir',$out,'--qa-tab',"$tab")
            if ($dark) { $a+=@('--qa-dark') }
            Checked './target/release/baauto-app.exe' $a
        }
    }
    Write-Host "Native Windows screenshots (10 images): $out. Record actual Windows monitor DPI separately; repeat at 100/150/200% and on mixed-DPI monitors."
}
if ($Interactive) {
    try {
        Checked 'cargo' @('build','--release','--workspace','--features','test-hooks')
        Write-Host 'Receiver will open: keep it foreground. This test checks IPC timeout/cancel/Space capture and Armed rejection.'
        Checked 'cargo' @('test','-p','baauto-win-platform','--features','test-hooks','--test','windows_capture','--','--ignored','--nocapture','--test-threads=1')
    } finally {
        # Restore the production binaries before any actual game use.
        Checked 'cargo' @('build','--release','--workspace')
    }
}
Write-Host 'Complete docs/acceptance-m6.md for mouse recording, all 23 actions, the 8-hour receiver run and both 1-hour game runs. These are not implied by compilation.'
