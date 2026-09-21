---
description: Project overview — what rbx is, how to build and use it
paths:
  - "**"
---

# rbx

A CLI tool for querying and manipulating rekordbox's master.db.
Designed primarily for coding agents (Claude Code, etc.) — agent-first, not human-first.

[日本語版は README.ja.md](README.ja.md)

## ⚠️ Back up your library first

rbx writes directly to your rekordbox master.db.
Before you run any `--execute` command, **make a backup of your rekordbox library**.
rekordbox can do this from Preferences → Advanced → Database → Library backup.

This software comes with **no warranty of any kind** (see [License](#license)).
If you run it without a backup and your library breaks, that is on you.

## Install

Download a build for your platform from the [Releases page](../../releases):

| Platform | Asset |
|---|---|
| Linux / WSL (x86-64) | `rbx-<version>-x86_64-unknown-linux-gnu.tar.gz` |
| macOS (Intel) | `rbx-<version>-x86_64-apple-darwin.tar.gz` |
| macOS (Apple Silicon) | `rbx-<version>-aarch64-apple-darwin.tar.gz` |

```sh
tar xzf rbx-<version>-<target>.tar.gz
# the binary is inside; move it onto your PATH
```

Verify the download against `SHA256SUMS.txt` on the same release.
There is no Windows-native build: run it under WSL.

Or build it yourself (see [Building](#building)).

## Design principles

- **Noun-Verb subcommand grammar**: `rbx tracks list`, `rbx tracks mytags add`, etc.
- **Structured JSON output**: every response wrapped in a `schema_version` / `kind` / `items|item|error` envelope
- **Self-describing via `describe`**: drill down from resources → actions → flags and output schemas
- **Semantic exit codes**: 0=success, 1=general error, 2=usage error, 3=not found, 4=config error, 5=conflict
- **Actionable errors**: `category` + `message` + `next_step` guide recovery
- **Mutations default to dry-run**: pass `--execute` to apply; dry-run output includes the `next_step` hint

Reference: [AI エージェントに自作CLIを効果的に使わせるための8原則](https://zenn.dev/assign/articles/b3d1d07d385b87)

## Building

```sh
cargo build --release
```

The first build takes a while because SQLCipher is statically linked.

## Command reference

```sh
# Set DB path via --db or environment variable
export RBX_DB_PATH=/path/to/master.db
```

### Schema discovery (no DB needed)

```sh
rbx describe                           # list resources
rbx describe tracks                    # list actions on tracks
rbx describe tracks filter             # flags, output schema, examples
```

### tracks

```sh
rbx tracks list                        # all tracks (excludes streaming-only)
rbx tracks get <id>                    # single track by ID
rbx tracks search 'query'             # search by title or artist
rbx tracks filter --bpm-min 125 --bpm-max 135 --key 8A --tag TAG_ID
                                       # filter by BPM range, key, and/or tag
rbx tracks update <id> --title '...' --artist '...' --bpm 128.0 \
    --key 8A --rating 4 --comment '...'
                                       # update track fields (dry-run)
rbx tracks update <id> --genre '...' --album '...' \
    --track-no 3 --disc-no 1 --year 2018
                                       # artist / genre / album are resolved by name
                                       # (rows created when missing; "" clears the column)
rbx tracks update <id> --path 'F:/Music/new name.m4a'
                                       # file moved on disk: rewrite FolderPath (FileNameL follows)
rbx tracks update <id> --bpm 130.0 --execute
                                       # update track fields (apply)
```

### tracks cues

```sh
rbx tracks cues list <track_id>        # list MEMORY/HOT cues on a track
rbx tracks cues add <track_id> 12345   # add MEMORY cue at 12345ms (dry-run)
rbx tracks cues add <track_id> 92000 --kind hot --slot 1 --comment 'Drop'
                                       # add HOT cue in slot 1 (dry-run)
rbx tracks cues update <cue_id> --msec 15000 --comment 'Verse'
                                       # update cue position/comment (dry-run)
rbx tracks cues delete <cue_id>        # delete a cue (dry-run)
```

### tracks mytags

```sh
rbx tracks mytags list <track_id>      # list tags assigned to a track
rbx tracks mytags add <track_id> <tag_id>
                                       # tag a track (dry-run)
rbx tracks mytags remove <track_id> <tag_id>
                                       # untag a track (dry-run)
```

### playlists

```sh
rbx playlists list                     # all playlists and folders
rbx playlists tracks list <playlist_id>
                                       # tracks in a playlist
rbx playlists tracks add <playlist_id> <track_id>
                                       # add track to playlist (dry-run)
rbx playlists tracks remove <playlist_id> <track_id>
                                       # remove track from playlist (dry-run)
rbx playlists search <track_id>        # find playlists containing a track
rbx playlists create 'name' --parent FOLDER_ID
                                       # create playlist (dry-run)
rbx playlists delete <id>              # delete playlist (dry-run)
```

### mytags

```sh
rbx mytags list                        # all categories and tags
rbx mytags tracks <tag_id>             # tracks with a specific tag
rbx mytags create 'name'              # create top-level category (dry-run)
rbx mytags create 'name' --parent CATEGORY_ID
                                       # create tag under category (dry-run)
rbx mytags delete <id>                 # delete a tag (dry-run)
```

### history

```sh
rbx history list                       # recent play sessions (default: 20)
rbx history list --limit 5             # limit results
rbx history tracks <session_id>        # tracks in a session (play order)
```

### raw SQL

```sh
rbx query 'SELECT ID, Title FROM djmdContent LIMIT 5'
rbx query 'PRAGMA table_info(djmdContent)'
```

`query` is read-only: a single SELECT / WITH / PRAGMA / EXPLAIN statement.
Writes require `--unsafe-write`,
which bypasses the rekordbox invariants the dedicated commands maintain
(USN allocation, timestamp format, numeric IDs, masterPlaylists6.xml sync).
Prefer the dedicated commands.

All mutations above show `(dry-run)`.
They preview the change and print `"next_step": "Add --execute to apply"`.
Append `--execute` to actually write.

## For agents

Start with `rbx describe` to discover available resources, actions, flags, and output schemas.
All output is JSON, so pipe through `jq` for further processing.

## About rekordbox master.db

rekordbox 6+ encrypts master.db with SQLCipher.
rbx decrypts it automatically using the known fixed password — no user action needed.

## License

MIT. See [LICENSE](LICENSE).

The software is provided "as is", without warranty of any kind.
The authors are not liable for any damage to your rekordbox library or data.
Keep backups.
