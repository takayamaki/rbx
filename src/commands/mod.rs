pub(crate) mod history;
pub(crate) mod mytags;
pub(crate) mod playlists;
pub(crate) mod query;
pub(crate) mod tracks;

use rbx::output;
use sqlx::sqlite::SqlitePool;

pub(crate) async fn resolve_tag_name(
    pool: &SqlitePool,
    tag_id: &str,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>(
        "SELECT Name FROM djmdMyTag WHERE ID = ? AND rb_local_deleted = 0",
    )
    .bind(tag_id)
    .fetch_optional(pool)
    .await
    .map(|r| r.map(|(n,)| n))
}

pub(crate) async fn resolve_track_summary(
    pool: &SqlitePool,
    content_id: &str,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as::<_, (String, String)>(
        "SELECT c.Title, COALESCE(a.Name, '') \
         FROM djmdContent c LEFT JOIN djmdArtist a ON c.ArtistID = a.ID \
         WHERE c.ID = ?",
    )
    .bind(content_id)
    .fetch_optional(pool)
    .await
}

pub(crate) fn db_error(e: sqlx::Error) -> (serde_json::Value, i32) {
    (
        output::error("database", output::EXIT_GENERAL, &e.to_string(), None),
        output::EXIT_GENERAL,
    )
}
