use rbx::output;
use sqlx::sqlite::SqlitePool;

use crate::cli::HistoryAction;
use crate::commands::db_error;

pub(crate) async fn handle_history(
    pool: &SqlitePool,
    action: HistoryAction,
) -> (serde_json::Value, i32) {
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

// --- describe ---

use crate::describe::{describe_command, describe_resource, flag};

pub(crate) fn describe(action: Option<&str>) -> Option<serde_json::Value> {
    Some(match action {
        None => describe_resource(
            "history",
            &[
                ("list", "List play history sessions (most recent first)"),
                ("tracks", "List tracks in a history session"),
            ],
        ),
        Some("list") => describe_command(
            "history list",
            &[flag(
                "--limit",
                "integer",
                false,
                "Max sessions to return (default: 20)",
            )],
            &serde_json::json!({
                "type": "array", "items": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "name": { "type": "string" },
                        "date": { "type": "string" },
                        "track_count": { "type": "integer" },
                    },
                },
            }),
            &["rbx history list", "rbx history list --limit 5"],
        ),
        Some("tracks") => describe_command(
            "history tracks",
            &[flag("id", "string", true, "History session ID")],
            &serde_json::json!({
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
            }),
            &["rbx history tracks HISTORY_ID"],
        ),

        // query
        _ => return None,
    })
}
