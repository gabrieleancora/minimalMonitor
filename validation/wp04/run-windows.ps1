$ErrorActionPreference = 'Stop'
$project = (Get-Location).Path
$env:RUSTUP_TOOLCHAIN = 'stable'
$env:RUSTUP_AUTO_INSTALL = '0'
$env:CARGO_HOME = Join-Path $project '.cargo'
$evidence = Join-Path $project 'validation/wp04/evidence/windows'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
Get-Date -AsUTC -Format o | Out-File -LiteralPath (Join-Path $evidence 'environment.txt')
rustc -Vv | Out-File -LiteralPath (Join-Path $evidence 'environment.txt') -Append
cargo -V | Out-File -LiteralPath (Join-Path $evidence 'environment.txt') -Append
$exits = Join-Path $evidence 'exits.tsv'
Set-Content -LiteralPath $exits -Value ''
function Invoke-Check {
    param([string]$Name, [string]$Executable, [string[]]$Arguments)
    & $Executable @Arguments *> (Join-Path $evidence "$Name.log")
    $result = $LASTEXITCODE
    Add-Content -LiteralPath $exits -Value "$Name`t$result"
    Write-Output "$Name exit=$result"
    if ($result -ne 0) {
        Get-Content -LiteralPath (Join-Path $evidence "$Name.log") -Tail 50
        throw "$Name failed"
    }
}
Invoke-Check 'fmt' 'cargo' @('fmt','--all','--','--check')
Invoke-Check 'clippy' 'cargo' @('clippy','--locked','--all-targets','--offline','--','-D','warnings')
Invoke-Check 'test' 'cargo' @('test','--locked','--offline')
Invoke-Check 'release' 'cargo' @('build','--release','--locked','--offline')
Invoke-Check 'version' 'target/release/minimalmonitor.exe' @('--version')
Invoke-Check 'lifecycle' 'python' @('validation/wp04/lifecycle.py','target/release/minimalmonitor.exe')
Get-FileHash -Algorithm SHA256 target/release/minimalmonitor.exe | Format-List | Out-File -LiteralPath (Join-Path $evidence 'artifact.txt')
(Get-Item target/release/minimalmonitor.exe).Length | Out-File -LiteralPath (Join-Path $evidence 'artifact.txt') -Append
