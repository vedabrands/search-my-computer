# SearchMyComputer — Build & Release Engineering Guide

This guide details the complete build, bundling, code signing, and release pipeline for SearchMyComputer on Windows.

---

## 1. Prerequisites

### Build Environment
- **Operating System**: Windows 10/11 (64-bit).
- **Rust**: Rust stable (1.80+ or latest stable toolchain with MSVC target `x86_64-pc-windows-msvc`).
- **Node.js**: Node 20+ LTS and npm 10+.
- **NSIS**: Nullsoft Scriptable Install System (managed automatically by Tauri CLI).
- **Windows SDK**: Contains `signtool.exe` for code signing.

---

## 2. Pre-Build Model Packaging

SearchMyComputer enforces a strict **Zero-Network Runtime Rule**. Embedding and vision models must be downloaded and verified at build time and bundled into the application resources.

### Model Download & Verification
Run the automated model fetch script:
```powershell
powershell -ExecutionPolicy Bypass -File ./scripts/download_models.ps1
```

Verify that model files exist and match their SHA-256 checksums in `models.lock`:
- `resources/models/bge-small-en-v1.5/model.onnx` (int8 quantized, ~33 MB)
- `resources/models/bge-small-en-v1.5/tokenizer.json` (~711 KB)
- `resources/models/clip-vit-base-patch32/visual.onnx` (~87 MB)
- `resources/models/clip-vit-base-patch32/textual.onnx` (~63 MB)

---

## 3. Production Build Pipeline

### Step 1: Frontend Asset Compilation
```powershell
npm run build
```
Generates production-optimized, minified web bundle in `./dist/`.

### Step 2: Rust Workspace Verification
```powershell
# Format and lint audit
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Execute test suite
cargo test --workspace

# Audit dependencies and licenses
cargo deny check
```

### Step 3: Zero-Network Audit
```powershell
powershell -ExecutionPolicy Bypass -File ./scripts/verify_zero_network.ps1
```

### Step 4: Tauri NSIS Installer Bundling
```powershell
npm run tauri build
```
Outputs:
- Standalone Executable: `src-tauri/target/release/SearchMyComputer.exe`
- NSIS Installer: `src-tauri/target/release/bundle/nsis/SearchMyComputer_0.1.0_x64-setup.exe`

---

## 4. Code Signing

Code signing ensures Windows SmartScreen trust and verifies binary integrity.

### Signing with `signtool.exe`
Run the signing helper script:
```powershell
powershell -ExecutionPolicy Bypass -File ./scripts/sign_windows.ps1 `
  -BinaryPath "src-tauri/target/release/bundle/nsis/SearchMyComputer_0.1.0_x64-setup.exe" `
  -CertPath "C:\path\to\certificate.pfx" `
  -CertPassword "YourCertPassword"
```

### Signing with Azure Trusted Signing / Hardware HSM
When using an EV Certificate stored in Azure Key Vault or a Hardware Security Module (YubiKey / SafeNet):
```powershell
signtool.exe sign /v /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /dlib "C:\path\to\dlib.dll" /dmc "C:\path\to\metadata.json" "src-tauri\target\release\SearchMyComputer.exe"
```

---

## 5. Artifact Verification & Checksums

Generate cryptographic hashes for all release artifacts:
```powershell
Get-FileHash -Algorithm SHA256 "src-tauri\target\release\bundle\nsis\SearchMyComputer_0.1.0_x64-setup.exe" | Format-List
```

Store the resulting `SHA256SUMS.txt` alongside release installers.

---

## 6. Release Checklist Reference
See [`docs/RELEASE_CHECKLIST.md`](./RELEASE_CHECKLIST.md) for the mandatory pre-tag verification steps.
