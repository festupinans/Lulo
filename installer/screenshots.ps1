<#
CI only: clicks through the installer in Spanish and saves a screenshot of
each page (and of the widget it opens) for reviewing changes. Uninstalls
afterwards so test-install.ps1 starts clean.

Usage: screenshots.ps1 Lulo-Setup.exe <output folder>
#>
param(
    [Parameter(Mandatory = $true)][string] $Setup,
    [Parameter(Mandatory = $true)][string] $OutDir
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
New-Item -ItemType Directory -Force $OutDir | Out-Null
$shell = New-Object -ComObject WScript.Shell

function Shot($name) {
    Start-Sleep -Milliseconds 800
    $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
    $bmp.Save((Join-Path $OutDir "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    # TEMP: echo a JPEG copy into the log for reviewers who can't download artifacts.
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Jpeg)
    Write-Host "SHOT:$name`:$([Convert]::ToBase64String($ms.ToArray()))"
    $g.Dispose(); $bmp.Dispose()
}

function Press($key, $title) {
    for ($i = 0; $i -lt 20 -and -not $shell.AppActivate($title); $i++) { Start-Sleep -Milliseconds 500 }
    [System.Windows.Forms.SendKeys]::SendWait($key)
}

$title = 'Instalar - Lulo'
Start-Process $Setup -ArgumentList '/LANG=es'
Start-Sleep 4
Shot '1-tareas'
Press '{ENTER}' $title          # Siguiente: installs (no Ready page)
Start-Sleep 8
Shot '2-terminado'
Press '{ENTER}' $title          # Finalizar: opens the widget
Start-Sleep 4
Shot '3-widget'

Stop-Process -Name lulo-widget -ErrorAction SilentlyContinue
$unins = Join-Path $env:LOCALAPPDATA 'Lulo\unins000.exe'
if (Test-Path $unins) {
    Start-Process $unins -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES' -Wait
    for ($i = 0; $i -lt 60 -and (Test-Path $unins); $i++) { Start-Sleep 1 }
}
