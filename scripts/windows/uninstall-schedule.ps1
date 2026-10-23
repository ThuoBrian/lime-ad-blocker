# Remove the daily adblocker refresh task. Run from an Administrator
# PowerShell. Leaves your config, cache, and the hosts file untouched; run
# `adblocker disable` (as Administrator) to remove the block itself.

$ErrorActionPreference = 'Stop'

$TaskName = 'adblocker-update'
$Script = Join-Path $env:APPDATA 'adblocker\refresh-and-apply.ps1'

if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    Write-Host "Removed scheduled task $TaskName."
} else {
    Write-Host "No scheduled task named $TaskName."
}

if (Test-Path $Script) {
    Remove-Item -Force $Script
    Write-Host "Removed $Script."
}
