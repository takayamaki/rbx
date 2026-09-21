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
| `main.rs` | Entry point: parse args, open the DB, dispatch to `commands::*` |
| `cli.rs` | CLI definition (clap): `Cli`, `Commands`, per-resource action enums, `needs_write` |
| `rows.rs` | DB row structs (`TrackRow`, `PlaylistRow`, …) and their `to_json()` |
| `commands/mod.rs` | Helpers shared by handlers: `db_error`, `resolve_track_summary`, `resolve_tag_name` |
| `commands/tracks/mod.rs` | `tracks list / get / search / filter`, track SQL constants |
| `commands/tracks/update.rs` | `tracks update`: `TrackFields`, artist / genre / album resolve-or-create |
| `commands/tracks/cues.rs` | `tracks cues list / add / update / delete` |
| `commands/tracks/mytags.rs` | `tracks mytags list / add / remove` |
| `commands/playlists.rs` | `playlists *` (incl. `masterPlaylists6.xml` sync) |
| `commands/mytags.rs` | `mytags *` (tag category / tag CRUD) |
| `commands/history.rs` | `history *` |
| `commands/query.rs` | `query` (read-only SQL allowlist, `--unsafe-write`) |
| `describe.rs` | `describe` schema definitions (resources → actions → flags / output schemas) |
| `db.rs` | master.db connection. Auto-decrypts SQLCipher; falls back to unencrypted DB (for testing) |
| `helpers.rs` | Native-format helpers: timestamps, USN allocation, numeric IDs |
| `output.rs` | Structured JSON output helpers. Envelope builders (success/error/mutation) and exit code constants |
| `playlist_xml.rs` | `masterPlaylists6.xml` node add / remove |

The layout follows the command tree: to change `rbx tracks cues add`, open `commands/tracks/cues.rs`.

## Conventions

- Separate DB row structs (`TrackRow`, etc.) from JSON output. Use `FromRow` for DB types, `to_json()` for output conversion (e.g. BPM /100)
- Extract SQL query strings into constants (`TRACK_QUERY_BASE`, etc.) and reuse across handlers
- All handlers return `(serde_json::Value, i32)`. `main` does the single print
- Errors go through `output::error()` for structured output. No `eprintln!` or `panic!`
