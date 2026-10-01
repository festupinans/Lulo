<#
Signs Windows files (exe, the installer and its uninstaller) with Authenticode.

The signing method comes from environment variables, so the same script works
in GitHub Actions and on your own PC:

  Azure Artifact Signing (CI):
    ARTIFACT_SIGNING_ENDPOINT   e.g. https://eus.codesigning.azure.net/
    ARTIFACT_SIGNING_ACCOUNT    signing account name
    ARTIFACT_SIGNING_PROFILE    certificate profile name
    AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET   app registration

  A certificate in the Windows certificate store (Certum SimplySign, a USB
  token, or any OV/EV certificate):
    SIGN_CERT_THUMBPRINT        SHA-1 thumbprint of the certificate
    SIGN_TIMESTAMP_URL          optional, default http://time.certum.pl

Usage: sign.ps1 file1.exe [file2.exe ...]
#>
param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]] $Files
)

$ErrorActionPreference = 'Stop'
$Files = $Files | ForEach-Object { (Resolve-Path -LiteralPath $_.Trim('"')).Path }
$Description = 'Lulo'
$DescriptionUrl = 'https://github.com/festupinans/Lulo'

function Find-SignTool {
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) { return $cmd.Source }
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $found = Get-ChildItem -Path $kits -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Directory.Name -eq 'x64' } |
        Sort-Object { [version]$_.Directory.Parent.Name } -Descending -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if (-not $found) {
        throw 'signtool.exe not found. Install the Windows SDK ("Windows SDK Signing Tools for Desktop Apps").'
    }
    return $found.FullName
}

if ($env:ARTIFACT_SIGNING_ENDPOINT) {
    if (-not (Get-Module -ListAvailable -Name ArtifactSigning)) {
        Install-Module -Name ArtifactSigning -Scope CurrentUser -Force -Repository PSGallery
    }
    Import-Module ArtifactSigning
    Invoke-ArtifactSigning `
        -Endpoint $env:ARTIFACT_SIGNING_ENDPOINT `
        -CodeSigningAccountName $env:ARTIFACT_SIGNING_ACCOUNT `
        -CertificateProfileName $env:ARTIFACT_SIGNING_PROFILE `
        -Files ($Files -join ',') `
        -FileDigest SHA256 `
        -TimestampRfc3161 'http://timestamp.acs.microsoft.com' `
        -TimestampDigest SHA256 `
        -Description $Description `
        -DescriptionUrl $DescriptionUrl
}
elseif ($env:SIGN_CERT_THUMBPRINT) {
    $timestamp = if ($env:SIGN_TIMESTAMP_URL) { $env:SIGN_TIMESTAMP_URL } else { 'http://time.certum.pl' }
    $signtool = Find-SignTool
    & $signtool sign /sha1 $env:SIGN_CERT_THUMBPRINT /fd SHA256 /tr $timestamp /td SHA256 `
        /d $Description /du $DescriptionUrl @Files
    if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE" }
}
else {
    throw 'No signing method configured: set ARTIFACT_SIGNING_ENDPOINT or SIGN_CERT_THUMBPRINT.'
}
