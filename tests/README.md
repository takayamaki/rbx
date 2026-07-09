---
description: Regression tests — rekordbox compatibility invariants and the agent-facing CLI contract
paths:
  - tests/**
---

# tests/

Regression tests.
Most of them lock in facts about rekordbox that we learned the hard way:
records written in the wrong shape do not error — they are **silently ignored by rekordbox**.
Tests are the only guard against that class of bug.

## Files

| File | What it guards |
|---|---|
| `common/mod.rs` | Fixture builder. Creates an unencrypted SQLite db with the rekordbox-shaped schema (11 tables, seeded agentRegistry / content / artist / key / mytags / playlist). `db.rs` falls back to unencrypted, so tests need no SQLCipher key. |
| `invariants_test.rs` | Native-format invariants: timestamp format `%Y-%m-%d %H:%M:%S%.3f +00:00`, numeric 28-bit IDs (>= 100), USN allocation from `agentRegistry.localUpdateCount` (rows never exceed the counter). |
| `playlist_xml_test.rs` | `masterPlaylists6.xml` editing: NODE lines are added/removed without touching any other byte of the file; `Id` matching is exact (not fooled by `ParentId`). |
| `cli_contract_test.rs` | The agent-facing contract, tested through the real binary (`assert_cmd`): JSON envelope shape, semantic exit codes, dry-run defaults, full column set on INSERT, read-only `query` allowlist. |

## Why these invariants matter

rekordbox checks more than the SQL schema.
A row is invisible to it when:

- `created_at` / `updated_at` are not in the exact native format (milliseconds + ` +00:00`)
- the row ID of djmdPlaylist / djmdMyTag / djmdCue / djmdArtist is not a numeric 28-bit value
  (`masterPlaylists6.xml` stores playlist IDs in hex)
- `rb_local_usn` is larger than `agentRegistry.localUpdateCount`
- a playlist is missing from `masterPlaylists6.xml`

## Rules

- Every new mutation command must add a `cli_contract_test.rs` case
  that checks the inserted row has the full native column set
  (UUID, `rb_*` fields, USN within the counter, native timestamps).
- Tests must not require SQLCipher or a real master.db. Use the fixture builder.
- Keep tests independent: each test builds its own db in a tempdir.
