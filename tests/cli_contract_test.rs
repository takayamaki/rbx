mod common;

use assert_cmd::Command;

fn rbx_cmd(db_path: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("rbx").unwrap();
    cmd.env_remove("RBX_DB_PATH");
    cmd.arg("--db").arg(db_path);
    cmd
}

fn stdout_json(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    serde_json::from_slice(&assert.get_output().stdout).unwrap()
}

fn ts_regex() -> regex::Regex {
    regex::Regex::new(r"^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}\.\d{3} \+00:00$").unwrap()
}

/// A created playlist must satisfy every native-format requirement at once:
/// numeric ID, separate UUID, ParentID "root", USN within the agentRegistry
/// counter, native timestamp format, and zeroed rb_ status fields.
#[tokio::test]
async fn playlists_create_writes_native_format_row() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args(["playlists", "create", "AGENT TEST", "--execute"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);

    assert_eq!(json["kind"], "playlists.create");
    assert_eq!(json["dry_run"], false);
    let id = json["result"]["id"].as_str().unwrap();
    assert!(id.chars().all(|c| c.is_ascii_digit()), "non-numeric ID: {}", id);

    let pool = common::open_pool(&db_path).await;
    let row: (String, String, i64, i64, String, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT UUID, ParentID, Seq, rb_local_usn, created_at, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced \
         FROM djmdPlaylist WHERE ID = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let (uuid, parent_id, seq, usn, created_at, ds, lds, ld, ls) = row;

    assert!(!uuid.is_empty());
    assert_ne!(uuid, id, "UUID must be distinct from the row ID");
    assert_eq!(parent_id, "root");
    assert!(seq >= 1);
    assert!(ts_regex().is_match(&created_at), "bad timestamp: {}", created_at);
    assert_eq!((ds, lds, ld, ls), (0, 0, 0, 0));

    let (counter,): (i64,) = sqlx::query_as(
        "SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(usn <= counter, "row USN {} exceeds counter {}", usn, counter);
}

#[tokio::test]
async fn tracks_mytags_add_allocates_usn_and_uuid() {
    let (db_path, _dir) = common::setup_db().await;

    rbx_cmd(&db_path)
        .args(["tracks", "mytags", "add", "101", "402", "--execute"])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let (uuid, usn, created_at): (String, i64, String) = sqlx::query_as(
        "SELECT UUID, rb_local_usn, created_at FROM djmdSongMyTag \
         WHERE ContentID = '101' AND MyTagID = '402'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    assert!(!uuid.is_empty());
    assert_eq!(usn, 1001, "first allocation from seeded counter 1000");
    assert!(ts_regex().is_match(&created_at), "bad timestamp: {}", created_at);
}

/// Bulk adds must allocate contiguous USNs and leave the counter equal to
/// the highest row USN — rows beyond the counter are invisible to rekordbox.
#[tokio::test]
async fn bulk_mytags_add_keeps_usns_contiguous_with_counter() {
    let (db_path, _dir) = common::setup_db().await;

    rbx_cmd(&db_path)
        .args(["tracks", "mytags", "add", "102", "402", "403", "--execute"])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let usns: Vec<(i64,)> = sqlx::query_as(
        "SELECT rb_local_usn FROM djmdSongMyTag WHERE ContentID = '102' ORDER BY rb_local_usn",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    let usns: Vec<i64> = usns.into_iter().map(|(u,)| u).collect();
    assert_eq!(usns, vec![1001, 1002], "contiguous block from seeded counter");

    let (counter,): (i64,) = sqlx::query_as(
        "SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counter, *usns.last().unwrap());
}

#[tokio::test]
async fn not_found_returns_exit_3_with_actionable_error() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "get", "999999999"])
        .assert()
        .code(3);
    let json = stdout_json(&assert);

    assert_eq!(json["kind"], "error");
    assert_eq!(json["error"]["category"], "not_found");
    assert_eq!(json["error"]["exit_code"], 3);
    assert!(json["error"]["next_step"].is_string());
}

#[tokio::test]
async fn missing_db_returns_exit_4_config_error() {
    let mut cmd = Command::cargo_bin("rbx").unwrap();
    let assert = cmd
        .env_remove("RBX_DB_PATH")
        .args(["tracks", "list"])
        .assert()
        .code(4);
    let json = stdout_json(&assert);

    assert_eq!(json["kind"], "error");
    assert_eq!(json["error"]["category"], "config");
}

#[tokio::test]
async fn mutations_default_to_dry_run() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args(["playlists", "create", "DRYRUN-PL"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);

    assert_eq!(json["dry_run"], true);
    assert!(
        json["next_step"].as_str().unwrap().contains("--execute"),
        "dry-run must point at --execute"
    );

    let pool = common::open_pool(&db_path).await;
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM djmdPlaylist WHERE Name = 'DRYRUN-PL'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0, "dry-run must not write");
}

#[tokio::test]
async fn query_is_read_only_by_default() {
    let (db_path, _dir) = common::setup_db().await;

    // SELECT and PRAGMA pass
    rbx_cmd(&db_path)
        .args(["query", "SELECT COUNT(*) AS c FROM djmdContent"])
        .assert()
        .code(0);
    rbx_cmd(&db_path)
        .args(["query", "PRAGMA table_info(djmdContent)"])
        .assert()
        .code(0);

    // Writes are rejected with an actionable error, nothing is written
    let assert = rbx_cmd(&db_path)
        .args(["query", "UPDATE djmdContent SET Title = 'pwned'"])
        .assert()
        .code(2);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "error");
    assert_eq!(json["error"]["category"], "readonly");
    assert!(json["error"]["next_step"]
        .as_str()
        .unwrap()
        .contains("--unsafe-write"));

    // Multi-statement input is rejected even when it starts with SELECT
    rbx_cmd(&db_path)
        .args(["query", "SELECT 1; DROP TABLE djmdContent"])
        .assert()
        .code(2);

    let pool = common::open_pool(&db_path).await;
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM djmdContent WHERE Title = 'pwned'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
async fn query_unsafe_write_allows_writes() {
    let (db_path, _dir) = common::setup_db().await;

    rbx_cmd(&db_path)
        .args([
            "query",
            "UPDATE djmdContent SET Commnt = 'escape hatch' WHERE ID = '101'",
            "--unsafe-write",
        ])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let (count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM djmdContent WHERE Commnt = 'escape hatch'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
}
