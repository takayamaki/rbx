use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};
use rbx::helpers::{allocate_usns, generate_numeric_id, now_datetime};
use rbx::{db, output, playlist_xml};
use sqlx::sqlite::SqlitePool;
use sqlx::{Column, FromRow, Row as _};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "rbx", about = "CLI tool for rekordbox master.db",
    after_help = "For machine-readable schema info, use: rbx describe [resource] [action]")]
struct Cli {
    /// Path to rekordbox master.db
    #[arg(long, env = "RBX_DB_PATH")]
    db: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Query and manage tracks
    Tracks {
        #[command(subcommand)]
        action: TracksAction,
    },
    /// Query playlists
    Playlists {
        #[command(subcommand)]
        action: PlaylistsAction,
    },
    /// Manage My Tags (categories and tags themselves)
    Mytags {
        #[command(subcommand)]
        action: MytagsAction,
    },
    /// Query play history
    History {
        #[command(subcommand)]
        action: HistoryAction,
    },
    /// Run raw SQL query (read-only by default)
    Query {
        /// SQL to execute
        sql: String,
        /// Allow writes. Bypasses all rekordbox invariants (USN allocation,
        /// timestamp format, numeric IDs, masterPlaylists6.xml sync) —
        /// prefer the dedicated commands
        #[arg(long)]
        unsafe_write: bool,
    },
    /// Describe available commands and their schemas
    Describe {
        /// Resource name (e.g. "tracks", "playlists", "mytags")
        resource: Option<String>,
        /// Action name (e.g. "list", "get", "search")
        action: Option<String>,
    },
}

#[derive(Subcommand)]
enum TracksAction {
    /// List all tracks
    List,
    /// Get a track by ID
    Get { id: String },
    /// Search tracks by title or artist
    Search { query: String },
    /// Filter tracks by BPM, key, and/or tag
    Filter {
        #[arg(long)]
        bpm_min: Option<f64>,
        #[arg(long)]
        bpm_max: Option<f64>,
        /// Musical key (e.g. "8A")
        #[arg(long)]
        key: Option<String>,
        /// My Tag ID to filter by
        #[arg(long)]
        tag: Option<String>,
    },
    /// Update track fields (dry-run by default)
    Update {
        /// Track ID
        id: String,
        #[arg(long)]
        title: Option<String>,
        /// Artist name (resolved or created in djmdArtist; "" clears)
        #[arg(long)]
        artist: Option<String>,
        /// Genre name (resolved or created in djmdGenre; "" clears)
        #[arg(long)]
        genre: Option<String>,
        /// BPM as decimal (e.g. 128.0)
        #[arg(long)]
        bpm: Option<f64>,
        /// Musical key (e.g. "8A", "1B")
        #[arg(long)]
        key: Option<String>,
        /// Rating (0-5)
        #[arg(long)]
        rating: Option<i32>,
        /// Comment text
        #[arg(long)]
        comment: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Manage My Tags on a track
    Mytags {
        #[command(subcommand)]
        action: TrackMytagsAction,
    },
    /// Manage cue points on a track
    Cues {
        #[command(subcommand)]
        action: TrackCuesAction,
    },
}

#[derive(Subcommand)]
enum TrackCuesAction {
    /// List cue points on a track
    List { track_id: String },
    /// Add a cue point (dry-run by default)
    Add {
        track_id: String,
        /// Position in milliseconds
        msec: i64,
        /// Cue type: "memory" (default) or "hot"
        #[arg(long, default_value = "memory")]
        kind: String,
        /// Hot cue slot (1-8, required for hot cues)
        #[arg(long)]
        slot: Option<i32>,
        /// Cue comment/name
        #[arg(long)]
        comment: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Update a cue point (dry-run by default)
    Update {
        /// Cue ID
        cue_id: String,
        /// Position in milliseconds
        #[arg(long)]
        msec: Option<i64>,
        /// Cue comment/name
        #[arg(long)]
        comment: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Delete a cue point (dry-run by default)
    Delete {
        /// Cue ID
        cue_id: String,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
enum TrackMytagsAction {
    /// List My Tags assigned to a track
    List { track_id: String },
    /// Add My Tags to a track (dry-run by default)
    Add {
        track_id: String,
        /// One or more tag IDs
        tag_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Remove My Tags from a track (dry-run by default)
    Remove {
        track_id: String,
        /// One or more tag IDs
        tag_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
enum PlaylistsAction {
    /// List all playlists (folders and playlists)
    List,
    /// Manage tracks in a playlist
    Tracks {
        #[command(subcommand)]
        action: PlaylistTracksAction,
    },
    /// Find playlists containing a specific track
    Search { track_id: String },
    /// Create a new playlist (dry-run by default)
    Create {
        /// Playlist name
        name: String,
        /// Parent folder ID (omit for top-level)
        #[arg(long)]
        parent: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Delete a playlist (dry-run by default)
    Delete {
        id: String,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
enum PlaylistTracksAction {
    /// List tracks in a playlist
    List { playlist_id: String },
    /// Add tracks to a playlist (dry-run by default)
    Add {
        playlist_id: String,
        /// One or more track IDs
        track_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Remove tracks from a playlist (dry-run by default)
    Remove {
        playlist_id: String,
        /// One or more track IDs
        track_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
enum HistoryAction {
    /// List play history sessions
    List {
        /// Max number of sessions to return
        #[arg(long, default_value = "20")]
        limit: i32,
    },
    /// List tracks in a history session
    Tracks { id: String },
}

#[derive(Subcommand)]
enum MytagsAction {
    /// List all My Tags (categories and tags)
    List,
    /// List tracks with a specific My Tag
    Tracks { id: String },
    /// Create a new My Tag (dry-run by default)
    Create {
        /// Tag name
        name: String,
        /// Parent category ID (required for tags, omit for top-level categories)
        #[arg(long)]
        parent: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Delete a My Tag (dry-run by default)
    Delete {
        /// My Tag ID
        id: String,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

// --- Row types ---

#[derive(Debug, FromRow)]
struct TrackRow {
    id: String,
    title: Option<String>,
    artist_name: Option<String>,
    duration: Option<i32>,
    bpm: Option<i32>,
    key_name: Option<String>,
    folder_path: Option<String>,
}

impl TrackRow {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "title": self.title,
            "artist": self.artist_name,
            "duration_sec": self.duration,
            "bpm": self.bpm.map(|b| b as f64 / 100.0),
            "key": self.key_name,
            "folder_path": self.folder_path,
        })
    }
}

#[derive(Debug, FromRow)]
struct PlaylistRow {
    id: String,
    name: Option<String>,
    attribute: Option<i32>,
    parent_id: Option<String>,
}

impl PlaylistRow {
    fn to_json(&self) -> serde_json::Value {
        let kind = if self.attribute == Some(1) { "folder" } else { "playlist" };
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "kind": kind,
            "parent_id": self.parent_id,
        })
    }
}

#[derive(Debug, FromRow)]
struct PlaylistTrackRow {
    track_no: i32,
    content_id: String,
    title: Option<String>,
    artist_name: Option<String>,
    bpm: Option<i32>,
    key_name: Option<String>,
}

impl PlaylistTrackRow {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "track_no": self.track_no,
            "id": self.content_id,
            "title": self.title,
            "artist": self.artist_name,
            "bpm": self.bpm.map(|b| b as f64 / 100.0),
            "key": self.key_name,
        })
    }
}

#[derive(Debug, FromRow)]
struct MyTagRow {
    id: String,
    seq: Option<i32>,
    name: Option<String>,
    attribute: Option<i32>,
    parent_id: Option<String>,
}

impl MyTagRow {
    fn to_json(&self) -> serde_json::Value {
        let kind = if self.attribute == Some(1) { "category" } else { "tag" };
        serde_json::json!({
            "id": self.id,
            "seq": self.seq,
            "name": self.name,
            "kind": kind,
            "parent_id": self.parent_id,
        })
    }
}

#[derive(Debug, FromRow)]
struct MyTagTrackRow {
    content_id: String,
    title: Option<String>,
    artist_name: Option<String>,
    bpm: Option<i32>,
    key_name: Option<String>,
}

impl MyTagTrackRow {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.content_id,
            "title": self.title,
            "artist": self.artist_name,
            "bpm": self.bpm.map(|b| b as f64 / 100.0),
            "key": self.key_name,
        })
    }
}

#[derive(Debug, FromRow)]
struct TrackMyTagRow {
    tag_id: String,
    tag_name: Option<String>,
    category_name: Option<String>,
}

impl TrackMyTagRow {
    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "tag_id": self.tag_id,
            "tag_name": self.tag_name,
            "category_name": self.category_name,
        })
    }
}

#[derive(Debug, FromRow)]
struct CueRow {
    id: String,
    content_id: String,
    in_msec: Option<i64>,
    out_msec: Option<i64>,
    kind: Option<i32>,
    color: Option<i32>,
    comment: Option<String>,
}

impl CueRow {
    fn to_json(&self) -> serde_json::Value {
        let kind_str = match self.kind {
            Some(0) => "memory",
            Some(k @ 1..=8) => return serde_json::json!({
                "id": self.id,
                "track_id": self.content_id,
                "kind": "hot",
                "slot": k,
                "in_msec": self.in_msec,
                "out_msec": self.out_msec.filter(|&v| v >= 0),
                "color": self.color.filter(|&v| v >= 0),
                "comment": self.comment,
            }),
            _ => "other",
        };
        serde_json::json!({
            "id": self.id,
            "track_id": self.content_id,
            "kind": kind_str,
            "in_msec": self.in_msec,
            "out_msec": self.out_msec.filter(|&v| v >= 0),
            "color": self.color.filter(|&v| v >= 0),
            "comment": self.comment,
        })
    }
}

// --- Helpers ---

const TRACK_QUERY_BASE: &str = "\
    SELECT c.ID as id, c.Title as title, a.Name as artist_name, \
    c.Length as duration, c.BPM as bpm, k.ScaleName as key_name, \
    c.FolderPath as folder_path \
    FROM djmdContent c \
    LEFT JOIN djmdArtist a ON c.ArtistID = a.ID \
    LEFT JOIN djmdKey k ON c.KeyID = k.ID";

const TRACK_FILTER_LOCAL: &str = "\
    c.FolderPath NOT LIKE 'spotify:%' \
    AND c.FolderPath NOT LIKE 'apple:%' \
    AND c.FolderPath NOT LIKE 'itunes:%' \
    AND c.Title IS NOT NULL AND c.Title != ''";

fn needs_write(cmd: &Commands) -> bool {
    matches!(cmd, Commands::Query { unsafe_write: true, .. }
        | Commands::Tracks { action: TracksAction::Update { execute: true, .. }
            | TracksAction::Mytags {
                action: TrackMytagsAction::Add { execute: true, .. }
                      | TrackMytagsAction::Remove { execute: true, .. },
            }
            | TracksAction::Cues {
                action: TrackCuesAction::Add { execute: true, .. }
                      | TrackCuesAction::Update { execute: true, .. }
                      | TrackCuesAction::Delete { execute: true, .. },
            }}
        | Commands::Mytags {
            action: MytagsAction::Create { execute: true, .. }
                  | MytagsAction::Delete { execute: true, .. },
        }
        | Commands::Playlists {
            action: PlaylistsAction::Create { execute: true, .. }
                  | PlaylistsAction::Delete { execute: true, .. }
                  | PlaylistsAction::Tracks { action: PlaylistTracksAction::Add { execute: true, .. }
                      | PlaylistTracksAction::Remove { execute: true, .. } },
        }
    )
}

async fn resolve_tag_name(pool: &SqlitePool, tag_id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>(
        "SELECT Name FROM djmdMyTag WHERE ID = ? AND rb_local_deleted = 0"
    ).bind(tag_id).fetch_optional(pool).await.map(|r| r.map(|(n,)| n))
}

async fn resolve_track_summary(pool: &SqlitePool, content_id: &str) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT c.Title, COALESCE(a.Name, '') \
         FROM djmdContent c LEFT JOIN djmdArtist a ON c.ArtistID = a.ID \
         WHERE c.ID = ?"
    ).bind(content_id).fetch_optional(pool).await
}

fn db_error(e: sqlx::Error) -> (serde_json::Value, i32) {
    (
        output::error("database", output::EXIT_GENERAL, &e.to_string(), None),
        output::EXIT_GENERAL,
    )
}

// --- Main ---

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if matches!(cli.command, Commands::Describe { .. }) {
        let (out, code) = match cli.command {
            Commands::Describe { resource, action } => (handle_describe(resource, action), output::EXIT_OK),
            _ => unreachable!(),
        };
        output::print(&out);
        if code != output::EXIT_OK {
            std::process::exit(code);
        }
        return;
    }

    let db_path = match &cli.db {
        Some(p) => p.clone(),
        None => {
            output::print(&output::error(
                "config", output::EXIT_CONFIG, "Missing --db path",
                Some("Set --db or RBX_DB_PATH environment variable"),
            ));
            std::process::exit(output::EXIT_CONFIG);
        }
    };

    let read_only = !needs_write(&cli.command);
    let pool = match db::open(&db_path, read_only).await {
        Ok(p) => p,
        Err(e) => {
            output::print(&output::error(
                "config", output::EXIT_CONFIG,
                &format!("Failed to open database: {}", e),
                Some("Check that --db points to a valid rekordbox master.db"),
            ));
            std::process::exit(output::EXIT_CONFIG);
        }
    };

    let (out, code) = match cli.command {
        Commands::Tracks { action } => handle_tracks(&pool, action).await,
        Commands::Playlists { action } => handle_playlists(&pool, &db_path, action).await,
        Commands::Mytags { action } => handle_mytags(&pool, action).await,
        Commands::History { action } => handle_history(&pool, action).await,
        Commands::Query { sql, unsafe_write } => handle_query(&pool, &sql, unsafe_write).await,
        Commands::Describe { .. } => unreachable!(),
    };

    output::print(&out);
    if code != output::EXIT_OK {
        std::process::exit(code);
    }
}

// --- Handlers: tracks ---

async fn handle_tracks(pool: &SqlitePool, action: TracksAction) -> (serde_json::Value, i32) {
    match action {
        TracksAction::List => {
            let sql = format!("{} WHERE {}", TRACK_QUERY_BASE, TRACK_FILTER_LOCAL);
            match sqlx::query_as::<_, TrackRow>(&sql).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("tracks", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        TracksAction::Get { id } => {
            let sql = format!("{} WHERE c.ID = ?", TRACK_QUERY_BASE);
            match sqlx::query_as::<_, TrackRow>(&sql).bind(&id).fetch_optional(pool).await {
                Ok(Some(row)) => (output::success_one("track", row.to_json()), output::EXIT_OK),
                Ok(None) => (
                    output::error("not_found", output::EXIT_NOT_FOUND,
                        &format!("Track not found: {}", id),
                        Some("Use 'rbx tracks list' to see available tracks")),
                    output::EXIT_NOT_FOUND,
                ),
                Err(e) => db_error(e),
            }
        }
        TracksAction::Search { query } => {
            let sql = format!(
                "{} WHERE (c.Title LIKE ?1 OR a.Name LIKE ?1) AND {}",
                TRACK_QUERY_BASE, TRACK_FILTER_LOCAL
            );
            let pattern = format!("%{}%", query);
            match sqlx::query_as::<_, TrackRow>(&sql).bind(&pattern).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("tracks", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        TracksAction::Filter { bpm_min, bpm_max, key, tag } => {
            handle_tracks_filter(pool, bpm_min, bpm_max, key, tag).await
        }
        TracksAction::Update { id, title, artist, genre, bpm, key, rating, comment, execute } => {
            let fields = TrackFields { title, artist, genre, bpm, key, rating, comment };
            handle_tracks_update(pool, &id, fields, execute).await
        }
        TracksAction::Mytags { action } => handle_track_mytags(pool, action).await,
        TracksAction::Cues { action } => handle_track_cues(pool, action).await,
    }
}

// --- Handlers: tracks filter ---

async fn handle_tracks_filter(
    pool: &SqlitePool, bpm_min: Option<f64>, bpm_max: Option<f64>,
    key: Option<String>, tag: Option<String>,
) -> (serde_json::Value, i32) {
    let mut conditions = vec![TRACK_FILTER_LOCAL.to_string()];
    // BPM is stored as int * 100
    if let Some(min) = bpm_min {
        conditions.push(format!("c.BPM >= {}", (min * 100.0) as i32));
    }
    if let Some(max) = bpm_max {
        conditions.push(format!("c.BPM <= {}", (max * 100.0) as i32));
    }
    if let Some(ref k) = key {
        conditions.push(format!("k.ScaleName = '{}'", k.replace('\'', "''")));
    }
    if let Some(ref t) = tag {
        conditions.push(format!(
            "c.ID IN (SELECT ContentID FROM djmdSongMyTag WHERE MyTagID = '{}' AND rb_local_deleted = 0)",
            t.replace('\'', "''")
        ));
    }

    let sql = format!("{} WHERE {}", TRACK_QUERY_BASE, conditions.join(" AND "));
    match sqlx::query_as::<_, TrackRow>(&sql).fetch_all(pool).await {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
            (output::success("tracks", serde_json::Value::Array(items)), output::EXIT_OK)
        }
        Err(e) => db_error(e),
    }
}

// --- Handlers: tracks cues ---

async fn handle_track_cues(pool: &SqlitePool, action: TrackCuesAction) -> (serde_json::Value, i32) {
    match action {
        TrackCuesAction::List { track_id } => {
            if resolve_track_summary(pool, &track_id).await.ok().flatten().is_none() {
                return (
                    output::error("not_found", output::EXIT_NOT_FOUND,
                        &format!("Track not found: {}", track_id),
                        Some("Use 'rbx tracks list' to see available tracks")),
                    output::EXIT_NOT_FOUND,
                );
            }
            match sqlx::query_as::<_, CueRow>(
                "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
                 OutMsec as out_msec, Kind as kind, Color as color, Comment as comment \
                 FROM djmdCue WHERE ContentID = ? AND rb_local_deleted = 0 \
                 ORDER BY Kind, InMsec"
            ).bind(&track_id).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("track_cues", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        TrackCuesAction::Add { track_id, msec, kind, slot, comment, execute } => {
            handle_track_cue_add(pool, &track_id, msec, &kind, slot, comment, execute).await
        }
        TrackCuesAction::Update { cue_id, msec, comment, execute } => {
            handle_track_cue_update(pool, &cue_id, msec, comment, execute).await
        }
        TrackCuesAction::Delete { cue_id, execute } => {
            handle_track_cue_delete(pool, &cue_id, execute).await
        }
    }
}

async fn handle_track_cue_add(
    pool: &SqlitePool, track_id: &str, msec: i64, kind: &str,
    slot: Option<i32>, comment: Option<String>, execute: bool,
) -> (serde_json::Value, i32) {
    let (title, artist) = match resolve_track_summary(pool, track_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Track not found: {}", track_id),
                Some("Use 'rbx tracks list' to see available tracks")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    let kind_int = match kind {
        "memory" => 0,
        "hot" => match slot {
            Some(s) if (1..=8).contains(&s) => s,
            Some(s) => return (
                output::error("validation", output::EXIT_CONFLICT,
                    &format!("Hot cue slot must be 1-8, got: {}", s), None),
                output::EXIT_CONFLICT,
            ),
            None => return (
                output::error("validation", output::EXIT_CONFLICT,
                    "Hot cue requires --slot (1-8)", None),
                output::EXIT_CONFLICT,
            ),
        },
        _ => return (
            output::error("validation", output::EXIT_CONFLICT,
                &format!("Unknown cue kind: {} (use 'memory' or 'hot')", kind), None),
            output::EXIT_CONFLICT,
        ),
    };

    // Check for slot conflict on hot cues
    if kind_int >= 1 {
        let existing = sqlx::query_as::<_, (String,)>(
            "SELECT ID FROM djmdCue WHERE ContentID = ? AND Kind = ? AND rb_local_deleted = 0"
        ).bind(track_id).bind(kind_int).fetch_optional(pool).await;
        if let Ok(Some(_)) = existing {
            return (
                output::error("conflict", output::EXIT_CONFLICT,
                    &format!("Hot cue slot {} is already occupied on '{}'", kind_int, title),
                    Some(&format!("Use 'rbx tracks cues list {}' to see existing cues", track_id))),
                output::EXIT_CONFLICT,
            );
        }
    }

    let plan = serde_json::json!({
        "action": "add_cue",
        "track": { "id": track_id, "title": title, "artist": artist },
        "kind": kind,
        "slot": slot,
        "in_msec": msec,
        "comment": comment,
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.cues.add", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let new_id = match generate_numeric_id(pool, "djmdCue").await {
        Ok(v) => v, Err(e) => return db_error(e),
    };
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let usn = match allocate_usns(pool, 1).await {
        Ok(v) => v, Err(e) => return db_error(e),
    };
    let content_uuid: String = match sqlx::query_as::<_, (String,)>(
        "SELECT COALESCE(UUID, '') FROM djmdContent WHERE ID = ?"
    ).bind(track_id).fetch_one(pool).await {
        Ok((u,)) => u, Err(e) => return db_error(e),
    };
    match sqlx::query(
        "INSERT INTO djmdCue (ID, ContentID, InMsec, InFrame, InMpegFrame, InMpegAbs, \
         OutMsec, OutFrame, OutMpegFrame, OutMpegAbs, Kind, Color, ColorTableIndex, \
         ActiveLoop, Comment, BeatLoopSize, CueMicrosec, \
         ContentUUID, UUID, rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, 0, 0, 0, -1, 0, 0, 0, ?, -1, 0, 0, ?, 0, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)"
    )
    .bind(&new_id).bind(track_id).bind(msec)
    .bind(kind_int).bind(comment.as_deref().unwrap_or(""))
    .bind(msec * 1000) // CueMicrosec = msec * 1000
    .bind(&content_uuid).bind(&new_uuid).bind(usn)
    .bind(&now).bind(&now)
    .execute(pool).await {
        Ok(_) => (
            output::mutation_done("tracks.cues.add", serde_json::json!({
                "cue_id": new_id,
                "track": { "id": track_id, "title": title, "artist": artist },
                "kind": kind,
                "slot": slot,
                "in_msec": msec,
                "comment": comment,
            })),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}

async fn handle_track_cue_update(
    pool: &SqlitePool, cue_id: &str,
    msec: Option<i64>, comment: Option<String>, execute: bool,
) -> (serde_json::Value, i32) {
    let cue = match sqlx::query_as::<_, CueRow>(
        "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
         OutMsec as out_msec, Kind as kind, Color as color, Comment as comment \
         FROM djmdCue WHERE ID = ? AND rb_local_deleted = 0"
    ).bind(cue_id).fetch_optional(pool).await {
        Ok(Some(c)) => c,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Cue not found: {}", cue_id), None),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    if msec.is_none() && comment.is_none() {
        return (
            output::error("usage", output::EXIT_USAGE,
                "No fields specified to update",
                Some("Use --msec or --comment")),
            output::EXIT_USAGE,
        );
    }

    let mut changes = serde_json::Map::new();
    if let Some(v) = msec { changes.insert("in_msec".into(), serde_json::json!(v)); }
    if let Some(ref v) = comment { changes.insert("comment".into(), serde_json::json!(v)); }

    let plan = serde_json::json!({
        "action": "update_cue",
        "cue": cue.to_json(),
        "changes": serde_json::Value::Object(changes.clone()),
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.cues.update", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();
    if let Some(v) = msec {
        let _ = sqlx::query("UPDATE djmdCue SET InMsec = ?, CueMicrosec = ?, updated_at = ? WHERE ID = ?")
            .bind(v).bind(v * 1000).bind(&now).bind(cue_id).execute(pool).await;
    }
    if let Some(ref v) = comment {
        let _ = sqlx::query("UPDATE djmdCue SET Comment = ?, updated_at = ? WHERE ID = ?")
            .bind(v).bind(&now).bind(cue_id).execute(pool).await;
    }

    (
        output::mutation_done("tracks.cues.update", serde_json::json!({
            "cue": cue.to_json(),
            "changes": serde_json::Value::Object(changes),
        })),
        output::EXIT_OK,
    )
}

async fn handle_track_cue_delete(
    pool: &SqlitePool, cue_id: &str, execute: bool,
) -> (serde_json::Value, i32) {
    let cue = match sqlx::query_as::<_, CueRow>(
        "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
         OutMsec as out_msec, Kind as kind, Color as color, Comment as comment \
         FROM djmdCue WHERE ID = ? AND rb_local_deleted = 0"
    ).bind(cue_id).fetch_optional(pool).await {
        Ok(Some(c)) => c,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Cue not found: {}", cue_id), None),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    let plan = serde_json::json!({
        "action": "delete_cue",
        "cue": cue.to_json(),
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.cues.delete", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();
    match sqlx::query("UPDATE djmdCue SET rb_local_deleted = 1, updated_at = ? WHERE ID = ?")
        .bind(&now).bind(cue_id).execute(pool).await {
        Ok(_) => (
            output::mutation_done("tracks.cues.delete", serde_json::json!({
                "cue": cue.to_json(),
            })),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}

// --- Handlers: tracks update ---

async fn resolve_or_create_artist(pool: &SqlitePool, name: &str) -> Result<String, sqlx::Error> {
    if let Some((id,)) = sqlx::query_as::<_, (String,)>(
        "SELECT ID FROM djmdArtist WHERE Name = ?"
    ).bind(name).fetch_optional(pool).await? {
        return Ok(id);
    }
    let new_id = generate_numeric_id(pool, "djmdArtist").await?;
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let usn = allocate_usns(pool, 1).await?;
    sqlx::query(
        "INSERT INTO djmdArtist (ID, Name, SearchStr, UUID, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)"
    ).bind(&new_id).bind(name).bind(name).bind(&new_uuid).bind(usn).bind(&now).bind(&now)
    .execute(pool).await?;
    Ok(new_id)
}

/// Resolves a djmdGenre row by name, creating it in native format when missing.
async fn resolve_or_create_genre(pool: &SqlitePool, name: &str) -> Result<String, sqlx::Error> {
    if let Some((id,)) = sqlx::query_as::<_, (String,)>(
        "SELECT ID FROM djmdGenre WHERE Name = ?"
    ).bind(name).fetch_optional(pool).await? {
        return Ok(id);
    }
    let new_id = generate_numeric_id(pool, "djmdGenre").await?;
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let usn = allocate_usns(pool, 1).await?;
    sqlx::query(
        "INSERT INTO djmdGenre (ID, Name, UUID, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, 0, 0, 0, 0, ?, ?, ?)"
    ).bind(&new_id).bind(name).bind(&new_uuid).bind(usn).bind(&now).bind(&now)
    .execute(pool).await?;
    Ok(new_id)
}

/// Resolves the FK value for a name-keyed table: "" clears the column
/// (rekordbox stores "no artist" etc. as an empty ID), anything else is resolved or created.
async fn resolve_fk<F, Fut>(name: &str, resolve: F) -> Result<String, sqlx::Error>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, sqlx::Error>>,
{
    if name.is_empty() { Ok(String::new()) } else { resolve(name.to_string()).await }
}

async fn resolve_key_id(pool: &SqlitePool, key_name: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>(
        "SELECT ID FROM djmdKey WHERE ScaleName = ?"
    ).bind(key_name).fetch_optional(pool).await.map(|r| r.map(|(id,)| id))
}

/// Fields accepted by `tracks update`. `None` = leave the column alone.
#[derive(Default)]
struct TrackFields {
    title: Option<String>,
    artist: Option<String>,
    genre: Option<String>,
    bpm: Option<f64>,
    key: Option<String>,
    rating: Option<i32>,
    comment: Option<String>,
}

impl TrackFields {
    fn is_empty(&self) -> bool {
        self.title.is_none() && self.artist.is_none() && self.genre.is_none()
            && self.bpm.is_none() && self.key.is_none() && self.rating.is_none()
            && self.comment.is_none()
    }
}

async fn handle_tracks_update(
    pool: &SqlitePool, track_id: &str, fields: TrackFields, execute: bool,
) -> (serde_json::Value, i32) {
    let TrackFields { title, artist, genre, bpm, key, rating, comment } = &fields;
    // Verify track exists
    let (cur_title, cur_artist) = match resolve_track_summary(pool, track_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Track not found: {}", track_id),
                Some("Use 'rbx tracks list' to see available tracks")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    if fields.is_empty() {
        return (
            output::error("usage", output::EXIT_USAGE,
                "No fields specified to update",
                Some("Use --title, --artist, --genre, --bpm, --key, --rating, or --comment")),
            output::EXIT_USAGE,
        );
    }

    // Validate key name if provided
    if let Some(ref k) = key {
        match resolve_key_id(pool, k).await {
            Ok(Some(_)) => {}
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("Unknown key: {}", k),
                    Some("Use 'rbx query \"SELECT ScaleName FROM djmdKey\"' to see valid keys")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        }
    }

    if let Some(r) = *rating {
        if !(0..=5).contains(&r) {
            return (
                output::error("validation", output::EXIT_CONFLICT,
                    &format!("Rating must be 0-5, got: {}", r), None),
                output::EXIT_CONFLICT,
            );
        }
    }

    let mut changes = serde_json::Map::new();
    if let Some(v) = title { changes.insert("title".into(), serde_json::json!(v)); }
    if let Some(v) = artist { changes.insert("artist".into(), serde_json::json!(v)); }
    if let Some(v) = genre { changes.insert("genre".into(), serde_json::json!(v)); }
    if let Some(v) = bpm { changes.insert("bpm".into(), serde_json::json!(v)); }
    if let Some(v) = key { changes.insert("key".into(), serde_json::json!(v)); }
    if let Some(v) = rating { changes.insert("rating".into(), serde_json::json!(v)); }
    if let Some(v) = comment { changes.insert("comment".into(), serde_json::json!(v)); }

    let plan = serde_json::json!({
        "action": "update_track",
        "track": { "id": track_id, "title": cur_title, "artist": cur_artist },
        "changes": serde_json::Value::Object(changes.clone()),
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.update", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    // Resolve FK values before building the query
    let artist_id = if let Some(v) = artist {
        match resolve_fk(v, |n| async move { resolve_or_create_artist(pool, &n).await }).await {
            Ok(id) => Some(id),
            Err(e) => return db_error(e),
        }
    } else { None };

    let genre_id = if let Some(v) = genre {
        match resolve_fk(v, |n| async move { resolve_or_create_genre(pool, &n).await }).await {
            Ok(id) => Some(id),
            Err(e) => return db_error(e),
        }
    } else { None };

    let key_id = if let Some(v) = key {
        match resolve_key_id(pool, v).await {
            Ok(Some(id)) => Some(id),
            _ => unreachable!(),
        }
    } else { None };

    // Execute individual UPDATEs per field
    let now = now_datetime();
    let mut errors = Vec::new();

    macro_rules! update_field {
        ($col:expr, $val:expr) => {
            let sql = format!("UPDATE djmdContent SET {} = ?, updated_at = ? WHERE ID = ?", $col);
            if let Err(e) = sqlx::query(&sql).bind($val).bind(&now).bind(track_id).execute(pool).await {
                errors.push(e.to_string());
            }
        };
    }

    if let Some(v) = title { update_field!("Title", v); }
    if let Some(ref v) = artist_id { update_field!("ArtistID", v); }
    if let Some(ref v) = genre_id { update_field!("GenreID", v); }
    if let Some(v) = bpm { let bpm_int = (v * 100.0) as i32; update_field!("BPM", &bpm_int); }
    if let Some(ref v) = key_id { update_field!("KeyID", v); }
    if let Some(v) = rating { update_field!("Rating", v); }
    if let Some(v) = comment { update_field!("Commnt", v); }

    if !errors.is_empty() {
        return db_error(sqlx::Error::Protocol(errors.join("; ")));
    }

    (
        output::mutation_done("tracks.update", serde_json::json!({
            "track": { "id": track_id, "title": cur_title, "artist": cur_artist },
            "changes": serde_json::Value::Object(changes),
        })),
        output::EXIT_OK,
    )
}

// --- Handlers: tracks mytags ---

async fn handle_track_mytags(pool: &SqlitePool, action: TrackMytagsAction) -> (serde_json::Value, i32) {
    match action {
        TrackMytagsAction::List { track_id } => {
            if resolve_track_summary(pool, &track_id).await.ok().flatten().is_none() {
                return (
                    output::error("not_found", output::EXIT_NOT_FOUND,
                        &format!("Track not found: {}", track_id),
                        Some("Use 'rbx tracks list' to see available tracks")),
                    output::EXIT_NOT_FOUND,
                );
            }
            match sqlx::query_as::<_, TrackMyTagRow>(
                "SELECT DISTINCT t.ID as tag_id, t.Name as tag_name, p.Name as category_name \
                 FROM djmdSongMyTag smt \
                 JOIN djmdMyTag t ON smt.MyTagID = t.ID \
                 LEFT JOIN djmdMyTag p ON t.ParentID = p.ID \
                 WHERE smt.ContentID = ? AND smt.rb_local_deleted = 0 AND t.rb_local_deleted = 0"
            ).bind(&track_id).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("track_mytags", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        TrackMytagsAction::Add { track_id, tag_ids, execute } => {
            handle_track_mytag_add(pool, &track_id, &tag_ids, execute).await
        }
        TrackMytagsAction::Remove { track_id, tag_ids, execute } => {
            handle_track_mytag_remove(pool, &track_id, &tag_ids, execute).await
        }
    }
}

async fn handle_track_mytag_add(
    pool: &SqlitePool, track_id: &str, tag_ids: &[String], execute: bool,
) -> (serde_json::Value, i32) {
    let (title, artist) = match resolve_track_summary(pool, track_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Track not found: {}", track_id),
                Some("Use 'rbx tracks list' to see available tracks")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    let mut tags = Vec::new();
    let mut skipped = Vec::new();
    for tid in tag_ids {
        let tag_name = match resolve_tag_name(pool, tid).await {
            Ok(Some(n)) => n,
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("My Tag not found: {}", tid),
                    Some("Use 'rbx mytags list' to see available tags")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        };
        let exists = sqlx::query_as::<_, (i32,)>(
            "SELECT 1 FROM djmdSongMyTag WHERE MyTagID = ? AND ContentID = ? AND rb_local_deleted = 0"
        ).bind(tid).bind(track_id).fetch_optional(pool).await.ok().flatten().is_some();
        if exists {
            skipped.push(serde_json::json!({ "id": tid, "name": tag_name }));
        } else {
            tags.push((tid.clone(), tag_name));
        }
    }

    let plan = serde_json::json!({
        "action": "add_tags",
        "track": { "id": track_id, "title": title, "artist": artist },
        "tags_to_add": tags.iter().map(|(id, name)| serde_json::json!({"id": id, "name": name})).collect::<Vec<_>>(),
        "skipped_already_assigned": skipped,
    });

    if tags.is_empty() {
        return (
            output::error("conflict", output::EXIT_CONFLICT,
                "All specified tags are already assigned",
                None),
            output::EXIT_CONFLICT,
        );
    }

    if !execute {
        return (
            output::mutation_dry_run("tracks.mytags.add", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();
    let mut added = Vec::new();
    sqlx::query("BEGIN").execute(pool).await.ok();
    let mut base_usn = match allocate_usns(pool, tags.len() as i64).await {
        Ok(v) => v,
        Err(e) => {
            sqlx::query("ROLLBACK").execute(pool).await.ok();
            return db_error(e);
        }
    };
    for (tid, tag_name) in &tags {
        let new_id = Uuid::new_v4().to_string();
        let new_uuid = Uuid::new_v4().to_string();
        if let Err(e) = sqlx::query(
            "INSERT INTO djmdSongMyTag (ID, MyTagID, ContentID, TrackNo, UUID, \
             rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
             created_at, updated_at) \
             VALUES (?, ?, ?, 0, ?, 0, 0, 0, 0, ?, ?, ?)"
        ).bind(&new_id).bind(tid).bind(track_id).bind(&new_uuid).bind(base_usn).bind(&now).bind(&now)
        .execute(pool).await {
            sqlx::query("ROLLBACK").execute(pool).await.ok();
            return db_error(e);
        }
        added.push(serde_json::json!({ "tag_id": tid, "tag_name": tag_name }));
        base_usn += 1;
    }
    sqlx::query("COMMIT").execute(pool).await.ok();

    (
        output::mutation_done("tracks.mytags.add", serde_json::json!({
            "track": { "id": track_id, "title": title, "artist": artist },
            "added": added,
            "skipped": skipped,
        })),
        output::EXIT_OK,
    )
}

async fn handle_track_mytag_remove(
    pool: &SqlitePool, track_id: &str, tag_ids: &[String], execute: bool,
) -> (serde_json::Value, i32) {
    let (title, _) = match resolve_track_summary(pool, track_id).await {
        Ok(Some(t)) => t,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Track not found: {}", track_id),
                Some("Use 'rbx tracks list' to see available tracks")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    let mut targets = Vec::new();
    for tid in tag_ids {
        let tag_name = match resolve_tag_name(pool, tid).await {
            Ok(Some(n)) => n,
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("My Tag not found: {}", tid),
                    Some("Use 'rbx mytags list' to see available tags")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        };
        let row_id = match sqlx::query_as::<_, (String,)>(
            "SELECT ID FROM djmdSongMyTag WHERE MyTagID = ? AND ContentID = ? AND rb_local_deleted = 0"
        ).bind(tid).bind(track_id).fetch_optional(pool).await {
            Ok(Some((id,))) => id,
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("Tag '{}' is not assigned to '{}'", tag_name, title),
                    None),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        };
        targets.push((tid.clone(), tag_name, row_id));
    }

    let plan = serde_json::json!({
        "action": "remove_tags",
        "track": { "id": track_id, "title": title },
        "tags": targets.iter().map(|(id, name, _)| serde_json::json!({"id": id, "name": name})).collect::<Vec<_>>(),
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.mytags.remove", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();
    sqlx::query("BEGIN").execute(pool).await.ok();
    for (_, _, row_id) in &targets {
        let _ = sqlx::query(
            "UPDATE djmdSongMyTag SET rb_local_deleted = 1, updated_at = ? WHERE ID = ?"
        ).bind(&now).bind(row_id).execute(pool).await;
    }
    sqlx::query("COMMIT").execute(pool).await.ok();

    (
        output::mutation_done("tracks.mytags.remove", serde_json::json!({
            "track": { "id": track_id, "title": title },
            "removed_count": targets.len(),
        })),
        output::EXIT_OK,
    )
}

// --- Handlers: playlists ---

async fn resolve_playlist_name(pool: &SqlitePool, id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>(
        "SELECT Name FROM djmdPlaylist WHERE ID = ?"
    ).bind(id).fetch_optional(pool).await.map(|r| r.map(|(n,)| n))
}

async fn handle_playlists(pool: &SqlitePool, db_path: &Path, action: PlaylistsAction) -> (serde_json::Value, i32) {
    match action {
        PlaylistsAction::List => {
            match sqlx::query_as::<_, PlaylistRow>(
                "SELECT ID as id, Name as name, Attribute as attribute, ParentID as parent_id FROM djmdPlaylist"
            ).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("playlists", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        PlaylistsAction::Tracks { action } => handle_playlist_tracks(pool, action).await,
        PlaylistsAction::Search { track_id } => handle_playlists_search(pool, &track_id).await,
        PlaylistsAction::Create { name, parent, execute } => {
            handle_playlists_create(pool, db_path, &name, parent.as_deref(), execute).await
        }
        PlaylistsAction::Delete { id, execute } => {
            handle_playlists_delete(pool, db_path, &id, execute).await
        }
    }
}

// --- Handlers: playlists tracks ---

async fn handle_playlist_tracks(pool: &SqlitePool, action: PlaylistTracksAction) -> (serde_json::Value, i32) {
    match action {
        PlaylistTracksAction::List { playlist_id } => {
            match sqlx::query_as::<_, PlaylistTrackRow>(
                "SELECT sp.TrackNo as track_no, sp.ContentID as content_id, \
                 c.Title as title, a.Name as artist_name, c.BPM as bpm, \
                 k.ScaleName as key_name \
                 FROM djmdSongPlaylist sp \
                 JOIN djmdContent c ON sp.ContentID = c.ID \
                 LEFT JOIN djmdArtist a ON c.ArtistID = a.ID \
                 LEFT JOIN djmdKey k ON c.KeyID = k.ID \
                 WHERE sp.PlaylistID = ? \
                 ORDER BY sp.TrackNo"
            ).bind(&playlist_id).fetch_all(pool).await {
                Ok(rows) if rows.is_empty() => (
                    output::error("not_found", output::EXIT_NOT_FOUND,
                        &format!("Playlist not found or empty: {}", playlist_id),
                        Some("Use 'rbx playlists list' to see available playlists")),
                    output::EXIT_NOT_FOUND,
                ),
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("playlist_tracks", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        PlaylistTracksAction::Add { playlist_id, track_ids, execute } => {
            handle_playlist_track_add(pool, &playlist_id, &track_ids, execute).await
        }
        PlaylistTracksAction::Remove { playlist_id, track_ids, execute } => {
            handle_playlist_track_remove(pool, &playlist_id, &track_ids, execute).await
        }
    }
}

async fn handle_playlist_track_add(
    pool: &SqlitePool, playlist_id: &str, track_ids: &[String], execute: bool,
) -> (serde_json::Value, i32) {
    let pl_name = match resolve_playlist_name(pool, playlist_id).await {
        Ok(Some(n)) => n,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Playlist not found: {}", playlist_id),
                Some("Use 'rbx playlists list' to see available playlists")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    let mut entries = Vec::new();
    for tid in track_ids {
        match resolve_track_summary(pool, tid).await {
            Ok(Some((title, artist))) => entries.push(serde_json::json!({
                "id": tid, "title": title, "artist": artist,
            })),
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("Track not found: {}", tid),
                    Some("Use 'rbx tracks list' to see available tracks")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        }
    }

    let max_track_no = match sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(TrackNo) FROM djmdSongPlaylist WHERE PlaylistID = ?"
    ).bind(playlist_id).fetch_one(pool).await {
        Ok((n,)) => n.unwrap_or(0),
        Err(e) => return db_error(e),
    };

    let plan = serde_json::json!({
        "action": "add_tracks_to_playlist",
        "playlist": { "id": playlist_id, "name": pl_name },
        "tracks": entries,
        "starting_track_no": max_track_no + 1,
    });

    if !execute {
        return (
            output::mutation_dry_run("playlists.tracks.add", plan,
                "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();
    let mut results = Vec::new();
    sqlx::query("BEGIN").execute(pool).await.ok();
    let mut base_usn = match allocate_usns(pool, track_ids.len() as i64).await {
        Ok(v) => v,
        Err(e) => {
            sqlx::query("ROLLBACK").execute(pool).await.ok();
            return db_error(e);
        }
    };
    for (i, tid) in track_ids.iter().enumerate() {
        let new_id = Uuid::new_v4().to_string();
        let new_uuid = Uuid::new_v4().to_string();
        let track_no = max_track_no + 1 + i as i32;
        if let Err(e) = sqlx::query(
            "INSERT INTO djmdSongPlaylist (ID, PlaylistID, ContentID, TrackNo, UUID, \
             rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
             created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)"
        ).bind(&new_id).bind(playlist_id).bind(tid).bind(track_no)
        .bind(&new_uuid).bind(base_usn)
        .bind(&now).bind(&now).execute(pool).await {
            sqlx::query("ROLLBACK").execute(pool).await.ok();
            return db_error(e);
        }
        results.push(serde_json::json!({
            "track_id": tid, "track_no": track_no, "row_id": new_id,
        }));
        base_usn += 1;
    }
    sqlx::query("COMMIT").execute(pool).await.ok();

    (
        output::mutation_done("playlists.tracks.add", serde_json::json!({
            "playlist": { "id": playlist_id, "name": pl_name },
            "added": results,
        })),
        output::EXIT_OK,
    )
}

async fn handle_playlist_track_remove(
    pool: &SqlitePool, playlist_id: &str, track_ids: &[String], execute: bool,
) -> (serde_json::Value, i32) {
    let pl_name = match resolve_playlist_name(pool, playlist_id).await {
        Ok(Some(n)) => n,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Playlist not found: {}", playlist_id),
                Some("Use 'rbx playlists list' to see available playlists")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    let mut targets = Vec::new();
    for tid in track_ids {
        let (title, _) = match resolve_track_summary(pool, tid).await {
            Ok(Some(t)) => t,
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("Track not found: {}", tid),
                    Some("Use 'rbx tracks list' to see available tracks")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        };
        let existing = match sqlx::query_as::<_, (String, i32)>(
            "SELECT ID, TrackNo FROM djmdSongPlaylist WHERE PlaylistID = ? AND ContentID = ?"
        ).bind(playlist_id).bind(tid).fetch_optional(pool).await {
            Ok(Some(r)) => r,
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("Track '{}' is not in playlist '{}'", title, pl_name),
                    Some("Use 'rbx playlists tracks list <playlist_id>' to see tracks")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        };
        targets.push((tid.clone(), existing.0, existing.1));
    }

    let plan = serde_json::json!({
        "action": "remove_tracks_from_playlist",
        "playlist": { "id": playlist_id, "name": pl_name },
        "track_ids": track_ids,
        "count": targets.len(),
    });

    if !execute {
        return (
            output::mutation_dry_run("playlists.tracks.remove", plan,
                "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    sqlx::query("BEGIN").execute(pool).await.ok();
    for (_, row_id, _) in &targets {
        let _ = sqlx::query("DELETE FROM djmdSongPlaylist WHERE ID = ?")
            .bind(row_id).execute(pool).await;
    }

    // Renumber all remaining tracks sequentially
    let remaining = sqlx::query_as::<_, (String,)>(
        "SELECT ID FROM djmdSongPlaylist WHERE PlaylistID = ? ORDER BY TrackNo"
    ).bind(playlist_id).fetch_all(pool).await.unwrap_or_default();
    for (i, (row_id,)) in remaining.iter().enumerate() {
        let _ = sqlx::query("UPDATE djmdSongPlaylist SET TrackNo = ? WHERE ID = ?")
            .bind((i + 1) as i32).bind(row_id).execute(pool).await;
    }
    sqlx::query("COMMIT").execute(pool).await.ok();

    (
        output::mutation_done("playlists.tracks.remove", serde_json::json!({
            "playlist": { "id": playlist_id, "name": pl_name },
            "removed_count": targets.len(),
        })),
        output::EXIT_OK,
    )
}

// --- Handlers: playlists search ---

async fn handle_playlists_search(pool: &SqlitePool, track_id: &str) -> (serde_json::Value, i32) {
    if resolve_track_summary(pool, track_id).await.ok().flatten().is_none() {
        return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Track not found: {}", track_id),
                Some("Use 'rbx tracks list' to see available tracks")),
            output::EXIT_NOT_FOUND,
        );
    }

    match sqlx::query_as::<_, (String, String, i32)>(
        "SELECT p.ID, COALESCE(p.Name, ''), sp.TrackNo \
         FROM djmdSongPlaylist sp \
         JOIN djmdPlaylist p ON sp.PlaylistID = p.ID \
         WHERE sp.ContentID = ? \
         ORDER BY p.Name"
    ).bind(track_id).fetch_all(pool).await {
        Ok(rows) => {
            let items: Vec<_> = rows.iter().map(|(id, name, track_no)| {
                serde_json::json!({
                    "playlist_id": id,
                    "playlist_name": name,
                    "track_no": track_no,
                })
            }).collect();
            (output::success("track_playlists", serde_json::Value::Array(items)), output::EXIT_OK)
        }
        Err(e) => db_error(e),
    }
}

// --- Handlers: playlists create/delete ---

async fn handle_playlists_create(
    pool: &SqlitePool, db_path: &Path, name: &str, parent_id: Option<&str>, execute: bool,
) -> (serde_json::Value, i32) {
    if let Some(pid) = parent_id {
        match resolve_playlist_name(pool, pid).await {
            Ok(Some(_)) => {}
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("Parent folder not found: {}", pid),
                    Some("Use 'rbx playlists list' to see available folders")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        }
    }
    // rekordbox uses the literal string "root" for top-level playlists
    let parent = parent_id.unwrap_or("root");

    let plan = serde_json::json!({
        "action": "create_playlist",
        "name": name,
        "parent_id": parent,
    });

    if !execute {
        return (
            output::mutation_dry_run("playlists.create", plan,
                "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let new_id = match generate_numeric_id(pool, "djmdPlaylist").await {
        Ok(v) => v, Err(e) => return db_error(e),
    };
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let usn = match allocate_usns(pool, 1).await {
        Ok(v) => v, Err(e) => return db_error(e),
    };
    // Seq: max within same parent + 1
    let seq = match sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(Seq) FROM djmdPlaylist WHERE ParentID = ? AND rb_local_deleted = 0"
    ).bind(parent).fetch_one(pool).await {
        Ok((s,)) => s.unwrap_or(0) + 1,
        Err(e) => return db_error(e),
    };
    // Attribute 0 = playlist (not folder)
    match sqlx::query(
        "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, UUID, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, 0, ?, ?, 0, 0, 0, 0, ?, ?, ?)"
    ).bind(&new_id).bind(seq).bind(name).bind(parent).bind(&new_uuid).bind(usn)
    .bind(&now).bind(&now)
    .execute(pool).await {
        Ok(_) => {
            let xml_path = playlist_xml::xml_path_for(db_path);
            let xml_updated = playlist_xml::add_node(&xml_path, &new_id, parent, 0, now_ms)
                .unwrap_or(false);
            (
                output::mutation_done("playlists.create", serde_json::json!({
                    "id": new_id,
                    "name": name,
                    "kind": "playlist",
                    "parent_id": parent,
                    "seq": seq,
                    "playlist_xml_updated": xml_updated,
                })),
                output::EXIT_OK,
            )
        }
        Err(e) => db_error(e),
    }
}

async fn handle_playlists_delete(
    pool: &SqlitePool, db_path: &Path, playlist_id: &str, execute: bool,
) -> (serde_json::Value, i32) {
    let pl = match sqlx::query_as::<_, PlaylistRow>(
        "SELECT ID as id, Name as name, Attribute as attribute, ParentID as parent_id \
         FROM djmdPlaylist WHERE ID = ?"
    ).bind(playlist_id).fetch_optional(pool).await {
        Ok(Some(p)) => p,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("Playlist not found: {}", playlist_id),
                Some("Use 'rbx playlists list' to see available playlists")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    // Count tracks that would be removed
    let track_count = sqlx::query_as::<_, (i32,)>(
        "SELECT COUNT(*) FROM djmdSongPlaylist WHERE PlaylistID = ?"
    ).bind(playlist_id).fetch_one(pool).await.map(|(c,)| c).unwrap_or(0);

    let plan = serde_json::json!({
        "action": "delete_playlist",
        "playlist": pl.to_json(),
        "track_count": track_count,
    });

    if !execute {
        return (
            output::mutation_dry_run("playlists.delete", plan,
                "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    // Delete track entries first
    let _ = sqlx::query("DELETE FROM djmdSongPlaylist WHERE PlaylistID = ?")
        .bind(playlist_id).execute(pool).await;

    match sqlx::query("DELETE FROM djmdPlaylist WHERE ID = ?")
        .bind(playlist_id).execute(pool).await {
        Ok(_) => {
            let xml_path = playlist_xml::xml_path_for(db_path);
            let xml_updated = playlist_xml::remove_node(&xml_path, playlist_id)
                .unwrap_or(false);
            (
                output::mutation_done("playlists.delete", serde_json::json!({
                    "playlist": pl.to_json(),
                    "tracks_removed": track_count,
                    "playlist_xml_updated": xml_updated,
                })),
                output::EXIT_OK,
            )
        }
        Err(e) => db_error(e),
    }
}

// --- Handlers: mytags (tag CRUD) ---

async fn handle_mytags(pool: &SqlitePool, action: MytagsAction) -> (serde_json::Value, i32) {
    match action {
        MytagsAction::List => {
            match sqlx::query_as::<_, MyTagRow>(
                "SELECT ID as id, Seq as seq, Name as name, Attribute as attribute, \
                 ParentID as parent_id \
                 FROM djmdMyTag WHERE rb_local_deleted = 0 ORDER BY Seq"
            ).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("mytags", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        MytagsAction::Tracks { id } => {
            match sqlx::query_as::<_, MyTagTrackRow>(
                "SELECT smt.ContentID as content_id, c.Title as title, \
                 a.Name as artist_name, c.BPM as bpm, k.ScaleName as key_name \
                 FROM djmdSongMyTag smt \
                 JOIN djmdContent c ON smt.ContentID = c.ID \
                 LEFT JOIN djmdArtist a ON c.ArtistID = a.ID \
                 LEFT JOIN djmdKey k ON c.KeyID = k.ID \
                 WHERE smt.MyTagID = ? AND smt.rb_local_deleted = 0"
            ).bind(&id).fetch_all(pool).await {
                Ok(rows) if rows.is_empty() => (
                    output::error("not_found", output::EXIT_NOT_FOUND,
                        &format!("My Tag not found or has no tracks: {}", id),
                        Some("Use 'rbx mytags list' to see available tags")),
                    output::EXIT_NOT_FOUND,
                ),
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (output::success("mytag_tracks", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        MytagsAction::Create { name, parent, execute } => {
            handle_mytags_create(pool, &name, parent.as_deref(), execute).await
        }
        MytagsAction::Delete { id, execute } => {
            handle_mytags_delete(pool, &id, execute).await
        }
    }
}

async fn handle_mytags_create(
    pool: &SqlitePool, name: &str, parent_id: Option<&str>, execute: bool,
) -> (serde_json::Value, i32) {
    // attribute: 0 = tag (has parent), 1 = category (top-level)
    let attribute = if parent_id.is_some() { 0 } else { 1 };

    if let Some(pid) = parent_id {
        match resolve_tag_name(pool, pid).await {
            Ok(Some(_)) => {}
            Ok(None) => return (
                output::error("not_found", output::EXIT_NOT_FOUND,
                    &format!("Parent category not found: {}", pid),
                    Some("Use 'rbx mytags list' to see available categories")),
                output::EXIT_NOT_FOUND,
            ),
            Err(e) => return db_error(e),
        }
    }

    // Check for duplicate name under same parent
    let dup = match sqlx::query_as::<_, (String,)>(
        "SELECT ID FROM djmdMyTag WHERE Name = ? AND ParentID IS ? AND rb_local_deleted = 0"
    ).bind(name).bind(parent_id).fetch_optional(pool).await {
        Ok(r) => r,
        Err(e) => return db_error(e),
    };

    if let Some((existing_id,)) = dup {
        return (
            output::error("conflict", output::EXIT_CONFLICT,
                &format!("My Tag '{}' already exists (ID: {})", name, existing_id),
                None),
            output::EXIT_CONFLICT,
        );
    }

    let kind = if attribute == 1 { "category" } else { "tag" };
    let plan = serde_json::json!({
        "action": "create_mytag",
        "name": name,
        "kind": kind,
        "parent_id": parent_id,
    });

    if !execute {
        return (
            output::mutation_dry_run("mytags.create", plan,
                "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let new_id = match generate_numeric_id(pool, "djmdMyTag").await {
        Ok(v) => v, Err(e) => return db_error(e),
    };
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let usn = match allocate_usns(pool, 1).await {
        Ok(v) => v, Err(e) => return db_error(e),
    };

    // Seq: max existing + 1
    let max_seq = sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(Seq) FROM djmdMyTag WHERE rb_local_deleted = 0"
    ).fetch_one(pool).await.map(|(s,)| s.unwrap_or(0)).unwrap_or(0);

    match sqlx::query(
        "INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID, UUID, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)"
    ).bind(&new_id).bind(max_seq + 1).bind(name).bind(attribute).bind(parent_id)
    .bind(&new_uuid).bind(usn)
    .bind(&now).bind(&now).execute(pool).await {
        Ok(_) => (
            output::mutation_done("mytags.create", serde_json::json!({
                "id": new_id,
                "name": name,
                "kind": kind,
                "parent_id": parent_id,
                "seq": max_seq + 1,
            })),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}

async fn handle_mytags_delete(
    pool: &SqlitePool, tag_id: &str, execute: bool,
) -> (serde_json::Value, i32) {
    let tag = match sqlx::query_as::<_, MyTagRow>(
        "SELECT ID as id, Seq as seq, Name as name, Attribute as attribute, ParentID as parent_id \
         FROM djmdMyTag WHERE ID = ? AND rb_local_deleted = 0"
    ).bind(tag_id).fetch_optional(pool).await {
        Ok(Some(t)) => t,
        Ok(None) => return (
            output::error("not_found", output::EXIT_NOT_FOUND,
                &format!("My Tag not found: {}", tag_id),
                Some("Use 'rbx mytags list' to see available tags")),
            output::EXIT_NOT_FOUND,
        ),
        Err(e) => return db_error(e),
    };

    let plan = serde_json::json!({
        "action": "delete_mytag",
        "tag": tag.to_json(),
    });

    if !execute {
        return (
            output::mutation_dry_run("mytags.delete", plan,
                "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();

    // Also soft-delete all associations
    let _ = sqlx::query(
        "UPDATE djmdSongMyTag SET rb_local_deleted = 1, updated_at = ? WHERE MyTagID = ? AND rb_local_deleted = 0"
    ).bind(&now).bind(tag_id).execute(pool).await;

    match sqlx::query(
        "UPDATE djmdMyTag SET rb_local_deleted = 1, updated_at = ? WHERE ID = ?"
    ).bind(&now).bind(tag_id).execute(pool).await {
        Ok(_) => (
            output::mutation_done("mytags.delete", serde_json::json!({
                "tag": tag.to_json(),
            })),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}

// --- Handlers: history ---

async fn handle_history(pool: &SqlitePool, action: HistoryAction) -> (serde_json::Value, i32) {
    match action {
        HistoryAction::List { limit } => {
            match sqlx::query_as::<_, (String, String, String, i32)>(
                "SELECT h.ID, h.Name, h.DateCreated, \
                 (SELECT COUNT(*) FROM djmdSongHistory sh WHERE sh.HistoryID = h.ID) as track_count \
                 FROM djmdHistory h ORDER BY h.DateCreated DESC LIMIT ?"
            ).bind(limit).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|(id, name, date, count)| {
                        serde_json::json!({
                            "id": id,
                            "name": name,
                            "date": date,
                            "track_count": count,
                        })
                    }).collect();
                    (output::success("history", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
        HistoryAction::Tracks { id } => {
            match sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<i32>, Option<String>)>(
                "SELECT sh.ContentID, c.Title, a.Name, c.BPM, k.ScaleName \
                 FROM djmdSongHistory sh \
                 JOIN djmdContent c ON sh.ContentID = c.ID \
                 LEFT JOIN djmdArtist a ON c.ArtistID = a.ID \
                 LEFT JOIN djmdKey k ON c.KeyID = k.ID \
                 WHERE sh.HistoryID = ? \
                 ORDER BY sh.TrackNo"
            ).bind(&id).fetch_all(pool).await {
                Ok(rows) if rows.is_empty() => (
                    output::error("not_found", output::EXIT_NOT_FOUND,
                        &format!("History session not found or empty: {}", id),
                        Some("Use 'rbx history list' to see available sessions")),
                    output::EXIT_NOT_FOUND,
                ),
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|(id, title, artist, bpm, key)| {
                        serde_json::json!({
                            "id": id,
                            "title": title,
                            "artist": artist,
                            "bpm": bpm.map(|b| b as f64 / 100.0),
                            "key": key,
                        })
                    }).collect();
                    (output::success("history_tracks", serde_json::Value::Array(items)), output::EXIT_OK)
                }
                Err(e) => db_error(e),
            }
        }
    }
}

// --- Handlers: query ---

/// Without --unsafe-write only investigation statements are allowed:
/// SELECT, WITH (CTEs), PRAGMA (schema inspection), EXPLAIN.
fn is_read_only_statement(sql: &str) -> bool {
    let trimmed = sql.trim().trim_end_matches(';').trim();
    if trimmed.contains(';') {
        // Reject multi-statement input outright; a write could hide behind
        // a leading SELECT.
        return false;
    }
    let first = trimmed
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    matches!(first.as_str(), "SELECT" | "WITH" | "PRAGMA" | "EXPLAIN")
}

async fn handle_query(pool: &SqlitePool, sql: &str, unsafe_write: bool) -> (serde_json::Value, i32) {
    if !unsafe_write && !is_read_only_statement(sql) {
        return (
            output::error(
                "readonly",
                output::EXIT_USAGE,
                "query allows only SELECT / WITH / PRAGMA / EXPLAIN (single statement) by default",
                Some("Prefer a dedicated command (they maintain rekordbox invariants: \
                      USN allocation, timestamp format, numeric IDs, masterPlaylists6.xml sync). \
                      If you really need raw SQL writes, re-run with --unsafe-write"),
            ),
            output::EXIT_USAGE,
        );
    }
    match sqlx::query(sql).fetch_all(pool).await {
        Ok(rows) => {
            let items: Vec<serde_json::Value> = rows.iter().map(|row| {
                let cols = row.columns();
                let mut obj = serde_json::Map::new();
                for col in cols {
                    let name = col.name().to_string();
                    let val = row.try_get::<String, _>(col.ordinal())
                        .map(serde_json::Value::String)
                        .or_else(|_| row.try_get::<i64, _>(col.ordinal()).map(|v| serde_json::json!(v)))
                        .or_else(|_| row.try_get::<f64, _>(col.ordinal()).map(|v| serde_json::json!(v)))
                        .unwrap_or(serde_json::Value::Null);
                    obj.insert(name, val);
                }
                serde_json::Value::Object(obj)
            }).collect();
            (output::success("query_result", serde_json::Value::Array(items)), output::EXIT_OK)
        }
        Err(e) if e.to_string().contains("readonly database") => (
            output::error(
                "readonly",
                output::EXIT_USAGE,
                "query runs read-only by default; this statement needs write access",
                Some("Prefer a dedicated command (they maintain rekordbox invariants). \
                      If you really need raw SQL, re-run with --unsafe-write"),
            ),
            output::EXIT_USAGE,
        ),
        Err(e) => db_error(e),
    }
}

// --- Describe ---

fn handle_describe(resource: Option<String>, action: Option<String>) -> serde_json::Value {
    match (resource.as_deref(), action.as_deref()) {
        (None, _) => describe_root(),

        // tracks
        (Some("tracks"), None) => describe_resource("tracks", &[
            ("list", "List all tracks (excludes streaming-only)"),
            ("get", "Get a single track by ID"),
            ("search", "Search tracks by title or artist name"),
            ("filter", "Filter tracks by BPM range, key, and/or tag"),
            ("update", "Update track fields (dry-run by default)"),
            ("cues list", "List cue points on a track"),
            ("cues add", "Add a cue point (dry-run by default)"),
            ("cues update", "Update a cue point (dry-run by default)"),
            ("cues delete", "Delete a cue point (dry-run by default)"),
            ("mytags list", "List My Tags assigned to a track"),
            ("mytags add", "Add a My Tag to a track (dry-run by default)"),
            ("mytags remove", "Remove a My Tag from a track (dry-run by default)"),
        ]),
        (Some("tracks"), Some("list")) => describe_command("tracks list", &[], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &["rbx tracks list"]),
        (Some("tracks"), Some("get")) => describe_command("tracks get", &[
            flag("id", "string", true, "Track ID (from djmdContent.ID)"),
        ], &track_schema(), &["rbx tracks get 12345"]),
        (Some("tracks"), Some("search")) => describe_command("tracks search", &[
            flag("query", "string", true, "Search term (matched against title and artist)"),
        ], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &["rbx tracks search 'Butterfly'"]),
        (Some("tracks"), Some("filter")) => describe_command("tracks filter", &[
            flag("--bpm-min", "number", false, "Minimum BPM (inclusive)"),
            flag("--bpm-max", "number", false, "Maximum BPM (inclusive)"),
            flag("--key", "string", false, "Musical key (e.g. '8A')"),
            flag("--tag", "string", false, "My Tag ID to filter by"),
        ], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &[
            "rbx tracks filter --bpm-min 125 --bpm-max 135",
            "rbx tracks filter --key 8A --tag TAG_ID",
            "rbx tracks filter --bpm-min 120 --bpm-max 140 --key 8A",
        ]),
        (Some("tracks"), Some("update")) => describe_command("tracks update", &[
            flag("id", "string", true, "Track ID"),
            flag("--title", "string", false, "Track title"),
            flag("--artist", "string", false, "Artist name (resolved or created in djmdArtist; \"\" clears)"),
            flag("--genre", "string", false, "Genre name (resolved or created in djmdGenre; \"\" clears)"),
            flag("--bpm", "number", false, "BPM as decimal (e.g. 128.0)"),
            flag("--key", "string", false, "Musical key (e.g. '8A', '1B')"),
            flag("--rating", "integer", false, "Rating (0-5)"),
            flag("--comment", "string", false, "Comment text"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.update"), &[
            "rbx tracks update TRACK_ID --title 'New Title' --bpm 128.0",
            "rbx tracks update TRACK_ID --artist 'Artist' --key '8A' --execute",
        ]),
        (Some("tracks"), Some("cues list")) => describe_command("tracks cues list", &[
            flag("track_id", "string", true, "Track ID"),
        ], &serde_json::json!({
            "type": "array", "items": cue_schema(),
        }), &["rbx tracks cues list TRACK_ID"]),
        (Some("tracks"), Some("cues add")) => describe_command("tracks cues add", &[
            flag("track_id", "string", true, "Track ID"),
            flag("msec", "integer", true, "Position in milliseconds"),
            flag("--kind", "string", false, "Cue type: 'memory' (default) or 'hot'"),
            flag("--slot", "integer", false, "Hot cue slot (1-8, required for hot cues)"),
            flag("--comment", "string", false, "Cue comment/name"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.cues.add"), &[
            "rbx tracks cues add TRACK_ID 12345",
            "rbx tracks cues add TRACK_ID 12345 --kind hot --slot 1 --comment 'Drop' --execute",
        ]),
        (Some("tracks"), Some("cues update")) => describe_command("tracks cues update", &[
            flag("cue_id", "string", true, "Cue ID"),
            flag("--msec", "integer", false, "New position in milliseconds"),
            flag("--comment", "string", false, "New comment"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.cues.update"), &[
            "rbx tracks cues update CUE_ID --msec 15000 --comment 'Verse'",
            "rbx tracks cues update CUE_ID --comment 'Chorus' --execute",
        ]),
        (Some("tracks"), Some("cues delete")) => describe_command("tracks cues delete", &[
            flag("cue_id", "string", true, "Cue ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.cues.delete"), &[
            "rbx tracks cues delete CUE_ID",
            "rbx tracks cues delete CUE_ID --execute",
        ]),
        (Some("tracks"), Some("mytags list")) => describe_command("tracks mytags list", &[
            flag("track_id", "string", true, "Track ID"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "tag_id": { "type": "string" },
                    "tag_name": { "type": "string|null" },
                    "category_name": { "type": "string|null" },
                },
            },
        }), &["rbx tracks mytags list TRACK_ID"]),
        (Some("tracks"), Some("mytags add")) => describe_command("tracks mytags add", &[
            flag("track_id", "string", true, "Track ID"),
            flag("tag_id", "string", true, "My Tag ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.mytags.add"), &[
            "rbx tracks mytags add TRACK_ID TAG_ID",
            "rbx tracks mytags add TRACK_ID TAG_ID --execute",
        ]),
        (Some("tracks"), Some("mytags remove")) => describe_command("tracks mytags remove", &[
            flag("track_id", "string", true, "Track ID"),
            flag("tag_id", "string", true, "My Tag ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.mytags.remove"), &[
            "rbx tracks mytags remove TRACK_ID TAG_ID",
            "rbx tracks mytags remove TRACK_ID TAG_ID --execute",
        ]),

        // playlists
        (Some("playlists"), None) => describe_resource("playlists", &[
            ("list", "List all playlists and folders"),
            ("tracks list", "List tracks in a specific playlist"),
            ("tracks add", "Add a track to a playlist (dry-run by default)"),
            ("tracks remove", "Remove a track from a playlist (dry-run by default)"),
            ("search", "Find playlists containing a specific track"),
            ("create", "Create a new playlist (dry-run by default)"),
            ("delete", "Delete a playlist (dry-run by default)"),
        ]),
        (Some("playlists"), Some("list")) => describe_command("playlists list", &[], &serde_json::json!({
            "type": "array", "items": playlist_schema(),
        }), &["rbx playlists list"]),
        (Some("playlists"), Some("tracks list")) => describe_command("playlists tracks list", &[
            flag("playlist_id", "string", true, "Playlist ID"),
        ], &serde_json::json!({
            "type": "array", "items": playlist_track_schema(),
        }), &["rbx playlists tracks list PLAYLIST_ID"]),
        (Some("playlists"), Some("tracks add")) => describe_command("playlists tracks add", &[
            flag("playlist_id", "string", true, "Playlist ID"),
            flag("track_id", "string", true, "Track ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.tracks.add"), &[
            "rbx playlists tracks add PLAYLIST_ID TRACK_ID",
            "rbx playlists tracks add PLAYLIST_ID TRACK_ID --execute",
        ]),
        (Some("playlists"), Some("tracks remove")) => describe_command("playlists tracks remove", &[
            flag("playlist_id", "string", true, "Playlist ID"),
            flag("track_id", "string", true, "Track ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.tracks.remove"), &[
            "rbx playlists tracks remove PLAYLIST_ID TRACK_ID",
            "rbx playlists tracks remove PLAYLIST_ID TRACK_ID --execute",
        ]),
        (Some("playlists"), Some("search")) => describe_command("playlists search", &[
            flag("track_id", "string", true, "Track ID to search for"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "playlist_id": { "type": "string" },
                    "playlist_name": { "type": "string" },
                    "track_no": { "type": "integer" },
                },
            },
        }), &["rbx playlists search TRACK_ID"]),
        (Some("playlists"), Some("create")) => describe_command("playlists create", &[
            flag("name", "string", true, "Playlist name"),
            flag("--parent", "string", false, "Parent folder ID (omit for top-level)"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.create"), &[
            "rbx playlists create 'My Playlist'",
            "rbx playlists create 'My Playlist' --parent FOLDER_ID --execute",
        ]),
        (Some("playlists"), Some("delete")) => describe_command("playlists delete", &[
            flag("id", "string", true, "Playlist ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.delete"), &[
            "rbx playlists delete PLAYLIST_ID",
            "rbx playlists delete PLAYLIST_ID --execute",
        ]),

        // mytags
        (Some("mytags"), None) => describe_resource("mytags", &[
            ("list", "List all My Tag categories and tags"),
            ("tracks", "List tracks with a specific My Tag"),
            ("create", "Create a new My Tag or category (dry-run by default)"),
            ("delete", "Delete a My Tag (dry-run by default)"),
        ]),
        (Some("mytags"), Some("list")) => describe_command("mytags list", &[], &serde_json::json!({
            "type": "array", "items": mytag_schema(),
        }), &["rbx mytags list"]),
        (Some("mytags"), Some("tracks")) => describe_command("mytags tracks", &[
            flag("id", "string", true, "My Tag ID"),
        ], &serde_json::json!({
            "type": "array", "items": mytag_track_schema(),
        }), &["rbx mytags tracks 12345"]),
        (Some("mytags"), Some("create")) => describe_command("mytags create", &[
            flag("name", "string", true, "Tag name"),
            flag("--parent", "string", false, "Parent category ID (omit for top-level category)"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("mytags.create"), &[
            "rbx mytags create 'My Category'",
            "rbx mytags create 'My Tag' --parent CATEGORY_ID --execute",
        ]),
        (Some("mytags"), Some("delete")) => describe_command("mytags delete", &[
            flag("id", "string", true, "My Tag ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("mytags.delete"), &[
            "rbx mytags delete TAG_ID",
            "rbx mytags delete TAG_ID --execute",
        ]),

        // history
        (Some("history"), None) => describe_resource("history", &[
            ("list", "List play history sessions (most recent first)"),
            ("tracks", "List tracks in a history session"),
        ]),
        (Some("history"), Some("list")) => describe_command("history list", &[
            flag("--limit", "integer", false, "Max sessions to return (default: 20)"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "date": { "type": "string" },
                    "track_count": { "type": "integer" },
                },
            },
        }), &["rbx history list", "rbx history list --limit 5"]),
        (Some("history"), Some("tracks")) => describe_command("history tracks", &[
            flag("id", "string", true, "History session ID"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "title": { "type": "string|null" },
                    "artist": { "type": "string|null" },
                    "bpm": { "type": "number|null" },
                    "key": { "type": "string|null" },
                },
            },
        }), &["rbx history tracks HISTORY_ID"]),

        // query
        (Some("query"), None) | (Some("query"), Some(_)) => describe_command("query", &[
            flag("sql", "string", true,
                "SQL to execute. Read-only by default: single SELECT / WITH / PRAGMA / EXPLAIN statement"),
            flag("--unsafe-write", "bool", false,
                "Allow arbitrary SQL incl. writes. Bypasses rekordbox invariants \
                 (USN allocation, timestamp format, numeric IDs, masterPlaylists6.xml sync) — \
                 prefer dedicated commands"),
        ], &serde_json::json!({
            "type": "array",
            "items": { "type": "object", "description": "Dynamic columns based on query" },
        }), &[
            "rbx query 'SELECT ID, Title FROM djmdContent LIMIT 5'",
            "rbx query 'PRAGMA table_info(djmdContent)'",
        ]),

        (Some(r), _) => output::error(
            "not_found", output::EXIT_NOT_FOUND,
            &format!("Unknown resource: {}", r),
            Some("Use 'rbx describe' to see available resources"),
        ),
    }
}

fn describe_root() -> serde_json::Value {
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "resources": [
            { "name": "tracks", "description": "Query tracks and manage their My Tag assignments" },
            { "name": "playlists", "description": "Query playlists and their contents" },
            { "name": "mytags", "description": "Manage My Tag categories and tags (CRUD)" },
            { "name": "history", "description": "Query play history sessions" },
            { "name": "query", "description": "Run raw SQL against master.db (read-only: SELECT/WITH/PRAGMA/EXPLAIN)" },
        ],
        "global_flags": [
            { "name": "--db", "type": "path", "required": true, "env": "RBX_DB_PATH",
              "description": "Path to rekordbox master.db" },
        ],
        "discovery_sequence": [
            "rbx describe",
            "rbx describe <resource>",
            "rbx describe <resource> <action>",
            "rbx <resource> <action> [args]",
        ],
    })
}

fn describe_resource(name: &str, actions: &[(&str, &str)]) -> serde_json::Value {
    let acts: Vec<_> = actions.iter().map(|(n, d)| serde_json::json!({
        "name": n, "description": d,
    })).collect();
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "resource": name,
        "actions": acts,
    })
}

fn describe_command(
    command: &str, flags: &[serde_json::Value],
    output_schema: &serde_json::Value, examples: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "command": command,
        "flags": flags,
        "output_schema": output_schema,
        "examples": examples,
    })
}

fn flag(name: &str, typ: &str, required: bool, desc: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "type": typ, "required": required, "description": desc })
}

fn track_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "title": { "type": "string|null" },
            "artist": { "type": "string|null" },
            "duration_sec": { "type": "integer|null" },
            "bpm": { "type": "number|null", "description": "BPM as decimal (e.g. 128.0)" },
            "key": { "type": "string|null", "description": "Musical key (e.g. '8A', '1B')" },
            "folder_path": { "type": "string|null" },
        },
    })
}

fn playlist_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "name": { "type": "string|null" },
            "kind": { "type": "string", "enum": ["folder", "playlist"] },
            "parent_id": { "type": "string|null" },
        },
    })
}

fn playlist_track_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "track_no": { "type": "integer" },
            "id": { "type": "string" },
            "title": { "type": "string|null" },
            "artist": { "type": "string|null" },
            "bpm": { "type": "number|null" },
            "key": { "type": "string|null" },
        },
    })
}

fn mytag_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "seq": { "type": "integer|null" },
            "name": { "type": "string|null" },
            "kind": { "type": "string", "enum": ["category", "tag"] },
            "parent_id": { "type": "string|null" },
        },
    })
}

fn mytag_track_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "title": { "type": "string|null" },
            "artist": { "type": "string|null" },
            "bpm": { "type": "number|null" },
            "key": { "type": "string|null" },
        },
    })
}

fn cue_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "track_id": { "type": "string" },
            "kind": { "type": "string", "enum": ["memory", "hot", "other"] },
            "slot": { "type": "integer|null", "description": "Hot cue slot (1-8), null for memory cues" },
            "in_msec": { "type": "integer|null", "description": "Cue position in milliseconds" },
            "out_msec": { "type": "integer|null", "description": "Loop end in milliseconds, null if not a loop" },
            "color": { "type": "integer|null" },
            "comment": { "type": "string|null" },
        },
    })
}

fn mutation_result_schema(kind: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": format!("When dry_run=true, includes 'plan' and 'next_step'. When dry_run=false, includes 'result'. kind='{}'", kind),
        "properties": {
            "dry_run": { "type": "boolean" },
            "plan": { "type": "object", "description": "Present when dry_run=true" },
            "result": { "type": "object", "description": "Present when dry_run=false" },
            "next_step": { "type": "string", "description": "Present when dry_run=true" },
        },
    })
}
