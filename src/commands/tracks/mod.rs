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
use update::{handle_tracks_update, TrackFields};

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
