//! rbx closes master.db before the process exits.
//!
//! master.db is in WAL mode.
//! Only a clean close moves the write-ahead log into master.db and removes master.db-wal.
//! A process that exits while the connection is still open leaves master.db-wal behind with pages in it:
//! `backup create` then refuses as if rekordbox were open,
//! and a copy of master.db alone misses rbx's last write.

mod common;

use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn rbx_cmd(db_path: &Path) -> Command {
    let mut cmd = Command::cargo_bin("rbx").unwrap();
    cmd.env_remove("RBX_DB_PATH");
    cmd.arg("--db").arg(db_path);
    cmd
}

fn wal_path(db_path: &Path) -> PathBuf {
    db_path.with_file_name("master.db-wal")
}

/// After a write, the change is in master.db itself and master.db-wal is gone.
#[tokio::test]
async fn write_command_leaves_no_wal_file() {
    let (db_path, _dir) = common::setup_db().await;
    common::use_wal(&db_path).await;

    rbx_cmd(&db_path)
        .args(["tracks", "update", "101", "--comment", "kept", "--execute"])
        .assert()
        .code(0);

    assert!(!wal_path(&db_path).exists(), "master.db-wal is left behind");
    let pool = common::open_pool(&db_path).await;
    let comment: String = sqlx::query_scalar("SELECT Commnt FROM djmdContent WHERE ID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(comment, "kept");
}

/// A write command that fails (exit code 3) also closes master.db before it exits.
#[tokio::test]
async fn failed_write_command_leaves_no_wal_file() {
    let (db_path, _dir) = common::setup_db().await;
    common::use_wal(&db_path).await;

    rbx_cmd(&db_path)
        .args(["tracks", "update", "999", "--comment", "lost", "--execute"])
        .assert()
        .code(3);

    assert!(!wal_path(&db_path).exists(), "master.db-wal is left behind");
}
