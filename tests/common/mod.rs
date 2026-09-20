#![allow(dead_code)]

use std::path::{Path, PathBuf};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use tempfile::TempDir;

const RB_COLUMNS: &str = "\
    UUID TEXT, \
    rb_data_status INTEGER DEFAULT 0, \
    rb_local_data_status INTEGER DEFAULT 0, \
    rb_local_deleted INTEGER DEFAULT 0, \
    rb_local_synced INTEGER DEFAULT 0, \
    usn TEXT, \
    rb_local_usn INTEGER, \
    created_at TEXT, \
    updated_at TEXT";

/// Native-format timestamp used for seed rows.
pub const SEED_TS: &str = "2026-01-01 00:00:00.000 +00:00";

/// Creates an unencrypted rekordbox-shaped fixture DB and returns its path.
/// rbx's db::open falls back to keyless mode for unencrypted files.
pub async fn setup_db() -> (PathBuf, TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("master.db");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&db_path)
                .create_if_missing(true),
        )
        .await
        .unwrap();

    create_schema(&pool).await;
    seed(&pool).await;
    pool.close().await;

    (db_path, dir)
}

/// Opens the fixture DB through rbx's own opener (same code path as the CLI).
pub async fn open_pool(path: &Path) -> SqlitePool {
    rbx::db::open(path, false).await.unwrap()
}

async fn create_schema(pool: &SqlitePool) {
    let tables = [
        format!(
            "CREATE TABLE djmdContent (\
             ID TEXT PRIMARY KEY, Title TEXT, ArtistID TEXT, KeyID TEXT, \
             Length INTEGER, BPM INTEGER, FolderPath TEXT, FileNameL TEXT, FileNameS TEXT, \
             Rating INTEGER, ColorID TEXT, Commnt TEXT, \
             GenreID TEXT, AlbumID TEXT, TrackNo INTEGER, DiscNo INTEGER, ReleaseYear INTEGER, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdArtist (\
             ID TEXT PRIMARY KEY, Name TEXT, SearchStr TEXT, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdGenre (ID TEXT PRIMARY KEY, Name TEXT, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdAlbum (\
             ID TEXT PRIMARY KEY, Name TEXT, AlbumArtistID TEXT, ImagePath TEXT, \
             Compilation INTEGER, SearchStr TEXT, {})",
            RB_COLUMNS
        ),
        "CREATE TABLE djmdKey (ID TEXT PRIMARY KEY, ScaleName TEXT, Seq INTEGER)".to_string(),
        format!(
            "CREATE TABLE djmdPlaylist (\
             ID TEXT PRIMARY KEY, Seq INTEGER, Name TEXT, ImagePath TEXT, \
             Attribute INTEGER, ParentID TEXT, SmartList TEXT, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdSongPlaylist (\
             ID TEXT PRIMARY KEY, PlaylistID TEXT, ContentID TEXT, TrackNo INTEGER, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdMyTag (\
             ID TEXT PRIMARY KEY, Seq INTEGER, Name TEXT, Attribute INTEGER, \
             ParentID TEXT, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdSongMyTag (\
             ID TEXT PRIMARY KEY, MyTagID TEXT, ContentID TEXT, TrackNo INTEGER, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdCue (\
             ID TEXT PRIMARY KEY, ContentID TEXT, InMsec INTEGER, InFrame INTEGER, \
             InMpegFrame INTEGER, InMpegAbs INTEGER, OutMsec INTEGER, OutFrame INTEGER, \
             OutMpegFrame INTEGER, OutMpegAbs INTEGER, Kind INTEGER, Color INTEGER, \
             ColorTableIndex INTEGER, ActiveLoop INTEGER, Comment TEXT, \
             BeatLoopSize INTEGER, CueMicrosec INTEGER, InPointSeekInfo TEXT, \
             OutPointSeekInfo TEXT, ContentUUID TEXT, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdHistory (\
             ID TEXT PRIMARY KEY, Seq INTEGER, Name TEXT, Attribute INTEGER, \
             ParentID TEXT, DateCreated TEXT, {})",
            RB_COLUMNS
        ),
        format!(
            "CREATE TABLE djmdSongHistory (\
             ID TEXT PRIMARY KEY, HistoryID TEXT, ContentID TEXT, TrackNo INTEGER, {})",
            RB_COLUMNS
        ),
        "CREATE TABLE agentRegistry (\
         registry_id TEXT PRIMARY KEY, id_1 TEXT, id_2 TEXT, \
         int_1 INTEGER, int_2 INTEGER, str_1 TEXT, str_2 TEXT, \
         date_1 TEXT, date_2 TEXT, text_1 TEXT, text_2 TEXT, \
         created_at TEXT, updated_at TEXT)"
            .to_string(),
    ];
    for sql in &tables {
        sqlx::query(sql).execute(pool).await.unwrap();
    }
}

async fn seed(pool: &SqlitePool) {
    sqlx::query(
        "INSERT INTO agentRegistry (registry_id, int_1, created_at, updated_at) \
         VALUES ('localUpdateCount', 1000, ?, ?)",
    )
    .bind(SEED_TS)
    .bind(SEED_TS)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO djmdArtist (ID, Name, SearchStr, UUID, rb_local_usn, created_at, updated_at) \
         VALUES ('201', 'Artist A', 'Artist A', 'a0000000-0000-0000-0000-000000000201', 10, ?, ?)",
    )
    .bind(SEED_TS)
    .bind(SEED_TS)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO djmdGenre (ID, Name, UUID, rb_local_usn, created_at, updated_at) \
         VALUES ('501', 'Anime', 'g0000000-0000-0000-0000-000000000501', 10, ?, ?)",
    )
    .bind(SEED_TS)
    .bind(SEED_TS)
    .execute(pool)
    .await
    .unwrap();

    sqlx::query("INSERT INTO djmdKey (ID, ScaleName, Seq) VALUES ('301', '8A', 1)")
        .execute(pool)
        .await
        .unwrap();

    for (id, title, path) in [
        ("101", "Track One", "C:/Music/one.mp3"),
        ("102", "Track Two", "C:/Music/two.mp3"),
    ] {
        sqlx::query(
            "INSERT INTO djmdContent \
             (ID, Title, ArtistID, KeyID, Length, BPM, FolderPath, UUID, rb_local_usn, created_at, updated_at) \
             VALUES (?, ?, '201', '301', 240, 12800, ?, ?, 10, ?, ?)",
        )
        .bind(id)
        .bind(title)
        .bind(path)
        .bind(format!("c0000000-0000-0000-0000-000000000{}", id))
        .bind(SEED_TS)
        .bind(SEED_TS)
        .execute(pool)
        .await
        .unwrap();
    }

    // My Tag category with two tags under it
    sqlx::query(
        "INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID, UUID, rb_local_usn, created_at, updated_at) \
         VALUES ('401', 1, 'Genre', 1, 'root', 't0000000-0000-0000-0000-000000000401', 10, ?, ?)",
    )
    .bind(SEED_TS)
    .bind(SEED_TS)
    .execute(pool)
    .await
    .unwrap();
    for (id, name, seq) in [("402", "Bright", 1), ("403", "Dark", 2)] {
        sqlx::query(
            "INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID, UUID, rb_local_usn, created_at, updated_at) \
             VALUES (?, ?, ?, 0, '401', ?, 10, ?, ?)",
        )
        .bind(id)
        .bind(seq)
        .bind(name)
        .bind(format!("t0000000-0000-0000-0000-000000000{}", id))
        .bind(SEED_TS)
        .bind(SEED_TS)
        .execute(pool)
        .await
        .unwrap();
    }

    sqlx::query(
        "INSERT INTO djmdPlaylist \
         (ID, Seq, Name, Attribute, ParentID, UUID, rb_local_usn, created_at, updated_at) \
         VALUES ('501', 1, 'Existing', 0, 'root', 'p0000000-0000-0000-0000-000000000501', 10, ?, ?)",
    )
    .bind(SEED_TS)
    .bind(SEED_TS)
    .execute(pool)
    .await
    .unwrap();
}
