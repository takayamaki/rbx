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
    assert!(
        id.chars().all(|c| c.is_ascii_digit()),
        "non-numeric ID: {}",
        id
    );

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
    assert!(
        ts_regex().is_match(&created_at),
        "bad timestamp: {}",
        created_at
    );
    assert_eq!((ds, lds, ld, ls), (0, 0, 0, 0));

    let (counter,): (i64,) =
        sqlx::query_as("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        usn <= counter,
        "row USN {} exceeds counter {}",
        usn,
        counter
    );
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
    assert!(
        ts_regex().is_match(&created_at),
        "bad timestamp: {}",
        created_at
    );
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
    assert_eq!(
        usns,
        vec![1001, 1002],
        "contiguous block from seeded counter"
    );

    let (counter,): (i64,) =
        sqlx::query_as("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'")
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
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM djmdContent WHERE Title = 'pwned'")
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

// --- tracks update: genre / album / numeric fields / path ---
// Order: the everyday case (retag a genre) first, then album, plain numeric
// columns, file path, and finally clearing an FK column with an empty string.

/// `--genre` on a name that already exists in djmdGenre must reuse that row's
/// ID instead of creating a duplicate.
#[tokio::test]
async fn tracks_update_genre_reuses_existing_genre_row() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "update", "101", "--genre", "Anime", "--execute"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "tracks.update");
    assert_eq!(json["result"]["changes"]["genre"], "Anime");

    let pool = common::open_pool(&db_path).await;
    let (genre_id,): (String,) = sqlx::query_as("SELECT GenreID FROM djmdContent WHERE ID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(genre_id, "501", "must reuse the seeded djmdGenre row");
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM djmdGenre WHERE Name = 'Anime'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "must not create a duplicate genre row");
}

/// `--genre` on an unknown name creates a djmdGenre row in native format
/// (numeric ID, UUID, USN within the counter, native timestamps) and points
/// the track at it.
#[tokio::test]
async fn tracks_update_genre_creates_native_format_genre_row() {
    let (db_path, _dir) = common::setup_db().await;

    rbx_cmd(&db_path)
        .args([
            "tracks",
            "update",
            "101",
            "--genre",
            "IM@S SOLO",
            "--execute",
        ])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let (genre_id,): (String,) = sqlx::query_as("SELECT GenreID FROM djmdContent WHERE ID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(
        genre_id.chars().all(|c| c.is_ascii_digit()),
        "non-numeric genre ID: {}",
        genre_id
    );

    let (name, uuid, usn, created_at, updated_at): (String, String, i64, String, String) =
        sqlx::query_as(
            "SELECT Name, UUID, rb_local_usn, created_at, updated_at FROM djmdGenre WHERE ID = ?",
        )
        .bind(&genre_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "IM@S SOLO");
    assert!(!uuid.is_empty());
    assert_ne!(uuid, genre_id);
    assert!(
        ts_regex().is_match(&created_at),
        "bad timestamp: {}",
        created_at
    );
    assert!(
        ts_regex().is_match(&updated_at),
        "bad timestamp: {}",
        updated_at
    );

    let (counter,): (i64,) =
        sqlx::query_as("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(usn <= counter, "usn {} exceeds counter {}", usn, counter);
}

/// `--album` resolves or creates a djmdAlbum row with the full native column
/// set (AlbumArtistID, ImagePath, Compilation, SearchStr) and sets AlbumID.
#[tokio::test]
async fn tracks_update_album_resolves_or_creates_album_row() {
    let (db_path, _dir) = common::setup_db().await;

    rbx_cmd(&db_path)
        .args([
            "tracks",
            "update",
            "101",
            "--album",
            "MASTER ARTIST 01",
            "--execute",
        ])
        .assert()
        .code(0);
    // second track with the same album name must reuse the row
    rbx_cmd(&db_path)
        .args([
            "tracks",
            "update",
            "102",
            "--album",
            "MASTER ARTIST 01",
            "--execute",
        ])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let ids: Vec<(String,)> =
        sqlx::query_as("SELECT AlbumID FROM djmdContent WHERE ID IN ('101', '102') ORDER BY ID")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        ids[0].0, ids[1].0,
        "both tracks must point at the same album row"
    );
    let album_id = &ids[0].0;
    assert!(
        album_id.chars().all(|c| c.is_ascii_digit()),
        "non-numeric album ID: {}",
        album_id
    );

    let (name, album_artist, image, compilation, search, uuid, usn, created_at): (String, String, String, i64, String, String, i64, String) =
        sqlx::query_as(
            "SELECT Name, AlbumArtistID, ImagePath, Compilation, SearchStr, UUID, rb_local_usn, created_at \
             FROM djmdAlbum WHERE ID = ?",
        )
        .bind(album_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "MASTER ARTIST 01");
    assert_eq!(
        (
            album_artist.as_str(),
            image.as_str(),
            compilation,
            search.as_str()
        ),
        ("", "", 0, "")
    );
    assert!(!uuid.is_empty());
    assert!(
        ts_regex().is_match(&created_at),
        "bad timestamp: {}",
        created_at
    );
    let (counter,): (i64,) =
        sqlx::query_as("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(usn <= counter, "usn {} exceeds counter {}", usn, counter);
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM djmdAlbum")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
}

/// `--track-no`, `--disc-no`, `--year` write TrackNo / DiscNo / ReleaseYear.
#[tokio::test]
async fn tracks_update_writes_numeric_columns() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args([
            "tracks",
            "update",
            "101",
            "--track-no",
            "7",
            "--disc-no",
            "2",
            "--year",
            "2018",
            "--execute",
        ])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["result"]["changes"]["track_no"], 7);
    assert_eq!(json["result"]["changes"]["disc_no"], 2);
    assert_eq!(json["result"]["changes"]["year"], 2018);

    let pool = common::open_pool(&db_path).await;
    let row: (i64, i64, i64) =
        sqlx::query_as("SELECT TrackNo, DiscNo, ReleaseYear FROM djmdContent WHERE ID = '101'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, (7, 2, 2018));
}

/// `--path` rewrites FolderPath and keeps FileNameL in sync with its basename.
#[tokio::test]
async fn tracks_update_path_sets_folder_path_and_file_name() {
    let (db_path, _dir) = common::setup_db().await;

    rbx_cmd(&db_path)
        .args([
            "tracks",
            "update",
            "101",
            "--path",
            "F:/DJ用音楽/THE IDOLM@STER/01_S(mile)ING!.m4a",
            "--execute",
        ])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let (folder, name_l): (String, String) =
        sqlx::query_as("SELECT FolderPath, FileNameL FROM djmdContent WHERE ID = '101'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(folder, "F:/DJ用音楽/THE IDOLM@STER/01_S(mile)ING!.m4a");
    assert_eq!(name_l, "01_S(mile)ING!.m4a");
}

/// `--artist ""` (and the same for genre / album) clears the FK column to ""
/// without creating a row whose Name is empty.
#[tokio::test]
async fn tracks_update_empty_string_clears_fk_without_creating_row() {
    let (db_path, _dir) = common::setup_db().await;

    rbx_cmd(&db_path)
        .args([
            "tracks",
            "update",
            "101",
            "--artist",
            "",
            "--genre",
            "",
            "--album",
            "",
            "--execute",
        ])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let row: (String, String, String) =
        sqlx::query_as("SELECT ArtistID, GenreID, AlbumID FROM djmdContent WHERE ID = '101'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(row, ("".into(), "".into(), "".into()));
    for table in ["djmdArtist", "djmdGenre", "djmdAlbum"] {
        let (count,): (i64,) =
            sqlx::query_as(&format!("SELECT COUNT(*) FROM {} WHERE Name = ''", table))
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(count, 0, "{} must not get a row with an empty Name", table);
    }
}

// --- describe ---

fn describe(args: &[&str]) -> serde_json::Value {
    let mut cmd = Command::cargo_bin("rbx").unwrap();
    cmd.env_remove("RBX_DB_PATH").arg("describe").args(args);
    let assert = cmd.assert().code(0);
    stdout_json(&assert)
}

/// `describe` is the only map of the CLI an agent has. Every resource the
/// root lists must describe itself, and every action a resource lists must
/// resolve to a command description with an output schema.
#[test]
fn describe_listings_resolve_to_command_descriptions() {
    let root = describe(&[]);
    let resources = root["resources"].as_array().unwrap();
    assert!(!resources.is_empty());

    for res in resources {
        let name = res["name"].as_str().unwrap();
        let listing = describe(&[name]);
        assert_eq!(listing["kind"], "describe", "{}: {}", name, listing);
        if name == "query" {
            // query has no sub-actions: the resource describes the command itself
            assert_eq!(listing["command"], "query");
            continue;
        }
        assert_eq!(listing["resource"], name);
        let actions = listing["actions"].as_array().unwrap();
        assert!(!actions.is_empty(), "{} lists no actions", name);

        for act in actions {
            let action = act["name"].as_str().unwrap();
            let cmd = describe(&[name, action]);
            assert_eq!(cmd["command"], format!("{} {}", name, action), "{}", cmd);
            assert!(cmd["flags"].is_array(), "{} {}: no flags", name, action);
            assert!(
                cmd["output_schema"].is_object(),
                "{} {}: no output_schema",
                name,
                action
            );
            assert!(
                !cmd["examples"].as_array().unwrap().is_empty(),
                "{} {}: no examples",
                name,
                action
            );
        }
    }
}

// --- tracks bulk-update ---

fn write_plan(dir: &tempfile::TempDir, json: &str) -> std::path::PathBuf {
    let path = dir.path().join("updates.json");
    std::fs::write(&path, json).unwrap();
    path
}

// Order: the everyday case (apply a plan file) first, then dry-run, FK rows
// shared by many rows, then the ways a plan can be rejected, and stdin last.

/// A plan file with several rows is applied in one run: every row's columns
/// are updated and the result reports how many rows changed.
#[tokio::test]
async fn bulk_update_applies_every_row_in_one_run() {
    let (db_path, dir) = common::setup_db().await;
    let plan = write_plan(
        &dir,
        r#"[
          {"id": "101", "fields": {"title": "Track One (Remix)", "artist": "Artist A", "year": 2018}},
          {"id": "102", "fields": {"path": "C:/Music/sub/two renamed.mp3", "comment": "moved"}}
        ]"#,
    );

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", plan.to_str().unwrap(), "--execute"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "tracks.bulk_update");
    assert_eq!(json["dry_run"], false);
    assert_eq!(json["result"]["updated"], 2);

    let pool = common::open_pool(&db_path).await;
    let (title, artist_id, year): (String, String, i64) =
        sqlx::query_as("SELECT Title, ArtistID, ReleaseYear FROM djmdContent WHERE ID = '101'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (title.as_str(), artist_id.as_str(), year),
        ("Track One (Remix)", "201", 2018)
    );
    let (folder, file, comment): (String, String, String) =
        sqlx::query_as("SELECT FolderPath, FileNameL, Commnt FROM djmdContent WHERE ID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (folder.as_str(), file.as_str(), comment.as_str()),
        ("C:/Music/sub/two renamed.mp3", "two renamed.mp3", "moved")
    );
}

/// Without --execute nothing is written. The plan still validates every row
/// and lists the artist / genre / album names that would be created, so
/// typos in a plan show up before anything is applied.
#[tokio::test]
async fn bulk_update_dry_run_writes_nothing_and_lists_rows_to_create() {
    let (db_path, dir) = common::setup_db().await;
    let plan = write_plan(
        &dir,
        r#"[
          {"id": "101", "fields": {"artist": "Brand New Artist", "genre": "Anime"}},
          {"id": "102", "fields": {"artist": "Brand New Artist", "album": "New Album"}}
        ]"#,
    );

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", plan.to_str().unwrap()])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "tracks.bulk_update");
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["plan"]["count"], 2);
    assert_eq!(
        json["plan"]["creates"]["artists"],
        serde_json::json!(["Brand New Artist"])
    );
    assert_eq!(json["plan"]["creates"]["genres"], serde_json::json!([]));
    assert_eq!(
        json["plan"]["creates"]["albums"],
        serde_json::json!(["New Album"])
    );
    assert_eq!(json["next_step"], "Add --execute to apply");

    let pool = common::open_pool(&db_path).await;
    let (artist_id,): (String,) =
        sqlx::query_as("SELECT ArtistID FROM djmdContent WHERE ID = '101'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(artist_id, "201", "dry-run must not touch the track");
    let (artists, albums): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM djmdArtist WHERE Name = 'Brand New Artist'),                 (SELECT COUNT(*) FROM djmdAlbum)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((artists, albums), (0, 0), "dry-run must not create FK rows");
}

/// The same new artist name on many rows creates exactly one djmdArtist row
/// (in native format) and every row points at it.
#[tokio::test]
async fn bulk_update_creates_a_shared_artist_row_once() {
    let (db_path, dir) = common::setup_db().await;
    let plan = write_plan(
        &dir,
        r#"[
          {"id": "101", "fields": {"artist": "Shared New Artist"}},
          {"id": "102", "fields": {"artist": "Shared New Artist"}}
        ]"#,
    );

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", plan.to_str().unwrap(), "--execute"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(
        json["result"]["created"]["artists"],
        serde_json::json!(["Shared New Artist"])
    );

    let pool = common::open_pool(&db_path).await;
    let rows: Vec<(String, String, i64, String)> = sqlx::query_as(
        "SELECT ID, UUID, rb_local_usn, created_at FROM djmdArtist WHERE Name = 'Shared New Artist'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 1, "one artist row for the whole batch");
    let (artist_id, uuid, usn, created_at) = &rows[0];
    assert!(
        artist_id.chars().all(|c| c.is_ascii_digit()),
        "non-numeric ID: {}",
        artist_id
    );
    assert!(!uuid.is_empty());
    assert!(
        ts_regex().is_match(created_at),
        "bad timestamp: {}",
        created_at
    );
    let (counter,): (i64,) =
        sqlx::query_as("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        *usn <= counter,
        "row USN {} exceeds counter {}",
        usn,
        counter
    );

    let ids: Vec<(String,)> =
        sqlx::query_as("SELECT ArtistID FROM djmdContent WHERE ID IN ('101', '102') ORDER BY ID")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ids, vec![(artist_id.clone(),), (artist_id.clone(),)]);
}

/// One unknown track ID rejects the whole batch: nothing is written and the
/// error lists every bad row by index and id.
#[tokio::test]
async fn bulk_update_rejects_the_whole_batch_when_a_track_is_missing() {
    let (db_path, dir) = common::setup_db().await;
    let plan = write_plan(
        &dir,
        r#"[
          {"id": "101", "fields": {"title": "Changed"}},
          {"id": "999", "fields": {"title": "No such track"}},
          {"id": "102", "fields": {"key": "13Z"}}
        ]"#,
    );

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", plan.to_str().unwrap(), "--execute"])
        .assert()
        .code(3);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "error");
    assert_eq!(json["error"]["category"], "not_found");
    let errors = json["error"]["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 2, "every bad row is reported: {}", json);
    assert_eq!(
        (&errors[0]["index"], &errors[0]["id"]),
        (&serde_json::json!(1), &serde_json::json!("999"))
    );
    assert_eq!(
        (&errors[1]["index"], &errors[1]["id"]),
        (&serde_json::json!(2), &serde_json::json!("102"))
    );
    assert!(errors[1]["message"].as_str().unwrap().contains("13Z"));

    let pool = common::open_pool(&db_path).await;
    let (title,): (String,) = sqlx::query_as("SELECT Title FROM djmdContent WHERE ID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        title, "Track One",
        "a rejected batch must not write any row"
    );
}

/// A field name that is not a `tracks update` flag (e.g. `trackNo`) is a
/// usage error, so a plan generator with the wrong key names fails loudly.
#[tokio::test]
async fn bulk_update_rejects_unknown_field_names() {
    let (db_path, dir) = common::setup_db().await;
    let plan = write_plan(&dir, r#"[{"id": "101", "fields": {"trackNo": 3}}]"#);

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", plan.to_str().unwrap(), "--execute"])
        .assert()
        .code(2);
    let json = stdout_json(&assert);
    assert_eq!(json["error"]["category"], "usage");
    assert!(
        json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("trackNo"),
        "message must name the bad field: {}",
        json
    );
}

/// The same track ID twice in one plan is a conflict, not last-wins: it is
/// almost always a bug in the plan generator.
#[tokio::test]
async fn bulk_update_rejects_duplicate_ids() {}

/// `-` reads the plan from stdin.
#[tokio::test]
async fn bulk_update_reads_the_plan_from_stdin() {}
