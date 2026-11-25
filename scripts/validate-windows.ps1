[CmdletBinding(DefaultParameterSetName = 'Foreground')]
param(
    [Parameter(ParameterSetName = 'Detached')][switch]$Detach,
    [Parameter(ParameterSetName = 'Foreground')][string]$ResultFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if ($Detach) {
    $runName = 'windows-{0}-{1}' -f (Get-Date -Format 'yyyyMMdd-HHmmss'), ([guid]::NewGuid().ToString('N').Substring(0, 8))
    $run = Join-Path $root "target/validation/$runName"
    New-Item -ItemType Directory -Path $run | Out-Null
    $resultPath = Join-Path $run 'exit-code.txt'
    $process = Start-Process -FilePath (Get-Process -Id $PID).Path `
        -ArgumentList "-NoProfile -NonInteractive -File `"$PSCommandPath`" -ResultFile `"$resultPath`"" `
        -WorkingDirectory $root -WindowStyle Hidden -PassThru `
        -RedirectStandardOutput (Join-Path $run 'stdout.log') `
        -RedirectStandardError (Join-Path $run 'stderr.log')
    [IO.File]::WriteAllText((Join-Path $run 'pid.txt'), "$($process.Id)`n")
    Write-Output "Validation PID $($process.Id); logs and exit status: $run"
    return
}

if ($ResultFile) {
    $ResultFile = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($ResultFile)
}

function Invoke-CargoStep([string[]]$Arguments) {
    Write-Host "> cargo $($Arguments -join ' ')"
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "cargo $($Arguments -join ' ') exited with $LASTEXITCODE"
    }
}

$result = 1
Push-Location $root
try {
    Invoke-CargoStep @('fmt', '--all', '--', '--check')
    Invoke-CargoStep @('test', '--locked')
    Invoke-CargoStep @('test', '--locked', '--no-default-features')
    Invoke-CargoStep @('clippy', '--locked', '--all-targets', '--', '-D', 'warnings')
    Invoke-CargoStep @('build', '--locked', '--release')
    $result = 0
} catch {
    [Console]::Error.WriteLine($_.ToString())
} finally {
    Pop-Location
    if ($ResultFile) {
        # Publish only on a recorded success or failure; each detached run has its own directory.
        [IO.File]::WriteAllText("$ResultFile.tmp", "$result`n")
        [IO.File]::Move("$ResultFile.tmp", $ResultFile)
    }
}
exit $result
