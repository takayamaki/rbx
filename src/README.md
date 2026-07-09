---
description: Rust source code — CLI entry point, DB connection, structured output helpers
paths:
  - src/**
---

# src/

Rust source code.

## File layout

| File | Role |
|---|---|
| `main.rs` | CLI definition (clap), subcommand dispatch, handler implementations, `describe` schema definitions |
| `db.rs` | master.db connection. Auto-decrypts SQLCipher; falls back to unencrypted DB (for testing) |
| `output.rs` | Structured JSON output helpers. Envelope builders (success/error/mutation) and exit code constants |

## Conventions

- Separate DB row structs (`TrackRow`, etc.) from JSON output. Use `FromRow` for DB types, `to_json()` for output conversion (e.g. BPM /100)
- Extract SQL query strings into constants (`TRACK_QUERY_BASE`, etc.) and reuse across handlers
- All handlers return `(serde_json::Value, i32)`. `main` does the single print
- Errors go through `output::error()` for structured output. No `eprintln!` or `panic!`
