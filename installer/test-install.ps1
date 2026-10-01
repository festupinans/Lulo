<#
CI smoke test: installs Lulo silently, checks what the installer did,
uninstalls it and checks it cleaned up. Leaves the user's other settings alone.

Usage: test-install.ps1 Lulo-Setup.exe
#>
param([Parameter(Mandatory = $true)][string] $Setup)

$ErrorActionPreference = 'Stop'
$app = Join-Path $env:LOCALAPPDATA 'Lulo'
$settings = Join-Path $env:USERPROFILE '.claude\settings.json'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$shortcut = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Lulo.lnk'

function Check($ok, $what) {
    if (-not $ok) { throw "FAILED: $what" }
    Write-Host "ok: $what"
}

function Lulo-Hooks {
    $json = Get-Content $settings -Raw | ConvertFrom-Json
    if (-not $json.hooks) { return @() }
    @($json.hooks.PSObject.Properties | ForEach-Object { $_.Value } |
        ForEach-Object { $_.hooks } | Where-Object { $_.command -like '*lulo-hook.exe' })
}

# A settings.json with something of the user's in it, which must survive.
New-Item -ItemType Directory -Force (Split-Path $settings) | Out-Null
'{ "model": "opus" }' | Set-Content $settings

$p = Start-Process $Setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/TASKS=autostart' -Wait -PassThru
Check ($p.ExitCode -eq 0) "installer exit code 0 (got $($p.ExitCode))"
Check (Test-Path "$app\lulo-hook.exe") 'lulo-hook.exe installed'
Check (Test-Path "$app\lulo-widget.exe") 'lulo-widget.exe installed'
Check (Test-Path "$app\unins000.exe") 'uninstaller installed'
Check (Test-Path $shortcut) 'Start menu shortcut'
$run = (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).Lulo
Check ($run -eq "`"$app\lulo-widget.exe`"") "autostart value ($run)"

$hooks = Lulo-Hooks
Check ($hooks.Count -eq 13) "13 Lulo hooks in settings.json (got $($hooks.Count))"
Check ($hooks[0].command -eq "$app\lulo-hook.exe") "hooks point at the installed exe ($($hooks[0].command))"
Check ((Get-Content $settings -Raw | ConvertFrom-Json).model -eq 'opus') 'user settings kept on install'

# The hook runs for real: one event writes a session file.
$status = Join-Path $env:LOCALAPPDATA 'claude-status'
'{"session_id":"ci-test","hook_event_name":"UserPromptSubmit","cwd":"C:\\ci"}' | & "$app\lulo-hook.exe" hook
Check (Test-Path "$status\ci-test.json") 'installed hook writes a session file'

# Installing again (an upgrade) must not duplicate hooks.
$p = Start-Process $Setup -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait -PassThru
Check ($p.ExitCode -eq 0) 'reinstall exit code 0'
Check ((Lulo-Hooks).Count -eq 13) 'reinstall keeps 13 hooks'

# The uninstaller relaunches itself from %TEMP%, so wait for the folder to go.
Start-Process "$app\unins000.exe" -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART' -Wait
for ($i = 0; $i -lt 60 -and (Test-Path "$app\lulo-hook.exe"); $i++) { Start-Sleep 1 }
Check (-not (Test-Path "$app\lulo-hook.exe")) 'files removed'
Check (-not (Test-Path $shortcut)) 'shortcut removed'
Check ($null -eq (Get-ItemProperty $runKey -ErrorAction SilentlyContinue).Lulo) 'autostart value removed'
Check (-not (Test-Path $status)) 'session folder removed'
Check ((Lulo-Hooks).Count -eq 0) 'hooks removed from settings.json'
Check ((Get-Content $settings -Raw | ConvertFrom-Json).model -eq 'opus') 'user settings kept on uninstall'
