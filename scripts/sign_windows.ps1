<#
.SYNOPSIS
    Code Signing Script for SearchMyComputer Windows Executables and NSIS Installers.
.DESCRIPTION
    Signs SearchMyComputer binaries using Microsoft signtool.exe with Authenticode
    and RFC 3161 SHA-256 timestamping.
.PARAMETER BinaryPath
    The path to the .exe or setup bundle to sign.
.PARAMETER CertPath
    Path to the .pfx code signing certificate file.
.PARAMETER CertPassword
    Password for the .pfx certificate.
.PARAMETER TimestampServer
    RFC 3161 timestamp server URL (defaults to DigiCert).
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryPath,

    [Parameter(Mandatory = $false)]
    [string]$CertPath,

    [Parameter(Mandatory = $false)]
    [string]$CertPassword,

    [Parameter(Mandatory = $false)]
    [string]$TimestampServer = "http://timestamp.digicert.com"
)

$ErrorActionPreference = "Stop"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "  SearchMyComputer - Windows Binary Code Signing" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""

if (-not (Test-Path $BinaryPath)) {
    Write-Error "Target binary not found at path: $BinaryPath"
}

# Locate signtool.exe in Windows Kits
$SignToolPath = $null
$WindowsKitsPaths = @(
    "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe",
    "${env:ProgramFiles}\Windows Kits\10\bin\*\x64\signtool.exe"
)

foreach ($pattern in $WindowsKitsPaths) {
    $found = Get-Item $pattern -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
    if ($found) {
        $SignToolPath = $found.FullName
        break
    }
}

if (-not $SignToolPath) {
    # Check if signtool is in PATH
    $cmd = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($cmd) {
        $SignToolPath = $cmd.Source
    }
}

if (-not $SignToolPath) {
    Write-Error "signtool.exe not found. Please install the Windows 10/11 SDK or add signtool.exe to PATH."
}

Write-Host "Found signtool: $SignToolPath" -ForegroundColor DarkGray
Write-Host "Target binary:  $BinaryPath" -ForegroundColor DarkGray
Write-Host "Timestamp URL:  $TimestampServer" -ForegroundColor DarkGray
Write-Host ""

# Construct signing arguments
$SignArgs = @(
    "sign",
    "/fd", "SHA256",
    "/tr", $TimestampServer,
    "/td", "SHA256",
    "/v"
)

if ($CertPath) {
    if (-not (Test-Path $CertPath)) {
        Write-Error "Certificate file not found at: $CertPath"
    }
    $SignArgs += "/f", $CertPath
    if ($CertPassword) {
        $SignArgs += "/p", $CertPassword
    }
} else {
    Write-Host "No certificate path specified; attempting auto-selection from Windows Certificate Store..." -ForegroundColor Yellow
    $SignArgs += "/a"
}

$SignArgs += $BinaryPath

Write-Host "Executing signtool..." -ForegroundColor Cyan
& $SignToolPath $SignArgs

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    Write-Host "SUCCESS: Binary signed and timestamped successfully." -ForegroundColor Green

    Write-Host "Verifying signature..." -ForegroundColor Cyan
    & $SignToolPath verify /pa /v $BinaryPath
} else {
    Write-Host ""
    Write-Error "FAILURE: Code signing failed with exit code $LASTEXITCODE."
}
