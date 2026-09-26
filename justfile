# SearchMyComputer task runner
# Install just: cargo install just

set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

default:
    @just --list

# Format all code
fmt:
    cargo fmt --all
    cd src && npx prettier --write "**/*.{ts,tsx,css,json}" 2>$null; if (-not $?) { Write-Host "prettier not found, skipping frontend fmt" }

# Lint all code
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cd src && npx tsc --noEmit 2>$null; if (-not $?) { Write-Host "tsc not found, skipping frontend lint" }

# Run all tests
test:
    cargo test --workspace

# Run in dev mode
dev:
    cargo tauri dev

# Build release
build:
    cargo tauri build

# Check licenses
deny:
    cargo deny check licenses

# Run fmt + lint + test
check: fmt lint test
