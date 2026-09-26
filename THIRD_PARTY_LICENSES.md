# Third-Party Licenses

All direct dependencies in SearchMyComputer use commercial-friendly open source licenses.
GPL, AGPL, LGPL (without explicit approval), SSPL, and non-commercial licenses are strictly forbidden.

## Rust Crates (Direct Dependencies)

| Crate | Version | License | Purpose |
|---|---|---|---|
| `tauri` | 2.x | MIT OR Apache-2.0 | App shell, window management, tray |
| `tauri-build` | 2.x | MIT OR Apache-2.0 | Tauri build-time code generation |
| `tauri-plugin-global-shortcut` | 2.x | MIT OR Apache-2.0 | Global hotkey (Alt+Space) registration |
| `tauri-plugin-opener` | 2.x | MIT OR Apache-2.0 | Opening files and revealing in file explorer |
| `rusqlite` | 0.32 | MIT | SQLite database with FTS5 and trigram tokenizer |
| `r2d2` | 0.8 | MIT OR Apache-2.0 | Connection pooling for read queries |
| `r2d2_sqlite` | 0.25 | MIT | SQLite adapter for r2d2 |
| `walkdir` | 2.x | Unlicense OR MIT | Fast recursive directory traversal |
| `globset` | 0.4 | MIT OR Apache-2.0 | Glob-based file/folder exclusion matching |
| `serde` | 1.x | MIT OR Apache-2.0 | Serialization framework |
| `serde_json` | 1.x | MIT OR Apache-2.0 | JSON config file serialization |
| `tracing` | 0.1 | MIT | Structured diagnostics and logging |
| `tracing-subscriber` | 0.3 | MIT | Log formatting and filtering |
| `tracing-appender` | 0.2 | MIT | Daily rotating file logging |
| `directories` | 5.x | MIT OR Apache-2.0 | Standard OS app-data directories |
| `chrono` | 0.4 | MIT OR Apache-2.0 | Timestamp formatting and comparison |
| `uuid` | 1.x | MIT OR Apache-2.0 | Unique identifier generation |
| `thiserror` | 2.x | MIT OR Apache-2.0 | Error derivation |
| `parking_lot` | 0.12 | MIT OR Apache-2.0 | Fast mutexes and synchronization primitives |
| `crossbeam-channel` | 0.5 | MIT OR Apache-2.0 | Multi-producer multi-consumer channels |
| `tempfile` | 3.x | MIT OR Apache-2.0 | Temporary directory creation for tests (dev-only) |
| `pdfium-render` | 0.8 | Apache-2.0 | PDFium bindings for PDF text extraction |
| `zip` | 2.x | MIT | DOCX, PPTX, XLSX ZIP decompression |
| `quick-xml` | 0.37 | MIT | High-performance XML parser for OpenXML documents |
| `calamine` | 0.26 | MIT | Spreadsheet (XLSX, XLS, ODS) parsing |
| `encoding_rs` | 0.8 | Apache-2.0 OR MIT OR BSD-3-Clause | Encoding detection and conversion for text files |
| `content_inspector` | 0.2 | Apache-2.0 OR MIT | Binary vs. plain-text content sniffing |
| `tree-sitter` | 0.24 | MIT | AST-aware code chunking parser framework |
| `tree-sitter-rust` | 0.23 | MIT | Rust grammar for tree-sitter |
| `tree-sitter-python` | 0.23 | MIT | Python grammar for tree-sitter |
| `tree-sitter-javascript` | 0.23 | MIT | JavaScript grammar for tree-sitter |
| `tree-sitter-typescript` | 0.23 | MIT | TypeScript grammar for tree-sitter |
| `tree-sitter-c` | 0.23 | MIT | C grammar for tree-sitter |
| `tree-sitter-cpp` | 0.23 | MIT | C++ grammar for tree-sitter |
| `tree-sitter-java` | 0.23 | MIT | Java grammar for tree-sitter |
| `tree-sitter-c-sharp` | 0.23 | MIT | C# grammar for tree-sitter |
| `tree-sitter-go` | 0.23 | MIT OR Apache-2.0 | Go grammar for tree-sitter |
| `ort` | 2.0.0-rc.9 | Apache-2.0 OR MIT | ONNX Runtime CPU execution engine |
| `tokenizers` | 0.21 | Apache-2.0 | Hugging Face tokenization engine |
| `sha2` | 0.10 | MIT OR Apache-2.0 | SHA-256 chunk text hashing |
| `half` | 2.4 | MIT OR Apache-2.0 | f16 half-precision floating-point vector storage |
| `image` | 0.25 | MIT OR Apache-2.0 | Image decoding (PNG, JPEG, WebP, BMP), resizing, and thumbnailing |
| `rxing` | 0.5 | Apache-2.0 | Pure-Rust, 100% permissively licensed 1D/2D barcode & QR code reader |
| `kamadak-exif` | 0.5 | BSD-2-Clause | EXIF metadata reader for image dates, camera make/model |
| `qrcode` | 0.14 | MIT OR Apache-2.0 | QR code generation for unit & integration test fixtures (dev-only) |
| `toml` | 0.8 | MIT OR Apache-2.0 | TOML config/query parsing for evaluation benchmarks |
| `sysinfo` | 0.33 | MIT | Process memory measurement for embedding benchmarks (dev-only) |
| `notify` | 6.1 | CC0-1.0 OR Apache-2.0 | Cross-platform recursive filesystem event watching |
| `windows-sys` | 0.59 | MIT OR Apache-2.0 | Windows system power, input idle, and thread priority APIs |
| `ed25519-dalek` | 2.x | BSD-3-Clause | Offline Ed25519 asymmetric cryptographic license verification |
| `hmac` | 0.12 | MIT OR Apache-2.0 | HMAC-SHA256 SQLite trial integrity sealing |
| `hex` | 0.4 | MIT OR Apache-2.0 | Hexadecimal encoding/decoding for signatures and hashes |
| `rand` | 0.8 | MIT OR Apache-2.0 | Cryptographic random number generation for licensing tools (dev/build-only) |
| `clap` | 4.x | MIT OR Apache-2.0 | Command-line argument parsing for license-tool (dev/build-only) |
| `base64` | 0.22 | MIT OR Apache-2.0 | Base64 decoding utilities (dev/build-only) |
| `csv` | 1.x | Unlicense OR MIT | CSV batch license generation support (dev/build-only) |
| `cpal` | 0.15 | Apache-2.0 | Cross-platform audio input stream capturing |
| `hound` | 3.5 | Apache-2.0 | WAV audio file parsing and serialization for voice test fixtures |

## Embedding, Vision, and Voice Models (Fetched at Dev/Build Time via `models.lock`)

| Model | Source | License | Dimension | Purpose |
|---|---|---|---|---|
| `bge-small-en-v1.5` | BAAI | Apache-2.0 | 384 | Default int8 quantized local semantic embedding model |
| `all-MiniLM-L6-v2` | sentence-transformers | Apache-2.0 | 384 | Lite int8 quantized local semantic embedding model |
| `clip-vit-base-patch32` (Visual & Text) | OpenAI / HuggingFace | MIT | 512 | Optional local visual semantic search & cross-modal retrieval pack |
| `openWakeWord` (Embedding & Heads) | dscherec / openWakeWord | Apache-2.0 | 96 / 1 | Always-on continuous wake-word detector (<5% CPU) |
| `whisper-tiny.en` (Encoder & Decoder) | OpenAI / ONNX Community | MIT | 384 | Default local speech-to-text transcription model (int8, ~40 MB) |
| `whisper-base.en` (Encoder & Decoder) | OpenAI / ONNX Community | MIT | 512 | Fallback high-accuracy speech-to-text transcription model (int8, ~75 MB) |


## Frontend Packages (npm Direct Dependencies)

| Package | Version | License | Purpose |
|---|---|---|---|
| `react` | 19.x | MIT | UI component library |
| `react-dom` | 19.x | MIT | React DOM renderer |
| `@tauri-apps/api` | 2.x | MIT OR Apache-2.0 | Tauri IPC bridge (invoke, listen) |
| `@tauri-apps/plugin-global-shortcut` | 2.x | MIT OR Apache-2.0 | Frontend shortcut bindings |
| `@tauri-apps/plugin-opener` | 2.x | MIT OR Apache-2.0 | Frontend file open/reveal bindings |
| `typescript` | 5.x | Apache-2.0 | Type checking (dev-only) |
| `vite` | 6.x | MIT | Frontend bundler and dev server (dev-only) |
| `@vitejs/plugin-react` | 4.x | MIT | React plugin for Vite (dev-only) |
| `@tauri-apps/cli` | 2.x | MIT OR Apache-2.0 | Tauri CLI build tooling (dev-only) |

## Enforcement

License compliance is checked automatically via `cargo deny check licenses` using `deny.toml`.
Any dependency with an incompatible license will cause the build to fail.
