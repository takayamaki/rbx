# rbx project rules

See [`rules/repository-overview.md`](rules/repository-overview.md) (symlink to `README.md`) for the project overview.
This file only has rules that are not in the README.

## Language rule

All documents in this repository — including commit messages — must be in **simple, easy English**.
The only exception is `README.ja.md` (Japanese).
Code comments, identifiers, and CLI `--help` text are also in simple, easy English.

## Semantic line breaks

Write prose one sentence per line.
Do not hard-wrap at a fixed column width.
A long sentence may break into a few lines at natural clause boundaries.
This keeps diffs readable and lets sentences move as units.

## Directory READMEs are also Claude rules

Every directory README doubles as a rules file for agents:

1. Start it with frontmatter: a one-line `description:` and `paths:` globs covering the directory.
2. Symlink it into `.claude/rules/` (e.g. `.claude/rules/src.md -> ../../src/README.md`).

When you add a new directory with a README, do both.

## Commit message rules

- **Line 1 (summary):** What changed and how, in one short line.
- **Line 3+ (detail):** Why the change was made.
  Do not repeat what changed — the diff shows that.

## Design principles

Agent-first CLI design (reference: [Zenn article](https://zenn.dev/assign/articles/b3d1d07d385b87)):

1. **Structured output**: stdout is JSON envelope only. Logs go to stderr
2. **Semantic exit codes**: 0/1/2/3/4/5
3. **Noun-Verb grammar**: `rbx <resource> <action> [args]`
4. **Self-describing via `describe`**: returns schema, flags, and examples as JSON
5. **Actionable errors**: `category` + `message` + `next_step`
6. **Mutations default to dry-run**: `--execute` to apply. Dry-run output includes `next_step` hint

## Dependencies

- SQLCipher: `libsqlite3-sys` with `bundled-sqlcipher-vendored-openssl` (static link)
- CLI: `clap` (derive)
- Async: `tokio`
- Serialization: `serde` + `serde_json`
- ID generation: `uuid` v4
- Timestamps: `chrono` (UTC)
