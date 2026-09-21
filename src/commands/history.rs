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
