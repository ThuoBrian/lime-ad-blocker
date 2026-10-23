# Daily refresh: download + cache the blocklist, then apply it to the hosts
# file. Invoked by the "adblocker-update" scheduled task, which runs with
# highest privileges, so `apply` needs no extra elevation step.
#
# Output is appended to %APPDATA%\adblocker\refresh.log. The binary is run
# through cmd.exe so its UTF-8 output lands in the log byte-for-byte
# (Windows PowerShell 5.1 redirection would re-encode it as UTF-16).
#
# install-schedule.ps1 passes -Adblocker with the binary's path.

param(
    [string]$Adblocker = (Join-Path $env:USERPROFILE '.cargo\bin\adblocker.exe')
)

$AppDir = Join-Path $env:APPDATA 'adblocker'
$Log = Join-Path $AppDir 'refresh.log'

New-Item -ItemType Directory -Force -Path $AppDir | Out-Null
[IO.File]::AppendAllText($Log, "=== $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') ===`r`n")

$status = 0
foreach ($command in 'update', 'apply') {
    cmd.exe /d /c "`"$Adblocker`" $command >> `"$Log`" 2>&1"
    if ($LASTEXITCODE -ne 0) {
        # Never apply after a failed update; the previous hosts block stays.
        $status = $LASTEXITCODE
        break
    }
}

# Keep the log to its most recent lines (a few months of daily runs).
$lines = [IO.File]::ReadAllLines($Log)
if ($lines.Count -gt 1000) {
    [IO.File]::WriteAllLines($Log, $lines[($lines.Count - 1000)..($lines.Count - 1)])
}
exit $status
