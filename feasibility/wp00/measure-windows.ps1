$ErrorActionPreference = 'Stop'
$evidence = Join-Path $PSScriptRoot 'target/evidence'
New-Item -ItemType Directory -Force $evidence | Out-Null
$executable = Join-Path $PSScriptRoot 'target/release/minimalmonitor-wp00.exe'
$process = Start-Process -FilePath $executable -WorkingDirectory $PSScriptRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $evidence 'run.stdout.txt') -RedirectStandardError (Join-Path $evidence 'run.stderr.txt')
$samples = [System.Collections.Generic.List[object]]::new()
$peak = 0L
while (-not $process.HasExited) {
    $process.Refresh()
    $peak = [Math]::Max($peak, $process.PeakWorkingSet64)
    $samples.Add([pscustomobject]@{
        elapsed_ms = [int]([DateTime]::Now - $process.StartTime).TotalMilliseconds
        working_set_bytes = $process.WorkingSet64
        peak_working_set_bytes = $process.PeakWorkingSet64
        private_bytes = $process.PrivateMemorySize64
    })
    Start-Sleep -Milliseconds 10
}
$process.WaitForExit()
$samples | Export-Csv -NoTypeInformation (Join-Path $evidence 'memory.csv')
$summary = [pscustomobject]@{
    exit_code = $process.ExitCode
    sample_count = $samples.Count
    observed_peak_working_set_bytes = $peak
    executable_bytes = (Get-Item $executable).Length
    final_state_bytes = (Get-ChildItem (Join-Path $PSScriptRoot 'target/wp00-state') -File | Measure-Object -Property Length -Sum).Sum
}
$summary | ConvertTo-Json | Set-Content (Join-Path $evidence 'summary.json')
Get-Content (Join-Path $evidence 'run.stdout.txt')
$summary | Format-List
if ($process.ExitCode -ne 0) { throw 'WP-00 release harness failed' }
