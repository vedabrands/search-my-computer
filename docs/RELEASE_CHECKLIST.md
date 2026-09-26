# SearchMyComputer — Release Pre-Flight Checklist

Before tagging, signing, or distributing any release of SearchMyComputer, verify every item below:

---

## 1. Codebase & Compliance Audits
- [ ] **Zero-Network Audit**: Run `verify_zero_network.ps1` and verify all 4 checks PASS with zero banned crates/packages.
- [ ] **License Audit**: Run `cargo deny check licenses` and verify zero unapproved or copyleft licenses.
- [ ] **Third-Party Disclosures**: Verify `THIRD_PARTY_LICENSES.md` is updated with all current dependencies and models.
- [ ] **Model Integrity**: Verify all bundled ONNX models match hashes in `models.lock`.
- [ ] **Clean Lint**: `cargo clippy --workspace --all-targets -- -D warnings` completes with 0 warnings.
- [ ] **Clean Formatting**: `cargo fmt --all -- --check` completes with 0 diffs.
- [ ] **Unit & Integration Tests**: `cargo test --workspace` passes 100% across all crates.
- [ ] **Frontend Build**: `npm run build` succeeds without TypeScript errors or warnings.

---

## 2. Security & Hardening Checks
- [ ] **Content Security Policy**: Verify `src-tauri/tauri.conf.json` contains strict `default-src 'self'; connect-src 'self'`.
- [ ] **Non-Elevated Installer**: Verify `nsis.installMode` is set to `"currentUser"`.
- [ ] **No Secrets**: Verify no private keys (`private_key.hex`), PFX certificates, or temporary tokens are checked into git.
- [ ] **Command Injection**: Verify all Windows terminal / explorer launchers use `-LiteralPath` with escaped quotes.
- [ ] **FTS5 Sanitization**: Verify all search paths sanitize user queries against SQL / FTS5 syntax errors.

---

## 3. Packaging & Verification
- [ ] **Installer Generation**: `npm run tauri build` builds cleanly on Windows x64.
- [ ] **Clean VM Install Test**:
  - [ ] Install setup `.exe` in a clean Windows 10/11 sandbox.
  - [ ] Verify installer does NOT request administrative elevation (UAC prompt).
  - [ ] Verify hotkey `Alt+Space` opens the launcher immediately.
  - [ ] Verify onboarding wizard steps through index folder selection.
  - [ ] Verify indexing completes and search returns expected files and thumbnails.
- [ ] **Trial & Licensing Verification**:
  - [ ] Verify initial installation begins a 14-day full evaluation trial.
  - [ ] Verify activating a valid Ed25519 `license.json` unlocks permanent licensed status.
  - [ ] Verify tampered trial timestamps gracefully restrict search to read-only top 3 results.
- [ ] **Code Signing**: Sign binary and setup `.exe` with valid Authenticode certificate + RFC 3161 timestamp.
- [ ] **SHA256 Hashes**: Generate and publish `SHA256SUMS.txt`.
