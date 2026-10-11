param([string]$UncRoot='')
$ErrorActionPreference='Stop'
Set-Location (Join-Path $PSScriptRoot '../..')
cargo test -p baauto-config
if ($LASTEXITCODE -ne 0) { throw 'Portable compatibility/storage acceptance failed' }
cargo test -p baauto-win-platform --test windows_store
if ($LASTEXITCODE -ne 0) { throw 'Native file replacement acceptance failed' }
if ($UncRoot) {
    $env:BAAUTO_TEST_UNC_ROOT=$UncRoot
    cargo test -p baauto-win-platform --test windows_store paths_unc_native -- --ignored
    if ($LASTEXITCODE -ne 0) { throw 'UNC acceptance failed' }
}
