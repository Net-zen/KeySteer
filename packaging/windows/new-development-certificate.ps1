[CmdletBinding()]
param(
    [string] $Subject = "CN=KeySteer Development",
    [string] $OutputDirectory = (Join-Path $PSScriptRoot ".certificates"),
    [ValidateRange(1, 10)]
    [int] $ValidYears = 2,
    [switch] $TrustOnThisMachine
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw "development Authenticode certificates can only be created on Windows"
}
Import-Module Microsoft.PowerShell.Security
Import-Module PKI

$certificate = New-SelfSignedCertificate `
    -Type CodeSigningCert `
    -Subject $Subject `
    -FriendlyName "KeySteer development code signing" `
    -CertStoreLocation "Cert:\CurrentUser\My" `
    -KeyAlgorithm RSA `
    -KeyLength 3072 `
    -HashAlgorithm SHA256 `
    -KeyExportPolicy Exportable `
    -NotAfter (Get-Date).AddYears($ValidYears)

New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$output = (Resolve-Path -LiteralPath $OutputDirectory).Path
$pfxPath = Join-Path $output "keysteer-development.pfx"
$cerPath = Join-Path $output "keysteer-development.cer"
$password = Read-Host "Password protecting the PFX (also used when importing into Kleopatra)" -AsSecureString

Export-PfxCertificate -Cert $certificate -FilePath $pfxPath -Password $password | Out-Null
Export-Certificate -Cert $certificate -FilePath $cerPath | Out-Null

if ($TrustOnThisMachine) {
    Import-Certificate `
        -FilePath $cerPath `
        -CertStoreLocation "Cert:\CurrentUser\Root" | Out-Null
}

[PSCustomObject]@{
    Subject = $certificate.Subject
    Thumbprint = $certificate.Thumbprint
    Pfx = $pfxPath
    PublicCertificate = $cerPath
    TrustedForCurrentUser = [bool] $TrustOnThisMachine
    Kleopatra = "Import the PFX with File > Import. SignTool still uses the PFX or CurrentUser\\My thumbprint."
}
