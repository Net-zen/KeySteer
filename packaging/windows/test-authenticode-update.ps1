[CmdletBinding()]
param(
    [string] $Executable = "target\release\keysteer.exe",
    [switch] $VerifyPackaging
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw "Authenticode update verification requires Windows"
}
Import-Module Microsoft.PowerShell.Security
Import-Module PKI

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$executablePath = (Resolve-Path (Join-Path $projectRoot $Executable)).Path
$temporaryRoot = Join-Path $projectRoot "tmp\authenticode-update-test"
$expectedParent = [IO.Path]::GetFullPath((Join-Path $projectRoot "tmp"))
if (-not [IO.Path]::GetFullPath($temporaryRoot).StartsWith(
        $expectedParent,
        [StringComparison]::OrdinalIgnoreCase)) {
    throw "temporary signing path escaped the workspace tmp directory"
}

function Find-SignTool {
    $command = Get-Command "signtool.exe" -ErrorAction SilentlyContinue
    if ($command) {
        return $command.Source
    }
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $candidate = Get-ChildItem -LiteralPath $kits -Filter "signtool.exe" -Recurse |
        Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $candidate) {
        throw "signtool.exe was not found"
    }
    $candidate.FullName
}

New-Item -ItemType Directory -Force -Path $temporaryRoot | Out-Null
$certificate = $null
try {
    $certificate = New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject "CN=KeySteer Ephemeral Update Test" `
        -FriendlyName "KeySteer ephemeral update test" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -KeyAlgorithm RSA `
        -KeyLength 2048 `
        -HashAlgorithm SHA256 `
        -KeyExportPolicy Exportable `
        -NotAfter (Get-Date).AddDays(1)
    $publicCertificate = Join-Path $temporaryRoot "ephemeral.cer"
    $pfx = Join-Path $temporaryRoot "ephemeral.pfx"
    $pfxPasswordText = [Guid]::NewGuid().ToString("N")
    $pfxPassword = ConvertTo-SecureString -String $pfxPasswordText -AsPlainText -Force
    Export-Certificate -Cert $certificate -FilePath $publicCertificate | Out-Null
    Export-PfxCertificate `
        -Cert $certificate `
        -FilePath $pfx `
        -Password $pfxPassword | Out-Null
    if ($VerifyPackaging) {
        $packagingOutput = Join-Path $temporaryRoot "dist"
        & (Join-Path $PSScriptRoot "package.ps1") `
            -Target "x86_64-pc-windows-msvc" `
            -SigningPfx $pfx `
            -SigningPassword $pfxPasswordText `
            -SkipTimestamp `
            -OutputRoot $packagingOutput `
            -RequireSigning
        if ($LASTEXITCODE -ne 0) {
            throw "signed Windows packaging failed with exit code $LASTEXITCODE"
        }
        $targetOutput = Join-Path $packagingOutput "x86_64-pc-windows-msvc"
        $archives = @(Get-ChildItem -LiteralPath $targetOutput -Filter "*.zip" -File)
        $standaloneExecutables = @(Get-ChildItem -LiteralPath $targetOutput -Filter "*.exe" -File)
        if ($archives.Count -ne 1 -or $standaloneExecutables.Count -ne 0) {
            throw "Windows packaging must produce exactly one ZIP and no standalone EXE"
        }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($archives[0].FullName)
        try {
            $packagedExecutables = @($zip.Entries | Where-Object {
                    $_.FullName.Replace('\', '/') -eq "KeySteer/KeySteer.exe"
                })
            if ($packagedExecutables.Count -ne 1) {
                throw "Windows package must contain exactly one KeySteer/KeySteer.exe"
            }
        }
        finally {
            $zip.Dispose()
        }
    }

    $current = Join-Path $temporaryRoot "signed-current.exe"
    $candidate = Join-Path $temporaryRoot "signed-candidate.exe"
    $tampered = Join-Path $temporaryRoot "tampered-candidate.exe"
    Copy-Item -LiteralPath $executablePath -Destination $current
    Copy-Item -LiteralPath "$env:WINDIR\System32\notepad.exe" -Destination $candidate
    $signTool = Find-SignTool
    foreach ($path in @($current, $candidate)) {
        & $signTool sign /sha1 $certificate.Thumbprint /s My /fd SHA256 $path
        if ($LASTEXITCODE -ne 0) {
            throw "signing $path failed with exit code $LASTEXITCODE"
        }
    }
    Copy-Item -LiteralPath $candidate -Destination $tampered
    $stream = [IO.File]::Open($tampered, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite)
    try {
        $stream.Position = 1024
        $value = $stream.ReadByte()
        if ($value -lt 0) {
            throw "signed test executable is unexpectedly short"
        }
        $stream.Position = 1024
        $stream.WriteByte($value -bxor 1)
        $stream.Flush()
    }
    finally {
        $stream.Dispose()
    }

    $env:KEYSTEER_TEST_SIGNED_CURRENT = $current
    $env:KEYSTEER_TEST_SIGNED_CANDIDATE = $candidate
    $env:KEYSTEER_TEST_TAMPERED_SIGNED = $tampered
    Push-Location $projectRoot
    try {
        & cargo test --locked --all-features `
            platform::windows::update_installer::signature::tests -- --ignored
        if ($LASTEXITCODE -ne 0) {
            throw "Rust Authenticode signer comparison test failed with exit code $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}
finally {
    if ($certificate) {
        $thumbprint = $certificate.Thumbprint
        foreach ($store in @("Root", "My")) {
            $certificatePath = "Cert:\CurrentUser\$store\$thumbprint"
            if (Test-Path -LiteralPath $certificatePath) {
                Remove-Item -LiteralPath $certificatePath -Force
            }
        }
    }
    foreach ($name in @(
            "signed-current.exe",
            "signed-candidate.exe",
            "tampered-candidate.exe",
            "ephemeral.cer",
            "ephemeral.pfx")) {
        $path = Join-Path $temporaryRoot $name
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
        }
    }
    if (Test-Path -LiteralPath $temporaryRoot) {
        Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
    }
}
