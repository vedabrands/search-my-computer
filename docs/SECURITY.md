# SearchMyComputer — Security Architecture & Threat Model

SearchMyComputer is designed with a **privacy-first, zero-network, local-only** security model. Because the application indexes personal documents, code, images, and system files, it must maintain rigorous isolation from network egress, robust defenses against malformed files, and strong integrity protections against local tampering.

---

## 1. Zero-Network Architecture

SearchMyComputer operates with **zero runtime network requests**. No data, metadata, search queries, telemetry, crash reports, or license verification pings are ever transmitted over a network.

### Architectural Controls
1. **Tauri Content Security Policy (CSP)**:
   ```json
   "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'"
   ```
   Blocks all remote asset loading, external network fetches, WebSockets, and third-party scripts.
2. **Elimination of Network Plugins**:
   - `tauri-plugin-http` is omitted.
   - `tauri-plugin-updater` is omitted (updates are manual offline downloads verified by the user).
   - `tauri-plugin-shell` is omitted (only the tightly scoped `@tauri-apps/plugin-opener` is allowed for OS-default file launching).
3. **Zero Telemetry / Zero Analytics**:
   No tracking SDKs, pingbacks, or anonymous usage metrics exist in the codebase.
4. **Bundled Offline Models**:
   All embedding and vision models (`all-MiniLM-L6-v2`, `clip-vit-base-patch32`) are pre-converted to int8 ONNX and packaged at build time. The running binary never downloads models at runtime.
5. **Dependency Audit (`cargo-deny`)**:
   `deny.toml` strictly enforces license compliance and bans unauthorized crates or git dependencies.

---

## 2. Threat Model & Safeguards

### 2.1 Untrusted & Malformed Files (Fuzzing & Zip-Bomb Defenses)
When indexing user directories, the extractor processes arbitrary files that may be corrupt, truncated, or maliciously constructed.

- **Size & Memory Caps**:
  - Max text file size: 50 MB (configurable).
  - Max extraction memory per document: bounded chunk buffers.
  - Quick Look preview cap: max 10 MB file size, max 200 lines, max 32 KB preview buffer.
- **Decompression Bomb Protection**:
  - OpenXML extractors (`docx`, `pptx`, `xlsx`) stream XML parsing via `quick-xml` and enforce strict uncompressed byte limits to prevent zip bombs from consuming system memory.
- **Crash Isolation (`catch_unwind`)**:
  - All file extractors (PDF via `pdfium-render`, Office via `quick-xml`, images via `image`/`kamadak-exif`/`rxing`) execute inside `std::panic::catch_unwind(std::panic::AssertUnwindSafe(...))` boundaries. A corrupt file cannot crash the background worker or launcher.
- **Poison Pill Quarantine**:
  - Files that repeatedly fail extraction are backed off exponentially and placed into a quarantined `problem_files` table after 3 failed attempts, preventing infinite crash/retry loops.

### 2.2 Search & SQL Injection Prevention
Search queries accept arbitrary natural language input from users.

- **100% Parameterized SQLite Queries**:
  - All SQLite statements use `rusqlite::params![]` or cached prepared statements. No user input is ever concatenated into raw SQL strings.
- **Strict FTS5 Query Sanitization**:
  - For trigram searches (`files_fts`, `projects_fts`): double-quotes are escaped (`" -> ""`) and wrapped in phrase quotes (`"query"`).
  - For content searches (`chunks_fts`): `sanitize_fts5_query()` strips all non-alphanumeric/operator tokens (such as `NOT`, `AND`, `OR`, `*`, `^`, `{}`) and encapsulates individual terms in quoted prefix wildcards (`"term"*`).

### 2.3 OS Command Injection & Path Traversal Safeguards
The application provides convenience actions to open files, reveal items in File Explorer, or launch terminal sessions.

- **Path Existence & Directory Validation**:
  - `reveal_in_folder` and `open_terminal_in_folder` verify `Path::exists()` and confirm directory boundaries before invoking OS handlers.
- **Literal Path Parameterization**:
  - Windows terminal execution uses `-LiteralPath` with escaped single-quote strings (`' -> ''`), preventing PowerShell parameter expansion or command injection.
- **No Path Manipulation via IPC**:
  - User file paths are treated as opaque strings pointing to existing filesystem nodes and are never executed directly via shells.

### 2.4 Offline Licensing & Anti-Tampering Defenses
SearchMyComputer uses an offline commercial licensing model that must resist unauthorized modification without phoning home.

- **Asymmetric Ed25519 Cryptographic Signatures**:
  - Licenses are signed by the vendor using a private key and validated locally using an embedded 32-byte Ed25519 public key in `smc-license`.
  - Signatures cover the canonical tuple `PRODUCT_NAME | MAJOR_VERSION | customer_name | license_id | issued_epoch | expires_epoch`.
- **HMAC-SHA256 SQLite Integrity**:
  - Trial start timestamps and last-seen timestamps in SQLite `meta` are sealed with an internal HMAC-SHA256 signature (`trial_tamper_hash`).
  - Any direct modification of SQLite metadata invalidates the hash and immediately transitions the trial to expired state.
- **Monotonic Clock Rollback Detection**:
  - If the system clock is rolled back (`now < trial_start` or `now < last_seen - 60s`), the application detects clock manipulation and expires the evaluation trial.
- **Search Gating**:
  - Expired trials are gracefully restricted to read-only top 3 search results without breaking the launcher UI or corrupting local indexes.

### 2.5 Credential & Sensitive Data Protection
- **Default Exclusions**:
  The scanner automatically excludes sensitive directories and credential stores:
  - Version control & build artifacts: `.git`, `node_modules`, `target`, `dist`, `build`, `.cache`, `temp`
  - Credentials & keys: `.ssh`, `.gnupg`, `id_rsa`, `id_ed25519`, `*.pem`, `*.key`, `*.kdbx`, `*.keystore`
  - Browser profiles & secrets: `AppData/Local/Google/Chrome/User Data`, `AppData/Roaming/Mozilla/Firefox/Profiles`
- **QR Code Privacy Masking**:
  - Extracted sensitive QR payloads (e.g. payment UPI VPAs, EMVCo strings, Wi-Fi passwords) are masked by default in the UI (`upi://pay?pa=me***t@okaxis`) with an explicit "Click to reveal" toggle. Payloads are never logged at any log level.

### 2.6 User Isolation & OS Permissions
- **Non-Elevated Execution**:
  - The application and its NSIS installer run with `currentUser` privileges. Administrative elevation (`UAC`) is never requested or required.
- **Per-User AppData Storage**:
  - SQLite databases (`index.db`), configuration files (`config.json`), and rotating logs are stored in `%LOCALAPPDATA%\SearchMyComputer\` with Windows ACLs restricted to the current user profile.

---

## 3. Automated Zero-Network Verification

Run the verification test suite to ensure no network sockets or network dependencies are present:

```powershell
# 1. Verify zero network-capable crates and valid license configurations
cargo deny check

# 2. Run automated zero-network binary inspection
powershell -ExecutionPolicy Bypass -File ./scripts/verify_zero_network.ps1
```

---

## 4. Reporting Security Issues

If you discover a security vulnerability or potential data leak in SearchMyComputer, please report it privately:
- Email: **security@searchmycomputer.app**
- PGP Key: Available upon request

All valid security reports will be acknowledged within 24 hours.
