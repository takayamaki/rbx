//! rbx closes master.db before the process exits.
//!
//! master.db is in WAL mode.
//! Only a clean close moves the write-ahead log into master.db and removes master.db-wal.
//! A process that exits while the connection is still open leaves master.db-wal behind with pages in it:
//! `backup create` then refuses as if rekordbox were open,
//! and a copy of master.db alone misses rbx's last write.

mod common;

/// After a write, the change is in master.db itself and master.db-wal is gone.
#[tokio::test]
async fn write_command_leaves_no_wal_file() {}

/// A write command that fails (exit code 3) also closes master.db before it exits.
#[tokio::test]
async fn failed_write_command_leaves_no_wal_file() {}
