# Architecture

## Overview

SearchMyComputer is a local semantic-search launcher. It indexes files on the user's
machine and provides fast filename + content search through a global-hotkey launcher.

## Crate map

```
src-tauri/          Tauri 2 application shell
├── commands.rs     IPC commands exposed to the React UI
├── hotkey.rs       Global shortcut (Alt+Space) with fallback
├── tray.rs         System tray icon and menu
├── state.rs        Shared AppState (DB, config, job queue)
└── lib.rs          Tauri builder and plugin wiring

crates/
├── smc-core/       Core library — DB, config, scanner, job queue
│   ├── db.rs       Database wrapper (writer mutex + r2d2 read pool)
│   ├── schema.rs   Table/column constants
│   ├── migrate.rs  Versioned SQL migrations
│   ├── config.rs   AppConfig (serde JSON, OS app-data dir)
│   ├── scanner.rs  Parallel directory walker with exclusions
│   ├── jobs.rs     Persistent priority job queue
│   └── error.rs    Shared error types
│
├── smc-search/     Search engines
│   ├── filename.rs FTS5 trigram filename search with ranking
│   └── ranking.rs  Boost helpers (depth, recency, match type)
│
├── smc-extract/    (stub) File content extraction — PDF, DOCX, code, text
├── smc-embed/      (stub) ONNX embedding model inference
└── smc-nlq/        (stub) Natural language query parsing
```

## Data flow

```
User adds folder
  → Scanner walks directory tree (2 threads, parallel)
  → Inserts/updates rows in `files` table
  → Inserts FTS5 entry for each filename
  → Enqueues "extract" jobs for content extraction (Chunk 3)
  → Job workers process queue by priority

User types query
  → FTS5 trigram search on filenames
  → Results ranked: exact > prefix > substring × depth × recency
  → (Chunk 3+) Also searches chunk text FTS + vector similarity
  → Top results returned to UI
```

## SQLite design

- WAL mode for concurrent reads during writes
- Single writer connection (Mutex) — all inserts/updates serialized
- Read pool (4 connections via r2d2) — search queries don't block writes
- Pragmas: synchronous=NORMAL, cache_size=64MB, journal_size_limit=6MB

## Security model

- Zero network requests at runtime (see docs/SECURITY.md)
- Strict CSP: default-src 'self'
- No shell, HTTP, or filesystem plugins beyond core needs
- User data in OS app-data dir with default user-only permissions
- Default exclusions for sensitive directories (.ssh, .gnupg, password managers)
