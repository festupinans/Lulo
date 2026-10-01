<#
Builds Lulo and its installer on a Windows PC.

  .\installer\build.ps1                 unsigned build
  .\installer\build.ps1 -Sign           signs with the method set in the
                                        environment (see installer\sign.ps1)

Needs Rust (cargo) and Inno Setup 6 (winget install JRSoftware.InnoSetup).
The installer ends up in target\installer\Lulo-Setup-<version>.exe.
#>
param([switch] $Sign)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$version = (Select-String -Path 'crates\lulo-widget\Cargo.toml' -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value

cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }

$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $iscc = @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    ) | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw 'Inno Setup 6 not found. Install it with: winget install JRSoftware.InnoSetup' }

$isccArgs = @("/DAppVersion=$version")
if ($Sign) {
    $signScript = Join-Path $PSScriptRoot 'sign.ps1'
    & $signScript 'target\release\lulo-hook.exe' 'target\release\lulo-widget.exe'
    # Inno Setup calls this for the installer and the uninstaller it embeds.
    $isccArgs += '/DSign', "/Slulo=powershell.exe -NoProfile -ExecutionPolicy Bypass -File `$q$signScript`$q `$f"
}

& $iscc @isccArgs 'installer\lulo.iss'
if ($LASTEXITCODE -ne 0) { throw 'Inno Setup failed' }
Get-Item "target\installer\Lulo-Setup-$version.exe"
