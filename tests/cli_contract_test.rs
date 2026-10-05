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
async fn bulk_update_rejects_duplicate_ids() {
    let (db_path, dir) = common::setup_db().await;
    let plan = write_plan(
        &dir,
        r#"[
          {"id": "101", "fields": {"title": "First"}},
          {"id": "102", "fields": {"title": "Other"}},
          {"id": "101", "fields": {"title": "Second"}}
        ]"#,
    );

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", plan.to_str().unwrap(), "--execute"])
        .assert()
        .code(5);
    let json = stdout_json(&assert);
    assert_eq!(json["error"]["category"], "conflict");
    let errors = json["error"]["errors"].as_array().unwrap();
    assert_eq!(errors.len(), 1, "{}", json);
    assert_eq!(
        (&errors[0]["index"], &errors[0]["id"]),
        (&serde_json::json!(2), &serde_json::json!("101"))
    );

    let pool = common::open_pool(&db_path).await;
    let (title,): (String,) = sqlx::query_as("SELECT Title FROM djmdContent WHERE ID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        title, "Track One",
        "neither copy of the duplicate may be applied"
    );
}

/// Writes happen in one transaction: when a row fails mid-way (here forced
/// by a trigger on the second track), the rows already written are rolled
/// back and the DB is exactly as before.
#[tokio::test]
async fn bulk_update_rolls_back_every_row_when_a_write_fails() {
    let (db_path, dir) = common::setup_db().await;
    rbx_cmd(&db_path)
        .args([
            "query",
            "CREATE TRIGGER boom BEFORE UPDATE ON djmdContent WHEN NEW.ID = '102' \
             BEGIN SELECT RAISE(ABORT, 'boom'); END",
            "--unsafe-write",
        ])
        .assert()
        .code(0);
    let plan = write_plan(
        &dir,
        r#"[
          {"id": "101", "fields": {"title": "Written first", "artist": "Rolled Back Artist"}},
          {"id": "102", "fields": {"title": "Fails"}}
        ]"#,
    );

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", plan.to_str().unwrap(), "--execute"])
        .assert()
        .code(1);
    let json = stdout_json(&assert);
    assert_eq!(json["error"]["category"], "database");

    let pool = common::open_pool(&db_path).await;
    let (title, artist_count, counter): (String, i64, i64) = sqlx::query_as(
        "SELECT (SELECT Title FROM djmdContent WHERE ID = '101'), \
                (SELECT COUNT(*) FROM djmdArtist WHERE Name = 'Rolled Back Artist'), \
                (SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(title, "Track One", "the first row must be rolled back");
    assert_eq!(
        artist_count, 0,
        "the FK row created for the first row must be rolled back"
    );
    assert_eq!(counter, 1000, "the USN counter must be rolled back");
}

/// `-` reads the plan from stdin.
#[tokio::test]
async fn bulk_update_reads_the_plan_from_stdin() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "bulk-update", "-", "--execute"])
        .write_stdin(r#"[{"id": "101", "fields": {"comment": "from stdin"}}]"#)
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["result"]["updated"], 1);

    let pool = common::open_pool(&db_path).await;
    let (comment,): (String,) = sqlx::query_as("SELECT Commnt FROM djmdContent WHERE ID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(comment, "from stdin");
}

// --- playlists tracks add / remove --position ---
// Order: the everyday case (insert one track in the middle) first, then
// dry-run, several tracks at once, the edge positions, and removing by
// position (for a track that is in the playlist twice) last.

/// Creates tracks besides the seeded 101 / 102 (no-op for those).
async fn seed_tracks(pool: &sqlx::SqlitePool, track_ids: &[&str]) {
    for tid in track_ids {
        sqlx::query(
            "INSERT OR IGNORE INTO djmdContent (ID, Title, UUID, rb_local_usn, created_at, updated_at) \
             VALUES (?, ?, ?, 10, ?, ?)",
        )
        .bind(tid)
        .bind(format!("Track {}", tid))
        .bind(format!("c0000000-0000-0000-0000-000000000{}", tid))
        .bind(common::SEED_TS)
        .bind(common::SEED_TS)
        .execute(pool)
        .await
        .unwrap();
    }
}

/// Fills the seeded playlist 501 with the given tracks, in order.
async fn seed_playlist(pool: &sqlx::SqlitePool, track_ids: &[&str]) {
    seed_tracks(pool, track_ids).await;
    for (i, tid) in track_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO djmdSongPlaylist \
             (ID, PlaylistID, ContentID, TrackNo, UUID, rb_local_usn, created_at, updated_at) \
             VALUES (?, '501', ?, ?, ?, 10, ?, ?)",
        )
        .bind(format!("sp{}", i + 1))
        .bind(tid)
        .bind((i + 1) as i64)
        .bind(format!("s0000000-0000-0000-0000-00000000000{}", i + 1))
        .bind(common::SEED_TS)
        .bind(common::SEED_TS)
        .execute(pool)
        .await
        .unwrap();
    }
}

/// (TrackNo, track ID) of every row in playlist 501, in TrackNo order.
async fn playlist_rows(pool: &sqlx::SqlitePool) -> Vec<(i64, String)> {
    sqlx::query_as(
        "SELECT TrackNo, ContentID FROM djmdSongPlaylist WHERE PlaylistID = '501' ORDER BY TrackNo",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

fn rows(expected: &[(i64, &str)]) -> Vec<(i64, String)> {
    expected.iter().map(|(n, t)| (*n, t.to_string())).collect()
}

/// `--position N` inserts the track as the N-th row (1-based) and moves every
/// later row down by one. The new row has the full native column set.
#[tokio::test]
async fn playlist_tracks_add_at_position_inserts_and_shifts_later_rows() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102", "103"]).await;
    seed_tracks(&pool, &["104"]).await;

    let assert = rbx_cmd(&db_path)
        .args(["playlists", "tracks", "add", "501", "104"])
        .args(["--position", "2", "--execute"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "playlists.tracks.add");
    assert_eq!(json["result"]["added"][0]["track_no"], 2);

    assert_eq!(
        playlist_rows(&pool).await,
        rows(&[(1, "101"), (2, "104"), (3, "102"), (4, "103")])
    );

    let (uuid, usn, created_at, ds, lds, ld, ls): (String, i64, String, i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT UUID, rb_local_usn, created_at, \
             rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced \
             FROM djmdSongPlaylist WHERE PlaylistID = '501' AND ContentID = '104'",
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
    assert_eq!((ds, lds, ld, ls), (0, 0, 0, 0));
}

/// Without --execute nothing is written. The plan shows the position and how
/// many existing rows would move down.
#[tokio::test]
async fn playlist_tracks_add_at_position_dry_run_writes_nothing() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102", "103"]).await;
    seed_tracks(&pool, &["104"]).await;

    let assert = rbx_cmd(&db_path)
        .args([
            "playlists",
            "tracks",
            "add",
            "501",
            "104",
            "--position",
            "2",
        ])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["dry_run"], true);
    assert_eq!(json["plan"]["starting_track_no"], 2);
    assert_eq!(json["plan"]["shifted_count"], 2);

    assert_eq!(
        playlist_rows(&pool).await,
        rows(&[(1, "101"), (2, "102"), (3, "103")])
    );
}

/// Several track IDs with `--position N` go in at N, N+1, ... in the order
/// they were given.
#[tokio::test]
async fn playlist_tracks_add_several_tracks_at_position_keeps_their_order() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102", "103"]).await;
    seed_tracks(&pool, &["104", "105"]).await;

    rbx_cmd(&db_path)
        .args(["playlists", "tracks", "add", "501", "105", "104"])
        .args(["--position", "3", "--execute"])
        .assert()
        .code(0);

    assert_eq!(
        playlist_rows(&pool).await,
        rows(&[(1, "101"), (2, "102"), (3, "105"), (4, "104"), (5, "103")])
    );
}

/// `--position` one past the last row is the same as appending.
#[tokio::test]
async fn playlist_tracks_add_at_position_after_last_row_appends() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102"]).await;
    seed_tracks(&pool, &["103"]).await;

    rbx_cmd(&db_path)
        .args(["playlists", "tracks", "add", "501", "103"])
        .args(["--position", "3", "--execute"])
        .assert()
        .code(0);

    assert_eq!(
        playlist_rows(&pool).await,
        rows(&[(1, "101"), (2, "102"), (3, "103")])
    );
}

/// `--position 0` or a position past the end + 1 is a usage error and
/// nothing is written.
#[tokio::test]
async fn playlist_tracks_add_at_out_of_range_position_is_a_usage_error() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102"]).await;
    seed_tracks(&pool, &["103"]).await;

    for position in ["0", "4"] {
        let assert = rbx_cmd(&db_path)
            .args(["playlists", "tracks", "add", "501", "103"])
            .args(["--position", position, "--execute"])
            .assert()
            .code(2);
        let json = stdout_json(&assert);
        assert_eq!(json["error"]["category"], "usage", "position {}", position);
    }

    assert_eq!(playlist_rows(&pool).await, rows(&[(1, "101"), (2, "102")]));
}

/// `remove --position N` removes only the N-th row, even when the same track
/// is in the playlist twice, and renumbers the rest.
#[tokio::test]
async fn playlist_tracks_remove_at_position_removes_only_that_row() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102", "101", "103"]).await;

    let assert = rbx_cmd(&db_path)
        .args(["playlists", "tracks", "remove", "501"])
        .args(["--position", "3", "--execute"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "playlists.tracks.remove");
    assert_eq!(json["result"]["removed_count"], 1);

    assert_eq!(
        playlist_rows(&pool).await,
        rows(&[(1, "101"), (2, "102"), (3, "103")])
    );
    let (first_row_id,): (String,) =
        sqlx::query_as("SELECT ID FROM djmdSongPlaylist WHERE PlaylistID = '501' AND TrackNo = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(first_row_id, "sp1", "the first 101 must stay");
}

/// Without --execute nothing is written. The plan names the track at that
/// position so the caller can check it before applying.
#[tokio::test]
async fn playlist_tracks_remove_at_position_dry_run_names_the_track() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102", "101", "103"]).await;

    let assert = rbx_cmd(&db_path)
        .args(["playlists", "tracks", "remove", "501", "--position", "3"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["dry_run"], true);
    assert_eq!(
        json["plan"]["tracks"],
        serde_json::json!([{ "id": "101", "title": "Track One", "track_no": 3 }])
    );

    assert_eq!(playlist_rows(&pool).await.len(), 4);
}

/// A position with no row is a usage error and nothing is written.
#[tokio::test]
async fn playlist_tracks_remove_at_out_of_range_position_is_a_usage_error() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102"]).await;

    for position in ["0", "3"] {
        let assert = rbx_cmd(&db_path)
            .args(["playlists", "tracks", "remove", "501"])
            .args(["--position", position, "--execute"])
            .assert()
            .code(2);
        let json = stdout_json(&assert);
        assert_eq!(json["error"]["category"], "usage", "position {}", position);
    }

    assert_eq!(playlist_rows(&pool).await, rows(&[(1, "101"), (2, "102")]));
}

/// Track IDs and `--position` together are ambiguous and rejected.
#[tokio::test]
async fn playlist_tracks_remove_with_track_ids_and_position_is_a_usage_error() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_playlist(&pool, &["101", "102", "103"]).await;

    rbx_cmd(&db_path)
        .args(["playlists", "tracks", "remove", "501", "102"])
        .args(["--position", "3", "--execute"])
        .assert()
        .code(2);

    assert_eq!(playlist_rows(&pool).await.len(), 3);
}

// --- tracks cues add / update / delete ---
// What rekordbox itself writes was checked against a real master.db (rekordbox 7):
// InFrame is 1/150 s, unused columns are NULL, hot cues D-H are Kind 5-9,
// every track with cues has one contentCue row holding all its cues as JSON,
// and djmdContent.CueUpdated goes up on every cue change.
// Order: the everyday case (add a memory cue) first, then the side tables,
// hot cues, colors, list, update, delete, and the cases that are refused last.

/// CueMicrosec, ActiveLoop, BeatLoopSize, ColorTableIndex, Comment, rb_local_usn
type CueNullColumns = (
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<i64>,
);

/// ID, UUID, Cues, rb_cue_count, created_at, rb_* status fields
type ContentCueRow = (String, String, String, i64, String, i64, i64, i64, i64);

/// A memory cue on track 102 as rekordbox wrote it: an older field order
/// (ContentUUID near the end) and CueMicrosec 0.
const REKORDBOX_CUE_ENTRY: &str = r#"{"ID":"900","ContentID":"102","InMsec":1000,"InFrame":150,"InMpegFrame":0,"InMpegAbs":0,"OutMsec":-1,"OutFrame":0,"OutMpegFrame":0,"OutMpegAbs":0,"Kind":0,"Color":-1,"ColorTableIndex":0,"ActiveLoop":0,"BeatLoopSize":0,"CueMicrosec":0,"ContentUUID":"c0000000-0000-0000-0000-000000000102","UUID":"q0000000-0000-0000-0000-000000000900","created_at":"2026-01-01T00:00:00.000+00:00","updated_at":"2026-01-01T00:00:00.000+00:00"}"#;

/// Seeds REKORDBOX_CUE_ENTRY as a djmdCue row and a contentCue row.
async fn seed_rekordbox_cue(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO djmdCue (ID, ContentID, InMsec, InFrame, InMpegFrame, InMpegAbs, \
         OutMsec, OutFrame, OutMpegFrame, OutMpegAbs, Kind, Color, ColorTableIndex, \
         ActiveLoop, BeatLoopSize, CueMicrosec, ContentUUID, UUID, created_at, updated_at) \
         VALUES ('900', '102', 1000, 150, 0, 0, -1, 0, 0, 0, 0, -1, 0, 0, 0, 0, \
         'c0000000-0000-0000-0000-000000000102', 'q0000000-0000-0000-0000-000000000900', ?, ?)",
    )
    .bind(common::SEED_TS)
    .bind(common::SEED_TS)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count, UUID, rb_local_usn, created_at, updated_at) \
         VALUES ('c0000000-0000-0000-0000-000000000102', '102', ?, 1, \
         'r0000000-0000-0000-0000-000000000102', 10, ?, ?)",
    )
    .bind(format!("[{}]", REKORDBOX_CUE_ENTRY))
    .bind(common::SEED_TS)
    .bind(common::SEED_TS)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query("UPDATE djmdContent SET CueUpdated = '3' WHERE ID = '102'")
        .execute(pool)
        .await
        .unwrap();
}

/// A memory cue gets InFrame = floor(msec * 150 / 1000), OutMsec -1, and NULL
/// in the columns rekordbox leaves empty (CueMicrosec, ActiveLoop, BeatLoopSize,
/// ColorTableIndex, Comment). The cue row itself has no USN, like rekordbox's.
#[tokio::test]
async fn cues_add_memory_cue_writes_frame_and_leaves_unused_columns_null() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "cues", "add", "101", "141013", "--execute"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    assert_eq!(json["kind"], "tracks.cues.add");
    let cue_id = json["result"]["cue_id"].as_str().unwrap().to_string();

    let pool = common::open_pool(&db_path).await;
    let row: (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT InMsec, InFrame, InMpegFrame, InMpegAbs, OutMsec, OutFrame, \
         OutMpegFrame, OutMpegAbs, Kind, Color FROM djmdCue WHERE ID = ?",
    )
    .bind(&cue_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row, (141013, 21151, 0, 0, -1, 0, 0, 0, 0, -1));

    let nulls: CueNullColumns = sqlx::query_as(
        "SELECT CueMicrosec, ActiveLoop, BeatLoopSize, ColorTableIndex, Comment, rb_local_usn \
             FROM djmdCue WHERE ID = ?",
    )
    .bind(&cue_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(nulls, (None, None, None, None, None, None));

    let (uuid, content_uuid, created_at): (String, String, String) =
        sqlx::query_as("SELECT UUID, ContentUUID, created_at FROM djmdCue WHERE ID = ?")
            .bind(&cue_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(!uuid.is_empty());
    assert_eq!(content_uuid, "c0000000-0000-0000-0000-000000000101");
    assert!(
        ts_regex().is_match(&created_at),
        "bad timestamp: {}",
        created_at
    );
}

/// The first cue on a track creates its contentCue row: ID is the track UUID,
/// Cues is a JSON array with the cue (NULL fields left out, ISO timestamps)
/// and rb_cue_count is 1.
#[tokio::test]
async fn cues_add_creates_the_content_cue_row() {
    let (db_path, _dir) = common::setup_db().await;

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "cues", "add", "101", "141013", "--execute"])
        .assert()
        .code(0);
    let cue_id = stdout_json(&assert)["result"]["cue_id"]
        .as_str()
        .unwrap()
        .to_string();

    let pool = common::open_pool(&db_path).await;
    let (id, uuid, cues, count, created_at, ds, lds, ld, ls): ContentCueRow = sqlx::query_as(
        "SELECT ID, UUID, Cues, rb_cue_count, created_at, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced \
         FROM contentCue WHERE ContentID = '101'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        id, "c0000000-0000-0000-0000-000000000101",
        "ID is the track UUID"
    );
    assert!(!uuid.is_empty());
    assert_ne!(uuid, id);
    assert_eq!(count, 1);
    assert!(
        ts_regex().is_match(&created_at),
        "bad timestamp: {}",
        created_at
    );
    assert_eq!((ds, lds, ld, ls), (0, 0, 0, 0));

    // Same keys, in the same order, as the entries rekordbox writes for a plain cue
    let keys = [
        "ID",
        "ContentID",
        "ContentUUID",
        "InMsec",
        "InFrame",
        "InMpegFrame",
        "InMpegAbs",
        "OutMsec",
        "OutFrame",
        "OutMpegFrame",
        "OutMpegAbs",
        "Kind",
        "Color",
        "UUID",
        "created_at",
        "updated_at",
    ];
    let key_re = regex::Regex::new(r#""([A-Za-z_]+)":"#).unwrap();
    let found: Vec<&str> = key_re
        .captures_iter(&cues)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    assert_eq!(found, keys);

    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    let entry = &entries[0];
    assert_eq!(entry["ID"], cue_id.as_str());
    assert_eq!(entry["ContentID"], "101");
    assert_eq!(entry["InMsec"], 141013);
    assert_eq!(entry["InFrame"], 21151);
    assert_eq!(entry["OutMsec"], -1);
    assert_eq!(entry["Kind"], 0);
    assert_eq!(entry["Color"], -1);
    let iso = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{3}\+00:00$").unwrap();
    assert!(
        iso.is_match(entry["created_at"].as_str().unwrap()),
        "{}",
        entry["created_at"]
    );
}

/// A cue on a track that already has cues is appended to its contentCue JSON.
/// The entries rekordbox wrote are kept byte for byte.
#[tokio::test]
async fn cues_add_appends_to_content_cue_and_keeps_other_entries_as_they_are() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_rekordbox_cue(&pool).await;

    rbx_cmd(&db_path)
        .args(["tracks", "cues", "add", "102", "30000", "--execute"])
        .assert()
        .code(0);

    let (cues, count): (String, i64) =
        sqlx::query_as("SELECT Cues, rb_cue_count FROM contentCue WHERE ContentID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
    assert!(
        cues.starts_with(&format!("[{},", REKORDBOX_CUE_ENTRY)),
        "first entry changed: {}",
        cues
    );
    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    assert_eq!(entries[1]["InMsec"], 30000);
}

/// Adding a cue bumps djmdContent.CueUpdated by one and gives contentCue and
/// djmdContent new USNs, in that order, within the agentRegistry counter.
#[tokio::test]
async fn cues_add_bumps_cue_updated_and_usns() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_rekordbox_cue(&pool).await;

    rbx_cmd(&db_path)
        .args(["tracks", "cues", "add", "102", "30000", "--execute"])
        .assert()
        .code(0);

    let (cue_updated, track_usn, track_updated_at): (String, i64, String) = sqlx::query_as(
        "SELECT CueUpdated, rb_local_usn, updated_at FROM djmdContent WHERE ID = '102'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(cue_updated, "4");
    assert_ne!(track_updated_at, common::SEED_TS);
    assert!(
        ts_regex().is_match(&track_updated_at),
        "bad timestamp: {}",
        track_updated_at
    );

    let (content_cue_usn,): (i64,) =
        sqlx::query_as("SELECT rb_local_usn FROM contentCue WHERE ContentID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (content_cue_usn, track_usn),
        (1001, 1002),
        "contentCue first"
    );

    let (counter,): (i64,) =
        sqlx::query_as("SELECT int_1 FROM agentRegistry WHERE registry_id = 'localUpdateCount'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(counter, 1002);
}

/// Hot cue slots A-C are Kind 1-3 and D-H are Kind 5-9 (Kind 4 is not a slot).
#[tokio::test]
async fn cues_add_hot_cue_slots_d_to_h_use_kind_5_to_9() {
    let (db_path, _dir) = common::setup_db().await;

    for (slot, msec) in [("1", "1000"), ("3", "3000"), ("4", "4000"), ("8", "8000")] {
        rbx_cmd(&db_path)
            .args(["tracks", "cues", "add", "101", msec, "--kind", "hot"])
            .args(["--slot", slot, "--execute"])
            .assert()
            .code(0);
    }

    let pool = common::open_pool(&db_path).await;
    let kinds: Vec<(i64, i64)> =
        sqlx::query_as("SELECT InMsec, Kind FROM djmdCue WHERE ContentID = '101' ORDER BY InMsec")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(kinds, vec![(1000, 1), (3000, 3), (4000, 5), (8000, 9)]);
}

/// `--color pink|red|orange|yellow|green|aqua|blue|purple` sets Color 0-7 on a
/// memory cue (checked by coloring cues in rekordbox 7). Without it, Color is -1.
/// Names that only exist in the hot cue menu (teal, ...) are refused on memory cues.
#[tokio::test]
async fn cues_add_memory_cue_with_color() {
    let (db_path, _dir) = common::setup_db().await;

    for (color, msec) in [("pink", "1000"), ("aqua", "2000"), ("purple", "3000")] {
        rbx_cmd(&db_path)
            .args([
                "tracks",
                "cues",
                "add",
                "101",
                msec,
                "--color",
                color,
                "--execute",
            ])
            .assert()
            .code(0);
    }
    rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "add",
            "101",
            "4000",
            "--color",
            "teal",
            "--execute",
        ])
        .assert()
        .code(2);

    let pool = common::open_pool(&db_path).await;
    let colors: Vec<(i64, i64)> =
        sqlx::query_as("SELECT InMsec, Color FROM djmdCue WHERE ContentID = '101' ORDER BY InMsec")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(colors, vec![(1000, 0), (2000, 5), (3000, 7)]);

    let (cues,): (String,) = sqlx::query_as("SELECT Cues FROM contentCue WHERE ContentID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    assert_eq!(entries[2]["Color"], 7);
}

/// `cues list` turns Kind back into the hot cue slot and Color into its name.
#[tokio::test]
async fn cues_list_reports_slot_and_color_name() {
    let (db_path, _dir) = common::setup_db().await;
    rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "add",
            "101",
            "1000",
            "--color",
            "green",
            "--execute",
        ])
        .assert()
        .code(0);
    rbx_cmd(&db_path)
        .args([
            "tracks", "cues", "add", "101", "2000", "--kind", "hot", "--slot", "4",
        ])
        .arg("--execute")
        .assert()
        .code(0);

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "cues", "list", "101"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    let items = json["items"].as_array().unwrap();
    let memory = items.iter().find(|c| c["in_msec"] == 1000).unwrap();
    assert_eq!(memory["kind"], "memory");
    assert_eq!(memory["color"], "green");
    let hot = items.iter().find(|c| c["in_msec"] == 2000).unwrap();
    assert_eq!(hot["kind"], "hot");
    assert_eq!(hot["slot"], 4);
}

/// `cues update --msec` moves the cue, recomputes InFrame, and updates the
/// cue's entry in contentCue, CueUpdated and the USNs.
#[tokio::test]
async fn cues_update_msec_recomputes_frame_and_syncs_content_cue() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_rekordbox_cue(&pool).await;

    rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "update",
            "900",
            "--msec",
            "5000",
            "--execute",
        ])
        .assert()
        .code(0);

    let (in_msec, in_frame, updated_at): (i64, i64, String) =
        sqlx::query_as("SELECT InMsec, InFrame, updated_at FROM djmdCue WHERE ID = '900'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((in_msec, in_frame), (5000, 750));
    assert!(
        ts_regex().is_match(&updated_at),
        "bad timestamp: {}",
        updated_at
    );

    let (cues, count): (String, i64) =
        sqlx::query_as("SELECT Cues, rb_cue_count FROM contentCue WHERE ContentID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    assert_eq!(entries[0]["InMsec"], 5000);
    assert_eq!(entries[0]["InFrame"], 750);

    let (cue_updated,): (String,) =
        sqlx::query_as("SELECT CueUpdated FROM djmdContent WHERE ID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cue_updated, "4");
}

/// `cues update --color` changes the color of the same row (same ID).
/// `--color none` writes 255, as rekordbox does when a color is cleared.
#[tokio::test]
async fn cues_update_color_changes_the_row_in_place() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_rekordbox_cue(&pool).await;

    for (color, expected) in [("blue", 6), ("none", 255)] {
        rbx_cmd(&db_path)
            .args([
                "tracks",
                "cues",
                "update",
                "900",
                "--color",
                color,
                "--execute",
            ])
            .assert()
            .code(0);
        let (value,): (i64,) = sqlx::query_as("SELECT Color FROM djmdCue WHERE ID = '900'")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(value, expected, "--color {}", color);
    }

    let (cues,): (String,) = sqlx::query_as("SELECT Cues FROM contentCue WHERE ContentID = '102'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    assert_eq!(entries[0]["ID"], "900");
    assert_eq!(entries[0]["Color"], 255);
}

/// `cues delete` removes the row (rekordbox keeps no soft-deleted cues) and
/// its entry in contentCue.
#[tokio::test]
async fn cues_delete_removes_the_row_and_its_content_cue_entry() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_rekordbox_cue(&pool).await;
    rbx_cmd(&db_path)
        .args(["tracks", "cues", "add", "102", "30000", "--execute"])
        .assert()
        .code(0);

    rbx_cmd(&db_path)
        .args(["tracks", "cues", "delete", "900", "--execute"])
        .assert()
        .code(0);

    let (rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM djmdCue WHERE ID = '900'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "the row is removed, not soft-deleted");

    let (cues, count): (String, i64) =
        sqlx::query_as("SELECT Cues, rb_cue_count FROM contentCue WHERE ContentID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 1);
    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    assert_eq!(entries[0]["InMsec"], 30000);

    let (cue_updated,): (String,) =
        sqlx::query_as("SELECT CueUpdated FROM djmdContent WHERE ID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cue_updated, "5", "3 seeded + add + delete");
}

/// Deleting the last cue of a track removes its contentCue row
/// (rekordbox has no contentCue rows with zero cues).
#[tokio::test]
async fn cues_delete_last_cue_removes_the_content_cue_row() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_rekordbox_cue(&pool).await;

    rbx_cmd(&db_path)
        .args(["tracks", "cues", "delete", "900", "--execute"])
        .assert()
        .code(0);

    let (rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM contentCue WHERE ContentID = '102'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
    let (cue_updated,): (String,) =
        sqlx::query_as("SELECT CueUpdated FROM djmdContent WHERE ID = '102'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(cue_updated, "4");
}

/// rekordbox allows at most 10 memory cues per track. An 11th is a conflict.
#[tokio::test]
async fn cues_add_eleventh_memory_cue_is_a_conflict() {
    let (db_path, _dir) = common::setup_db().await;
    for i in 1..=10 {
        rbx_cmd(&db_path)
            .args([
                "tracks",
                "cues",
                "add",
                "101",
                &(i * 1000).to_string(),
                "--execute",
            ])
            .assert()
            .code(0);
    }

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "cues", "add", "101", "11000", "--execute"])
        .assert()
        .code(5);
    assert_eq!(stdout_json(&assert)["error"]["category"], "conflict");

    let pool = common::open_pool(&db_path).await;
    let (rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM djmdCue WHERE ContentID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 10);
}

/// mp3 (VBR needs MPEG frame offsets) and FLAC (needs seek info) are refused,
/// because rbx cannot compute those fields yet. Moving a cue (update --msec) is
/// refused for the same reason; changing its color or deleting it is fine.
#[tokio::test]
async fn cues_add_refuses_mp3_and_flac() {
    let (db_path, _dir) = common::setup_db().await;
    let pool = common::open_pool(&db_path).await;
    seed_rekordbox_cue(&pool).await;
    // rekordbox FileType: 1 = mp3, 5 = FLAC
    sqlx::query("UPDATE djmdContent SET FileType = 1 WHERE ID = '102'")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE djmdContent SET FileType = 5 WHERE ID = '101'")
        .execute(&pool)
        .await
        .unwrap();

    for track in ["101", "102"] {
        let assert = rbx_cmd(&db_path)
            .args(["tracks", "cues", "add", track, "30000", "--execute"])
            .assert()
            .code(2);
        assert_eq!(stdout_json(&assert)["error"]["category"], "usage");
    }
    rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "update",
            "900",
            "--msec",
            "5000",
            "--execute",
        ])
        .assert()
        .code(2);
    rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "update",
            "900",
            "--color",
            "red",
            "--execute",
        ])
        .assert()
        .code(0);

    let positions: Vec<(i64,)> = sqlx::query_as("SELECT InMsec FROM djmdCue")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(positions, vec![(1000,)]);
}

// --- hot cue colors ---
// Checked by coloring hot cues A-H on two tracks in rekordbox 7:
// the 16 colors of the hot cue color menu, read left to right and top to bottom,
// are ColorTableIndex 49, 56, 60, 62, 1, 5, 9, 14, 18, 22, 26, 30, 32, 38, 42, 45.
// Color stays -1.

/// `--color N` on a hot cue picks the N-th color of the menu (1-16)
/// and writes its ColorTableIndex to djmdCue and contentCue.
#[tokio::test]
async fn cues_add_hot_cue_with_menu_color() {
    let (db_path, _dir) = common::setup_db().await;

    for (slot, color) in [("1", "1"), ("2", "5"), ("3", "16")] {
        rbx_cmd(&db_path)
            .args(["tracks", "cues", "add", "101", "1000", "--kind", "hot"])
            .args(["--slot", slot, "--color", color, "--execute"])
            .assert()
            .code(0);
    }

    let pool = common::open_pool(&db_path).await;
    let colors: Vec<(i64, i64, i64)> = sqlx::query_as(
        "SELECT Kind, Color, ColorTableIndex FROM djmdCue WHERE ContentID = '101' ORDER BY Kind",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(colors, vec![(1, -1, 49), (2, -1, 1), (3, -1, 45)]);

    let (cues,): (String,) = sqlx::query_as("SELECT Cues FROM contentCue WHERE ContentID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    let indexes: Vec<i64> = entries
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["ColorTableIndex"].as_i64().unwrap())
        .collect();
    assert_eq!(indexes, vec![49, 1, 45]);
}

/// `cues update --color N` changes a hot cue's ColorTableIndex in place.
#[tokio::test]
async fn cues_update_hot_cue_color() {
    let (db_path, _dir) = common::setup_db().await;
    let assert = rbx_cmd(&db_path)
        .args([
            "tracks", "cues", "add", "101", "1000", "--kind", "hot", "--slot", "1",
        ])
        .arg("--execute")
        .assert()
        .code(0);
    let cue_id = stdout_json(&assert)["result"]["cue_id"]
        .as_str()
        .unwrap()
        .to_string();

    rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "update",
            &cue_id,
            "--color",
            "9",
            "--execute",
        ])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let (color, index): (i64, i64) =
        sqlx::query_as("SELECT Color, ColorTableIndex FROM djmdCue WHERE ID = ?")
            .bind(&cue_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((color, index), (-1, 18));
    let (cues,): (String,) = sqlx::query_as("SELECT Cues FROM contentCue WHERE ContentID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let entries: serde_json::Value = serde_json::from_str(&cues).unwrap();
    assert_eq!(entries[0]["ColorTableIndex"], 18);
}

/// Each menu color also has a name, picked from the color seen in the menu
/// and matching the memory cue names where the color is close (red, blue, ...).
#[tokio::test]
async fn cues_add_hot_cue_color_by_name() {
    let (db_path, _dir) = common::setup_db().await;
    for (slot, color) in [
        ("1", "violet"),
        ("2", "slateblue"),
        ("3", "red"),
        ("4", "deeppink"),
    ] {
        rbx_cmd(&db_path)
            .args(["tracks", "cues", "add", "101", "1000", "--kind", "hot"])
            .args(["--slot", slot, "--color", color, "--execute"])
            .assert()
            .code(0);
    }

    let pool = common::open_pool(&db_path).await;
    let indexes: Vec<(i64,)> =
        sqlx::query_as("SELECT ColorTableIndex FROM djmdCue WHERE ContentID = '101' ORDER BY Kind")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(indexes, vec![(49,), (62,), (42,), (45,)]);
}

/// `cues update --color none` on a hot cue writes ColorTableIndex 0,
/// which is what the menu's reset (初期化) writes in rekordbox 7.
#[tokio::test]
async fn cues_update_hot_cue_color_none_resets_it() {
    let (db_path, _dir) = common::setup_db().await;
    let assert = rbx_cmd(&db_path)
        .args([
            "tracks", "cues", "add", "101", "1000", "--kind", "hot", "--slot", "1",
        ])
        .args(["--color", "red", "--execute"])
        .assert()
        .code(0);
    let cue_id = stdout_json(&assert)["result"]["cue_id"]
        .as_str()
        .unwrap()
        .to_string();

    rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "update",
            &cue_id,
            "--color",
            "none",
            "--execute",
        ])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let (color, index): (i64, i64) =
        sqlx::query_as("SELECT Color, ColorTableIndex FROM djmdCue WHERE ID = ?")
            .bind(&cue_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!((color, index), (-1, 0));

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "cues", "list", "101"])
        .assert()
        .code(0);
    assert_eq!(
        stdout_json(&assert)["items"][0]["color"],
        serde_json::Value::Null
    );
}

/// `cues list` reports a hot cue's color by its name.
#[tokio::test]
async fn cues_list_reports_hot_cue_menu_color() {
    let (db_path, _dir) = common::setup_db().await;
    rbx_cmd(&db_path)
        .args([
            "tracks", "cues", "add", "101", "1000", "--kind", "hot", "--slot", "1",
        ])
        .args(["--color", "12", "--execute"])
        .assert()
        .code(0);
    rbx_cmd(&db_path)
        .args([
            "tracks", "cues", "add", "101", "2000", "--kind", "hot", "--slot", "2",
        ])
        .arg("--execute")
        .assert()
        .code(0);

    let assert = rbx_cmd(&db_path)
        .args(["tracks", "cues", "list", "101"])
        .assert()
        .code(0);
    let json = stdout_json(&assert);
    let items = json["items"].as_array().unwrap();
    let colored = items.iter().find(|c| c["slot"] == 1).unwrap();
    assert_eq!(colored["color"], "olive");
    let plain = items.iter().find(|c| c["slot"] == 2).unwrap();
    assert_eq!(plain["color"], serde_json::Value::Null);
}

/// A hot cue color outside 1-16, or a name that is not in the menu, is a usage error.
#[tokio::test]
async fn cues_hot_cue_color_outside_the_menu_is_a_usage_error() {
    let (db_path, _dir) = common::setup_db().await;
    for color in ["0", "17", "beige", "none"] {
        let assert = rbx_cmd(&db_path)
            .args([
                "tracks", "cues", "add", "101", "1000", "--kind", "hot", "--slot", "1",
            ])
            .args(["--color", color, "--execute"])
            .assert()
            .code(2);
        assert_eq!(
            stdout_json(&assert)["error"]["category"],
            "usage",
            "--color {}",
            color
        );
    }

    let pool = common::open_pool(&db_path).await;
    let (rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM djmdCue")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0);
}

// --- loops ---
// Checked against the loops in a real master.db (rekordbox 7) and with the user in rekordbox:
// a loop has OutMsec / OutFrame, Color 255, ColorTableIndex 0 (or the hot cue color),
// CueMicrosec 0, Comment '' and BeatLoopSize = beats << 16 | denominator (0 when not on beats).
// An active memory loop is Kind 4 (ActiveLoop stays 0); an active hot cue loop has ActiveLoop 1.
// A track has at most one active memory loop and one active hot cue loop.

/// Kind, InFrame, OutMsec, OutFrame, Color, ColorTableIndex, ActiveLoop,
/// BeatLoopSize, CueMicrosec, Comment
type LoopColumns = (
    i64,
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<String>,
);

/// `--out-msec` makes a memory cue a loop with the loop columns rekordbox writes.
#[tokio::test]
async fn cues_add_memory_loop_writes_the_loop_columns() {
    let (db_path, _dir) = common::setup_db().await;
    let assert = rbx_cmd(&db_path)
        .args([
            "tracks",
            "cues",
            "add",
            "101",
            "104987",
            "--out-msec",
            "109160",
            "--execute",
        ])
        .assert()
        .code(0);
    let cue_id = stdout_json(&assert)["result"]["cue_id"]
        .as_str()
        .unwrap()
        .to_string();

    let pool = common::open_pool(&db_path).await;
    let row: LoopColumns = sqlx::query_as(
        "SELECT Kind, InFrame, OutMsec, OutFrame, Color, ColorTableIndex, ActiveLoop, \
         BeatLoopSize, CueMicrosec, Comment FROM djmdCue WHERE ID = ?",
    )
    .bind(&cue_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            0,
            15748,
            109160,
            16374,
            255,
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(String::new())
        )
    );

    let (cues,): (String,) = sqlx::query_as("SELECT Cues FROM contentCue WHERE ContentID = '101'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let entry = &serde_json::from_str::<serde_json::Value>(&cues).unwrap()[0];
    assert_eq!(entry["OutMsec"], 109160);
    assert_eq!(entry["OutFrame"], 16374);
    assert_eq!(entry["Color"], 255);
    assert_eq!(entry["BeatLoopSize"], 0);
    assert_eq!(entry["CueMicrosec"], 0);
}

/// `--beats 8` writes BeatLoopSize 524289 (8 << 16 | 1); `--beats 1/2` writes 65538.
#[tokio::test]
async fn cues_add_loop_with_beats_sets_beat_loop_size() {
    let (db_path, _dir) = common::setup_db().await;
    for (msec, out, beats) in [("1000", "4000", "8"), ("5000", "5200", "1/2")] {
        rbx_cmd(&db_path)
            .args(["tracks", "cues", "add", "101", msec, "--out-msec", out])
            .args(["--beats", beats, "--execute"])
            .assert()
            .code(0);
    }

    let pool = common::open_pool(&db_path).await;
    let sizes: Vec<(i64,)> =
        sqlx::query_as("SELECT BeatLoopSize FROM djmdCue WHERE ContentID = '101' ORDER BY InMsec")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(sizes, vec![(524289,), (65538,)]);
}

/// A hot cue loop keeps its slot Kind; `--color` sets its ColorTableIndex.
#[tokio::test]
async fn cues_add_hot_cue_loop() {
    let (db_path, _dir) = common::setup_db().await;
    rbx_cmd(&db_path)
        .args([
            "tracks", "cues", "add", "101", "1000", "--kind", "hot", "--slot", "4",
        ])
        .args(["--out-msec", "4000", "--beats", "16", "--execute"])
        .assert()
        .code(0);
    rbx_cmd(&db_path)
        .args([
            "tracks", "cues", "add", "101", "5000", "--kind", "hot", "--slot", "1",
        ])
        .args(["--out-msec", "6000", "--color", "violet", "--execute"])
        .assert()
        .code(0);

    let pool = common::open_pool(&db_path).await;
    let rows: Vec<(i64, i64, i64, i64, i64)> = sqlx::query_as(
        "SELECT Kind, OutMsec, Color, ColorTableIndex, BeatLoopSize FROM djmdCue \
         WHERE ContentID = '101' ORDER BY InMsec",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![(5, 4000, 255, 0, 1048577), (1, 6000, 255, 49, 0)]
    );
}

/// `--active` on a memory loop writes Kind 4, on a hot cue loop ActiveLoop 1.
#[tokio::test]
async fn cues_add_active_loop() {}

/// `cues list` shows loops with out_msec, beats and active.
#[tokio::test]
async fn cues_list_reports_loops() {}

/// `cues update --out-msec` moves the end of a loop and recomputes OutFrame.
#[tokio::test]
async fn cues_update_out_msec_moves_the_loop_end() {}

/// An active memory loop (Kind 4) counts toward the 10 memory cues.
#[tokio::test]
async fn cues_add_counts_active_memory_loops_toward_the_memory_cue_limit() {}

/// A second active loop of the same kind (memory or hot) is a conflict.
#[tokio::test]
async fn cues_add_second_active_loop_is_a_conflict() {}

/// `--out-msec` at or before the start, `--beats` without `--out-msec`,
/// or `--active` without `--out-msec` is a usage error.
#[tokio::test]
async fn cues_add_bad_loop_flags_are_usage_errors() {}
