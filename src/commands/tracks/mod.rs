pub(crate) mod cues;
pub(crate) mod mytags;
pub(crate) mod update;

use rbx::output;
use sqlx::sqlite::SqlitePool;

use crate::cli::TracksAction;
use crate::commands::db_error;
use crate::rows::TrackRow;
use cues::handle_track_cues;
use mytags::handle_track_mytags;
use update::{handle_tracks_bulk_update, handle_tracks_update, TrackFields};

pub(crate) const TRACK_QUERY_BASE: &str = "\
    SELECT c.ID as id, c.Title as title, a.Name as artist_name, \
    c.Length as duration, c.BPM as bpm, k.ScaleName as key_name, \
    c.FolderPath as folder_path \
    FROM djmdContent c \
    LEFT JOIN djmdArtist a ON c.ArtistID = a.ID \
    LEFT JOIN djmdKey k ON c.KeyID = k.ID";

pub(crate) const TRACK_FILTER_LOCAL: &str = "\
    c.FolderPath NOT LIKE 'spotify:%' \
    AND c.FolderPath NOT LIKE 'apple:%' \
    AND c.FolderPath NOT LIKE 'itunes:%' \
    AND c.Title IS NOT NULL AND c.Title != ''";

pub(crate) async fn handle_tracks(
    pool: &SqlitePool,
    action: TracksAction,
) -> (serde_json::Value, i32) {
    match action {
        TracksAction::List => {
            let sql = format!("{} WHERE {}", TRACK_QUERY_BASE, TRACK_FILTER_LOCAL);
            match sqlx::query_as::<_, TrackRow>(&sql).fetch_all(pool).await {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (
                        output::success("tracks", serde_json::Value::Array(items)),
                        output::EXIT_OK,
                    )
                }
                Err(e) => db_error(e),
            }
        }
        TracksAction::Get { id } => {
            let sql = format!("{} WHERE c.ID = ?", TRACK_QUERY_BASE);
            match sqlx::query_as::<_, TrackRow>(&sql)
                .bind(&id)
                .fetch_optional(pool)
                .await
            {
                Ok(Some(row)) => (output::success_one("track", row.to_json()), output::EXIT_OK),
                Ok(None) => (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Track not found: {}", id),
                        Some("Use 'rbx tracks list' to see available tracks"),
                    ),
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
            match sqlx::query_as::<_, TrackRow>(&sql)
                .bind(&pattern)
                .fetch_all(pool)
                .await
            {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (
                        output::success("tracks", serde_json::Value::Array(items)),
                        output::EXIT_OK,
                    )
                }
                Err(e) => db_error(e),
            }
        }
        TracksAction::Filter {
            bpm_min,
            bpm_max,
            key,
            tag,
        } => handle_tracks_filter(pool, bpm_min, bpm_max, key, tag).await,
        TracksAction::Update {
            id,
            title,
            artist,
            genre,
            album,
            track_no,
            disc_no,
            year,
            path,
            bpm,
            key,
            rating,
            comment,
            execute,
        } => {
            let fields = TrackFields {
                title,
                artist,
                genre,
                album,
                track_no,
                disc_no,
                year,
                path,
                bpm,
                key,
                rating,
                comment,
            };
            handle_tracks_update(pool, &id, fields, execute).await
        }
        TracksAction::Mytags { action } => handle_track_mytags(pool, action).await,
        TracksAction::BulkUpdate { file, execute } => {
            handle_tracks_bulk_update(pool, &file, execute).await
        }
        TracksAction::Cues { action } => handle_track_cues(pool, action).await,
    }
}

// --- Handlers: tracks filter ---

async fn handle_tracks_filter(
    pool: &SqlitePool,
    bpm_min: Option<f64>,
    bpm_max: Option<f64>,
    key: Option<String>,
    tag: Option<String>,
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
            (
                output::success("tracks", serde_json::Value::Array(items)),
                output::EXIT_OK,
            )
        }
        Err(e) => db_error(e),
    }
}

// --- describe ---

use crate::describe::{describe_command, describe_resource, flag, mutation_result_schema};

/// `rbx describe tracks [action]`. Sub-resources (`cues …`, `mytags …`) describe themselves.
pub(crate) fn describe(action: Option<&str>) -> Option<serde_json::Value> {
    Some(match action {
        None => describe_resource("tracks", &[
            ("list", "List all tracks (excludes streaming-only)"),
            ("get", "Get a single track by ID"),
            ("search", "Search tracks by title or artist name"),
            ("filter", "Filter tracks by BPM range, key, and/or tag"),
            ("update", "Update track fields (dry-run by default)"),
            ("bulk-update", "Update many tracks from a JSON plan file (dry-run by default)"),
            ("cues list", "List cue points on a track"),
            ("cues add", "Add a cue point (dry-run by default)"),
            ("cues update", "Update a cue point (dry-run by default)"),
            ("cues delete", "Delete a cue point (dry-run by default)"),
            ("mytags list", "List My Tags assigned to a track"),
            ("mytags add", "Add a My Tag to a track (dry-run by default)"),
            ("mytags remove", "Remove a My Tag from a track (dry-run by default)"),
        ]),
        Some("list") => describe_command("tracks list", &[], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &["rbx tracks list"]),
        Some("get") => describe_command("tracks get", &[
            flag("id", "string", true, "Track ID (from djmdContent.ID)"),
        ], &track_schema(), &["rbx tracks get 12345"]),
        Some("search") => describe_command("tracks search", &[
            flag("query", "string", true, "Search term (matched against title and artist)"),
        ], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &["rbx tracks search 'Butterfly'"]),
        Some("filter") => describe_command("tracks filter", &[
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
        Some("update") => describe_command("tracks update", &[
            flag("id", "string", true, "Track ID"),
            flag("--title", "string", false, "Track title"),
            flag("--artist", "string", false, "Artist name (resolved or created in djmdArtist; \"\" clears)"),
            flag("--genre", "string", false, "Genre name (resolved or created in djmdGenre; \"\" clears)"),
            flag("--album", "string", false, "Album name (resolved or created in djmdAlbum; \"\" clears)"),
            flag("--track-no", "integer", false, "Track number within the disc"),
            flag("--disc-no", "integer", false, "Disc number"),
            flag("--year", "integer", false, "Release year"),
            flag("--path", "string", false, "Full file path as rekordbox stores it (FolderPath; FileNameL follows its basename)"),
            flag("--bpm", "number", false, "BPM as decimal (e.g. 128.0)"),
            flag("--key", "string", false, "Musical key (e.g. '8A', '1B')"),
            flag("--rating", "integer", false, "Rating (0-5)"),
            flag("--comment", "string", false, "Comment text"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.update"), &[
            "rbx tracks update TRACK_ID --title 'New Title' --bpm 128.0",
            "rbx tracks update TRACK_ID --artist 'Artist' --key '8A' --execute",
        ]),
        Some("bulk-update") => describe_command(
            "tracks bulk-update",
            &[
                flag("file", "string", true, "JSON plan file, or \"-\" for stdin: [{\"id\": \"...\", \"fields\": {...}}, ...]. Field names are the tracks update flags in snake_case (title, artist, genre, album, track_no, disc_no, year, path, bpm, key, rating, comment)"),
                flag("--execute", "bool", false, "Actually apply the changes (default: dry-run)"),
            ],
            &mutation_result_schema("tracks.bulk_update"),
            &[
                "rbx tracks bulk-update updates.json",
                "rbx tracks bulk-update updates.json --execute",
            ],
        ),
        Some(a) if a.starts_with("cues ") => return cues::describe(a),
        Some(a) if a.starts_with("mytags ") => return mytags::describe(a),
        _ => return None,
    })
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
