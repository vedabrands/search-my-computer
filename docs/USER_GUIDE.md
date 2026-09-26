# SearchMyComputer — User Guide

Welcome to **SearchMyComputer**, the fast, 100% private semantic search launcher for Windows.

---

## 1. Quick Start

### Global Launcher Hotkey
Press **`Alt+Space`** anywhere in Windows to toggle the search launcher.

### Searching
Simply type what you are looking for in plain English:
- *"find that quarterly revenue forecast excel sheet"*
- *"where is my rust database connection pool"*
- *"screenshot with wifi password qr"*
- *"nutrigrade presentation pdf from last month"*

---

## 2. Keyboard Shortcuts & Navigation

| Key Combination | Action |
|---|---|
| **`Alt+Space`** | Global hotkey to toggle SearchMyComputer window |
| **`Arrow Down` / `Arrow Up`** | Navigate through search results |
| **`Enter`** | Open highlighted file / project in default OS app |
| **`Ctrl+Enter`** or **`Alt+O`** | Reveal file / project in Windows File Explorer |
| **`Alt+T`** | Open PowerShell terminal in target folder |
| **`Ctrl+C`** | Copy full file path to clipboard |
| **`Ctrl+K`** | Open Context Actions menu |
| **`Ctrl+,`** | Open Settings modal |
| **`Space`** or **`Tab`** | Toggle side-by-side Quick Look preview pane (when query is empty) |
| **`Ctrl+Space`** | Quick image preview modal |
| **`Escape`** | Close modal, preview pane, actions menu, or hide launcher |

---

## 3. Advanced Query Syntax & Filters

SearchMyComputer combines **keyword FTS5 trigrams**, **semantic vector embeddings**, and **metadata extraction** to find your files.

### Natural Language Date Filters
- `in 2026`: Matches files created or modified in year 2026.
- `last month`: Matches files modified in the previous calendar month.
- `yesterday` / `today`: Restricts results to recent files.

### File Type Filters
- `kind:pdf` / `kind:document`: Restricts to PDF, Word (DOCX), Text, or Markdown documents.
- `kind:code`: Restricts to source code files (Rust, Python, TS/JS, C/C++, Java, Go, C#).
- `kind:image`: Restricts to images, photos, and screenshots.
- `kind:sheet`: Restricts to spreadsheets (Excel XLSX/XLS, ODS, CSV).
- `kind:slide`: Restricts to presentations (PowerPoint PPTX).

### Extension Filters
- `ext:rs`, `ext:py`, `ext:tsx`, `ext:docx`, `ext:pdf`

---

## 4. Features & Capabilities

### 4.1 Side-by-Side Quick Look Preview Pane
Press **`Tab`** or **`Space`** to toggle the inline preview panel.
- **Documents & Code**: Syntax-highlighted text preview with line numbers and matched snippet highlighting.
- **Images**: High-resolution thumbnail, camera EXIF metadata, and dimensions.
- **QR Codes**: Detected QR payloads with one-click privacy unmasking and copy-to-clipboard.
- **Projects**: Git branch, commit count, detected tech stack badges, and README snippet.

### 4.2 Actions Menu (`Ctrl+K`)
Quickly access file operations:
- Open with default application
- Reveal in File Explorer
- Launch PowerShell terminal in directory
- Copy full absolute path
- Pause / Resume background indexing
- Open Settings

### 4.3 Settings & Privacy Configuration (`Ctrl+,`)
- **Folders**: Add or remove indexed folders (e.g. `C:\Users\You\Documents`, `D:\Projects`).
- **Exclusions**: Configure custom folder and glob ignore rules.
- **Indexing**: Adjust worker thread count, battery saver mode, and view live status.
- **Models**: Switch embedding models (`bge-small-en-v1.5` or `all-MiniLM-L6-v2`) or toggle CLIP visual search.
- **License**: View evaluation trial status or activate an offline commercial license.
- **About**: View software version, privacy verification guarantee, and export diagnostic logs.

---

## 5. Offline Licensing & Evaluation

- **14-Day Free Evaluation**: Full access to all features upon installation.
- **Permanent Activation**: Enter your offline license JSON in Settings > License or through the trial banner to unlock permanent lifetime or subscription access.
- **Zero-Network Guarantee**: Activating a license requires no internet connection.

---

## 6. Privacy & Data Storage

All data created by SearchMyComputer lives strictly on your PC in:
`%LOCALAPPDATA%\SearchMyComputer\`
- `index.db`: Local SQLite database containing document chunks and vector index.
- `config.json`: Application settings and folder paths.
- `logs/`: Rotating local diagnostics log files.

You can purge all indexed data at any time via Settings > Indexing > Reindex / Purge.
