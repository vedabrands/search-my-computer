# setup-github.ps1
# Initialize Git repository, commit all 12 chunks, and push to GitHub.

$ErrorActionPreference = "Stop"

# Navigate to project root
$ProjectDir = if ($PSScriptRoot) { $PSScriptRoot } else { "C:\Users\dev\search-my-computer" }
Set-Location -Path $ProjectDir
Write-Host "Working in: $(Get-Location)"

# Initialize git repository if not already initialized
if (-not (Test-Path ".git")) {
    Write-Host "Initializing git repository..."
    git init
} else {
    Write-Host "Git repository already initialized."
}

# Stage all files
Write-Host "Staging all project files..."
git add .

# Create initial commit
Write-Host "Committing project..."
git commit -m @"
Search My Computer - 12 chunks complete: semantic search, voice input, floating UI, offline licensing

Co-Authored-By: Claude Code <noreply@anthropic.com>
"@

# Configure remote origin
$RemoteUrl = "https://github.com/vedabrands/search-my-computer.git"
Write-Host "Configuring remote origin: $RemoteUrl"
$ExistingRemote = git remote | Where-Object { $_ -eq "origin" }
if ($ExistingRemote) {
    git remote set-url origin $RemoteUrl
} else {
    git remote add origin $RemoteUrl
}

# Set default branch to main
Write-Host "Setting default branch to main..."
git branch -M main

# Push to GitHub
Write-Host "Pushing to GitHub (main branch)..."
git push -u origin main

Write-Host "Setup complete!"
