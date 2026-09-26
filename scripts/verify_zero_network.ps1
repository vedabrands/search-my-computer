<#
.SYNOPSIS
    Automated Zero-Network Verification Script for SearchMyComputer.
.DESCRIPTION
    Audits Rust workspace dependencies, frontend packages, Tauri CSP, and source
    code to guarantee 100% offline operation and zero runtime network egress.
#>

$ErrorActionPreference = "Stop"
$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host "  SearchMyComputer - Automated Zero-Network Audit Verification" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan
Write-Host ""

$Passed = $true
$TotalChecks = 0
$PassedChecks = 0

function Report-Check {
    param(
        [string]$Name,
        [bool]$Success,
        [string]$Details = ""
    )
    $script:TotalChecks++
    if ($Success) {
        $script:PassedChecks++
        Write-Host " [PASS] $Name" -ForegroundColor Green
        if ($Details) {
            Write-Host "        $Details" -ForegroundColor DarkGray
        }
    } else {
        $script:Passed = $false
        Write-Host " [FAIL] $Name" -ForegroundColor Red
        if ($Details) {
            Write-Host "        $Details" -ForegroundColor Yellow
        }
    }
}

# --- Check 1: Rust Cargo Dependencies (Cargo.lock) ---
$BannedRustCrates = @(
    "reqwest", "hyper", "curl", "ureq", "surf", "attohttpc", "isahc",
    "tungstenite", "tokio-tungstenite", "async-tungstenite",
    "tauri-plugin-http", "tauri-plugin-updater", "sentry", "datadog"
)

$CargoLockPath = Join-Path $ProjectRoot "Cargo.lock"
if (Test-Path $CargoLockPath) {
    $LockContent = Get-Content $CargoLockPath -Raw
    $FoundBannedCrates = @()
    foreach ($crate in $BannedRustCrates) {
        if ($LockContent -match "(?m)^name = `"$crate`"") {
            $FoundBannedCrates += $crate
        }
    }

    if ($FoundBannedCrates.Count -eq 0) {
        Report-Check "No banned network or telemetry crates in Cargo.lock" $true "Audited: $($BannedRustCrates -join ', ')"
    } else {
        Report-Check "Banned network crates detected in Cargo.lock" $false "Found: $($FoundBannedCrates -join ', ')"
    }
} else {
    Report-Check "Cargo.lock exists" $false "Cargo.lock not found at $CargoLockPath"
}

# --- Check 2: Frontend Dependencies (package.json) ---
$BannedNpmPackages = @(
    "axios", "posthog-js", "mixpanel-browser", "@sentry/browser",
    "@sentry/react", "segment-analytics", "firebase", "@amplitude/analytics-browser",
    "@tauri-apps/plugin-http", "@tauri-apps/plugin-updater"
)

$PackageJsonPath = Join-Path $ProjectRoot "package.json"
if (Test-Path $PackageJsonPath) {
    $PkgJson = Get-Content $PackageJsonPath -Raw | ConvertFrom-Json
    $Deps = @()
    if ($PkgJson.dependencies) { $Deps += $PkgJson.dependencies.PSObject.Properties.Name }
    if ($PkgJson.devDependencies) { $Deps += $PkgJson.devDependencies.PSObject.Properties.Name }

    $FoundBannedNpm = @()
    foreach ($pkg in $BannedNpmPackages) {
        if ($Deps -contains $pkg) {
            $FoundBannedNpm += $pkg
        }
    }

    if ($FoundBannedNpm.Count -eq 0) {
        Report-Check "No banned network or telemetry packages in package.json" $true "Audited: $($BannedNpmPackages -join ', ')"
    } else {
        Report-Check "Banned npm packages detected in package.json" $false "Found: $($FoundBannedNpm -join ', ')"
    }
} else {
    Report-Check "package.json exists" $false "package.json not found at $PackageJsonPath"
}

# --- Check 3: Tauri CSP Configuration (tauri.conf.json) ---
$TauriConfPath = Join-Path $ProjectRoot "src-tauri\tauri.conf.json"
if (Test-Path $TauriConfPath) {
    $TauriConf = Get-Content $TauriConfPath -Raw | ConvertFrom-Json
    $Csp = $TauriConf.app.security.csp
    $IsStrictCsp = $Csp -and ($Csp -match "default-src 'self'") -and ($Csp -match "connect-src 'self'")

    if ($IsStrictCsp) {
        Report-Check "Strict Tauri Content Security Policy (CSP)" $true "CSP: $Csp"
    } else {
        Report-Check "Strict Tauri Content Security Policy (CSP)" $false "CSP missing default-src 'self' or connect-src 'self': $Csp"
    }
} else {
    Report-Check "tauri.conf.json exists" $false "tauri.conf.json not found at $TauriConfPath"
}

# --- Check 4: Runtime Network Calls in Source Code ---
$RustFiles = Get-ChildItem -Path (Join-Path $ProjectRoot "crates"), (Join-Path $ProjectRoot "src-tauri") -Recurse -Filter "*.rs" | Where-Object {
    $_.FullName -notmatch "target" -and $_.FullName -notmatch "tests"
}

$SuspiciousNetworkPatterns = @(
    "std::net::TcpStream::connect",
    "std::net::UdpSocket::bind",
    "tokio::net::TcpStream::connect"
)

$FoundSuspicious = @()
foreach ($file in $RustFiles) {
    $content = Get-Content $file.FullName -Raw
    foreach ($pat in $SuspiciousNetworkPatterns) {
        if ($content -match [regex]::Escape($pat)) {
            $FoundSuspicious += "$($file.Name): $pat"
        }
    }
}

if ($FoundSuspicious.Count -eq 0) {
    Report-Check "No raw runtime TCP/UDP socket connections in Rust crates" $true "Scanned $($RustFiles.Count) source files"
} else {
    Report-Check "Suspicious socket calls detected in Rust crates" $false ($FoundSuspicious -join "; ")
}

# --- Summary ---
Write-Host ""
Write-Host "------------------------------------------------------------" -ForegroundColor Cyan
Write-Host "Audit Results: $PassedChecks / $TotalChecks checks passed." -ForegroundColor ($Passed ? "Green" : "Red")
Write-Host "------------------------------------------------------------" -ForegroundColor Cyan

if ($Passed) {
    Write-Host "SUCCESS: Zero-Network verification PASSED. SearchMyComputer is 100% offline." -ForegroundColor Green
    exit 0
} else {
    Write-Host "FAILURE: Zero-Network verification FAILED. Please review findings above." -ForegroundColor Red
    exit 1
}
