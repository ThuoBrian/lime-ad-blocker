# Install the daily adblocker refresh schedule (Task Scheduler) for the
# current user. Run from an Administrator PowerShell:
#
#   powershell -ExecutionPolicy Bypass -File scripts\windows\install-schedule.ps1
#
# What it does:
#   1. Copies refresh-and-apply.ps1 into %APPDATA%\adblocker\
#   2. Registers the "adblocker-update" task: daily at 04:17 (or at the next
#      chance if the PC was off), as you, with highest privileges, only while
#      you are logged on. No password is stored.

$ErrorActionPreference = 'Stop'

$TaskName = 'adblocker-update'
$AppDir = Join-Path $env:APPDATA 'adblocker'
$Script = Join-Path $AppDir 'refresh-and-apply.ps1'

# Wherever it was installed (cargo, Scoop, or a release download on PATH).
$Adblocker = (Get-Command adblocker.exe -ErrorAction SilentlyContinue).Source
if (-not $Adblocker) {
    $Adblocker = Join-Path $env:USERPROFILE '.cargo\bin\adblocker.exe'
}
if (-not (Test-Path $Adblocker)) {
    Write-Error "adblocker.exe not found on PATH or in ~\.cargo\bin. Run 'just install' first."
}

$identity = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
if (-not $identity.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    Write-Error 'Registering a highest-privilege task needs an Administrator PowerShell.'
}

Write-Host "Installing adblocker daily refresh for $env:USERNAME..."

# 1. Refresh script.
New-Item -ItemType Directory -Force -Path $AppDir | Out-Null
Copy-Item -Force -Path (Join-Path $PSScriptRoot 'refresh-and-apply.ps1') -Destination $Script

# 2. Scheduled task.
$action = New-ScheduledTaskAction -Execute 'powershell.exe' `
    -Argument "-NoProfile -NonInteractive -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$Script`" -Adblocker `"$Adblocker`""
$trigger = New-ScheduledTaskTrigger -Daily -At '04:17'
$principal = New-ScheduledTaskPrincipal -UserId "$env:USERDOMAIN\$env:USERNAME" `
    -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries -ExecutionTimeLimit (New-TimeSpan -Minutes 10)

Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger `
    -Principal $principal -Settings $settings -Force `
    -Description 'Refresh the adblocker blocklist and apply it to the hosts file.' | Out-Null

Write-Host "Scheduled: $TaskName runs daily at 04:17. Log: $AppDir\refresh.log"
Write-Host ''
Write-Host "To test the schedule now:   Start-ScheduledTask -TaskName $TaskName"
Write-Host 'To uninstall:               scripts\windows\uninstall-schedule.ps1 (as Administrator)'
