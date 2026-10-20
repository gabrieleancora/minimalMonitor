$ErrorActionPreference='Stop'
$project=(Get-Location).Path
$env:RUSTUP_TOOLCHAIN='stable';$env:RUSTUP_AUTO_INSTALL='0';$env:CARGO_HOME=Join-Path $project '.cargo'
$evidence=Join-Path $project $(if($env:MM_WP05_EVIDENCE){$env:MM_WP05_EVIDENCE}else{'validation/wp05/evidence/windows'})
New-Item -ItemType Directory -Force $evidence | Out-Null
Get-Date -AsUTC -Format o | Out-File (Join-Path $evidence 'environment.txt')
rustc -Vv | Out-File (Join-Path $evidence 'environment.txt') -Append
cargo -V | Out-File (Join-Path $evidence 'environment.txt') -Append
Set-Content (Join-Path $evidence 'exits.tsv') ''
function Invoke-Check {
 param([string]$Name,[string]$Executable,[string[]]$Arguments)
 & $Executable @Arguments *> (Join-Path $evidence "$Name.log")
 $code=$LASTEXITCODE
 Add-Content (Join-Path $evidence 'exits.tsv') "$Name`t$code"
 Write-Output "$Name exit=$code"
 if($code -ne 0){Get-Content (Join-Path $evidence "$Name.log") -Tail 40;throw "$Name failed"}
}
Invoke-Check 'fmt' 'cargo' @('fmt','--all','--','--check')
Invoke-Check 'clippy' 'cargo' @('clippy','--locked','--all-targets','--offline','--','-D','warnings')
Invoke-Check 'test' 'cargo' @('test','--locked','--offline')
Invoke-Check 'release' 'cargo' @('build','--release','--locked','--offline')
Invoke-Check 'version' 'target/release/minimalmonitor.exe' @('--version')
Invoke-Check 'lifecycle' 'python' @('validation/wp04/lifecycle.py','target/release/minimalmonitor.exe')
Get-FileHash -Algorithm SHA256 target/release/minimalmonitor.exe | Format-List | Out-File (Join-Path $evidence 'artifact.txt')
(Get-Item target/release/minimalmonitor.exe).Length | Out-File (Join-Path $evidence 'artifact.txt') -Append
