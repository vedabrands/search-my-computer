# SearchMyComputer — Frequently Asked Questions (FAQ)

---

### Privacy & Network

#### Does SearchMyComputer ever connect to the internet?
**No, never.** SearchMyComputer is built with a hard architectural zero-network guarantee. There are no network calls, telemetry pingbacks, cloud APIs, auto-updater network requests, or analytics scripts in the application.

#### How can I verify that SearchMyComputer does not make network requests?
1. Open Windows Resource Monitor or Sysinternals TCPView and filter by `SearchMyComputer.exe` — you will see zero active TCP/UDP connections.
2. SearchMyComputer enforces a strict Content Security Policy (`connect-src 'self'`) in Tauri that blocks any browser-layer network attempts.
3. You can audit the codebase and run `powershell ./scripts/verify_zero_network.ps1` to inspect crates, packages, and sockets.

#### Where are my files and search indexes stored?
All data (SQLite index, settings, and logs) is stored locally on your hard drive in:
`%LOCALAPPDATA%\SearchMyComputer\`
No files or index metadata are ever uploaded to any cloud server.

---

### Performance & System Impact

#### How much RAM and CPU does SearchMyComputer use?
- **Idle Memory**: Under 150 MB when the launcher is closed and indexing is idle.
- **Search Latency**: Typically under 50 ms (warm queries < 200 ms).
- **Background Indexing**: Uses low-priority worker threads (default 2 threads) that automatically yield when on battery power or high system load.

#### Does SearchMyComputer require a GPU?
**No.** All machine learning models (embedding and vision models) use highly optimized int8 quantized ONNX Runtime running purely on CPU.

---

### Features & Formats

#### What file formats can SearchMyComputer search inside?
- **Documents**: PDF, Word (DOCX), Text, Markdown, RTF.
- **Spreadsheets**: Excel (XLSX, XLS), OpenDocument Spreadsheet (ODS), CSV, TSV.
- **Presentations**: PowerPoint (PPTX).
- **Source Code**: Rust, Python, JavaScript, TypeScript, C, C++, Java, Go, C#, HTML, CSS, JSON, YAML, TOML, SQL, Shell, and more with AST-aware tree-sitter chunking.
- **Images**: PNG, JPEG, WebP, BMP with EXIF metadata, barcode/QR code text decoding, and optional visual semantic search.

#### How do I exclude certain folders (e.g. sensitive files or heavy caches)?
Press `Ctrl+,` to open Settings > Exclusions. Add folder names or glob patterns (e.g. `*.kdbx`, `node_modules`, `D:\PrivateVault`).

---

### Licensing & Evaluation

#### How does the evaluation trial work?
Every new installation includes a 14-day evaluation trial with full feature access. After 14 days, search results are restricted to the top 3 matches until an offline license is activated. Background indexing and file monitoring continue running uninterrupted.

#### How do I activate a commercial license?
When you purchase a license, you receive a license text block or JSON file. Open SearchMyComputer, press `Ctrl+,` (Settings) > License, and paste the license text or upload the file. Activation happens instantly and 100% offline.

#### Can I use my license on an air-gapped machine without internet?
**Yes.** The licensing engine uses Ed25519 digital signatures verified entirely on-device with an embedded public key. Internet access is never needed.
