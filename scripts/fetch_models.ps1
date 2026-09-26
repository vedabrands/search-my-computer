param(
    [string]$TargetModel = "all",
    [string]$ModelsLockPath = "$PSScriptRoot/../models.lock",
    [string]$OutputDir = "$PSScriptRoot/../models"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $ModelsLockPath)) {
    Write-Error "models.lock not found at $ModelsLockPath"
    exit 1
}

$lockContent = Get-Content $ModelsLockPath -Raw | ConvertFrom-Json
$models = $lockContent.models

if ($null -eq $models) {
    Write-Error "No models defined in $ModelsLockPath"
    exit 1
}

$targetKeys = if ($TargetModel -eq "all") {
    $models.PSObject.Properties.Name
} else {
    if (-not $models.PSObject.Properties.Name.Contains($TargetModel)) {
        Write-Error "Model '$TargetModel' not found in models.lock. Available: $($models.PSObject.Properties.Name -join ', ')"
        exit 1
    }
    @($TargetModel)
}

foreach ($modelKey in $targetKeys) {
    $modelInfo = $models.$modelKey
    Write-Host "=== Fetching Model: $modelKey ($($modelInfo.name)) ==="
    $modelDir = Join-Path $OutputDir $modelKey
    if (-not (Test-Path $modelDir)) {
        New-Item -ItemType Directory -Path $modelDir -Force | Out-Null
    }

    $files = $modelInfo.files
    foreach ($fileName in $files.PSObject.Properties.Name) {
        $fileObj = $files.$fileName
        $destPath = Join-Path $modelDir $fileName
        $expectedSha256 = $fileObj.sha256
        $url = $fileObj.url

        $downloadNeeded = $true
        if (Test-Path $destPath) {
            $existingHash = (Get-FileHash -Path $destPath -Algorithm SHA256).Hash.ToLower()
            if ($existingHash -eq $expectedSha256.ToLower()) {
                Write-Host "  [OK] $fileName exists and SHA-256 matches ($existingHash)"
                $downloadNeeded = $false
            } else {
                Write-Host "  [WARN] $fileName hash mismatch (found $existingHash, expected $expectedSha256), re-downloading..."
            }
        }

        if ($downloadNeeded) {
            Write-Host "  [DOWNLOADING] $fileName from $url ..."
            $tempPath = "$destPath.tmp"
            Invoke-WebRequest -Uri $url -OutFile $tempPath -UseBasicParsing
            $downloadedHash = (Get-FileHash -Path $tempPath -Algorithm SHA256).Hash.ToLower()
            if ($downloadedHash -ne $expectedSha256.ToLower()) {
                Remove-Item -Path $tempPath -Force
                Write-Error "SHA-256 verification failed for $fileName! Expected: $expectedSha256, Got: $downloadedHash"
                exit 1
            }
            Move-Item -Path $tempPath -Destination $destPath -Force
            Write-Host "  [VERIFIED] $fileName downloaded and verified successfully."
        }
    }
}

Write-Host "=== All requested models verified successfully ==="
