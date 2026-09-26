<#
.SYNOPSIS
    Trains or exports a custom wake-word ONNX model for SearchMyComputer.

.DESCRIPTION
    Automates synthetic data generation, openWakeWord embedding feature extraction,
    and ONNX classification head export for custom wake words (e.g. "Kira").

.PARAMETER Word
    The wake word or phrase to train (Default: "Kira").

.PARAMETER Output
    The output destination path for the exported ONNX model (Default: "models/wake_words/kira.onnx").

.PARAMETER Steps
    Number of training optimization steps (Default: 2000).

.EXAMPLE
    .\scripts\train_wake_word.ps1 -Word "Kira" -Output "models/wake_words/kira.onnx"
#>

[CmdletBinding()]
param(
    [string]$Word = "Kira",
    [string]$Output = "models/wake_words/kira.onnx",
    [int]$Steps = 2000
)

$ErrorActionPreference = "Stop"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host " SearchMyComputer - Custom Wake Word Pipeline ($Word)" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

$PythonExe = (Get-Command python -ErrorAction SilentlyContinue).Source
if (-not $PythonExe) {
    Write-Warning "Python 3 was not found in PATH. Please install Python 3.10+."
    exit 1
}

$ScriptPath = Join-Path $PSScriptRoot "train_wake_word.py"
& $PythonExe $ScriptPath --word $Word --output $Output --steps $Steps

if ($LASTEXITCODE -eq 0) {
    Write-Host "`n[SUCCESS] Wake word model ready at: $Output" -ForegroundColor Green
} else {
    Write-Error "[FAILED] Failed to train or export wake word model."
}
