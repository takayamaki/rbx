use rbx::helpers::{allocate_usns, generate_numeric_id, now_datetime};
use rbx::output;
use serde::Deserialize;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::commands::{db_error, resolve_track_summary};

async fn resolve_or_create_artist(pool: &SqlitePool, name: &str) -> Result<String, sqlx::Error> {
    if let Some((id,)) = sqlx::query_as::<_, (String,)>("SELECT ID FROM djmdArtist WHERE Name = ?")
        .bind(name)
        .fetch_optional(pool)
        .await?
    {
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
         VALUES (?, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(name)
    .bind(name)
    .bind(&new_uuid)
    .bind(usn)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(new_id)
}

/// Resolves a djmdGenre row by name, creating it in native format when missing.
async fn resolve_or_create_genre(pool: &SqlitePool, name: &str) -> Result<String, sqlx::Error> {
    if let Some((id,)) = sqlx::query_as::<_, (String,)>("SELECT ID FROM djmdGenre WHERE Name = ?")
        .bind(name)
        .fetch_optional(pool)
        .await?
    {
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
         VALUES (?, ?, ?, 0, 0, 0, 0, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(name)
    .bind(&new_uuid)
    .bind(usn)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(new_id)
}

/// Resolves a djmdAlbum row by name, creating it in native format when missing.
/// rekordbox fills AlbumArtistID / ImagePath / SearchStr with "" and Compilation with 0 for new albums.
async fn resolve_or_create_album(pool: &SqlitePool, name: &str) -> Result<String, sqlx::Error> {
    if let Some((id,)) = sqlx::query_as::<_, (String,)>("SELECT ID FROM djmdAlbum WHERE Name = ?")
        .bind(name)
        .fetch_optional(pool)
        .await?
    {
        return Ok(id);
    }
    let new_id = generate_numeric_id(pool, "djmdAlbum").await?;
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let usn = allocate_usns(pool, 1).await?;
    sqlx::query(
        "INSERT INTO djmdAlbum (ID, Name, AlbumArtistID, ImagePath, Compilation, SearchStr, UUID, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, '', '', 0, '', ?, 0, 0, 0, 0, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(name)
    .bind(&new_uuid)
    .bind(usn)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(new_id)
}

/// Resolves the FK value for a name-keyed table: "" clears the column
/// (rekordbox stores "no artist" etc. as an empty ID), anything else is resolved or created.
async fn resolve_fk<F, Fut>(name: &str, resolve: F) -> Result<String, sqlx::Error>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<String, sqlx::Error>>,
{
    if name.is_empty() {
        Ok(String::new())
    } else {
        resolve(name.to_string()).await
    }
}

async fn resolve_key_id(pool: &SqlitePool, key_name: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>("SELECT ID FROM djmdKey WHERE ScaleName = ?")
        .bind(key_name)
        .fetch_optional(pool)
        .await
        .map(|r| r.map(|(id,)| id))
}

/// Fields accepted by `tracks update` and by each row of `tracks bulk-update`.
/// `None` = leave the column alone. JSON keys are the flag names in snake_case
/// (`track_no`, `disc_no`).
#[derive(Default, Deserialize)]
pub(crate) struct TrackFields {
    pub(crate) title: Option<String>,
    pub(crate) artist: Option<String>,
    pub(crate) genre: Option<String>,
    pub(crate) album: Option<String>,
    pub(crate) track_no: Option<i32>,
    pub(crate) disc_no: Option<i32>,
    pub(crate) year: Option<i32>,
    pub(crate) path: Option<String>,
    pub(crate) bpm: Option<f64>,
    pub(crate) key: Option<String>,
    pub(crate) rating: Option<i32>,
    pub(crate) comment: Option<String>,
}

impl TrackFields {
    fn is_empty(&self) -> bool {
        self.changes().is_empty()
    }

    /// The requested changes as JSON, for plans and results.
    fn changes(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut changes = serde_json::Map::new();
        macro_rules! put {
            ($name:literal, $field:ident) => {
                if let Some(v) = &self.$field {
                    changes.insert($name.into(), serde_json::json!(v));
                }
            };
        }
        put!("title", title);
        put!("artist", artist);
        put!("genre", genre);
        put!("album", album);
        put!("track_no", track_no);
        put!("disc_no", disc_no);
        put!("year", year);
        put!("path", path);
        put!("bpm", bpm);
        put!("key", key);
        put!("rating", rating);
        put!("comment", comment);
        changes
    }
}

/// rekordbox stores the basename of FolderPath in FileNameL; keep them in sync.
fn file_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Everything that can be checked without writing: the track exists, at least
/// one field is set, the key name is known, the rating is in range.
/// Returns the track's current (title, artist) for the plan.
async fn validate(
    pool: &SqlitePool,
    track_id: &str,
    fields: &TrackFields,
) -> Result<(String, String), (serde_json::Value, i32)> {
    let summary = match resolve_track_summary(pool, track_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return Err((
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("Track not found: {}", track_id),
                    Some("Use 'rbx tracks list' to see available tracks"),
                ),
                output::EXIT_NOT_FOUND,
            ))
        }
        Err(e) => return Err(db_error(e)),
    };

    if fields.is_empty() {
        return Err((
            output::error("usage", output::EXIT_USAGE,
                "No fields specified to update",
                Some("Use --title, --artist, --genre, --album, --track-no, --disc-no, --year, --path, --bpm, --key, --rating, or --comment")),
            output::EXIT_USAGE,
        ));
    }

    if let Some(k) = &fields.key {
        match resolve_key_id(pool, k).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return Err((
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Unknown key: {}", k),
                        Some("Use 'rbx query \"SELECT ScaleName FROM djmdKey\"' to see valid keys"),
                    ),
                    output::EXIT_NOT_FOUND,
                ))
            }
            Err(e) => return Err(db_error(e)),
        }
    }

    if let Some(r) = fields.rating {
        if !(0..=5).contains(&r) {
            return Err((
                output::error(
                    "validation",
                    output::EXIT_CONFLICT,
                    &format!("Rating must be 0-5, got: {}", r),
                    None,
                ),
                output::EXIT_CONFLICT,
            ));
        }
    }

    Ok(summary)
}

/// Writes the fields of one validated track. Artist / genre / album names are
/// resolved or created first.
async fn apply(pool: &SqlitePool, track_id: &str, fields: &TrackFields) -> Result<(), sqlx::Error> {
    let artist_id = match &fields.artist {
        Some(v) => Some(
            resolve_fk(
                v,
                |n| async move { resolve_or_create_artist(pool, &n).await },
            )
            .await?,
        ),
        None => None,
    };
    let genre_id = match &fields.genre {
        Some(v) => Some(
            resolve_fk(
                v,
                |n| async move { resolve_or_create_genre(pool, &n).await },
            )
            .await?,
        ),
        None => None,
    };
    let album_id = match &fields.album {
        Some(v) => Some(
            resolve_fk(
                v,
                |n| async move { resolve_or_create_album(pool, &n).await },
            )
            .await?,
        ),
        None => None,
    };
    let key_id = match &fields.key {
        // validated: the key name exists
        Some(v) => resolve_key_id(pool, v).await?,
        None => None,
    };

    let now = now_datetime();
    macro_rules! update_field {
        ($col:expr, $val:expr) => {
            let sql = format!(
                "UPDATE djmdContent SET {} = ?, updated_at = ? WHERE ID = ?",
                $col
            );
            sqlx::query(&sql)
                .bind($val)
                .bind(&now)
                .bind(track_id)
                .execute(pool)
                .await?;
        };
    }

    if let Some(v) = &fields.title {
        update_field!("Title", v);
    }
    if let Some(v) = &artist_id {
        update_field!("ArtistID", v);
    }
    if let Some(v) = &genre_id {
        update_field!("GenreID", v);
    }
    if let Some(v) = &album_id {
        update_field!("AlbumID", v);
    }
    if let Some(v) = fields.track_no {
        update_field!("TrackNo", v);
    }
    if let Some(v) = fields.disc_no {
        update_field!("DiscNo", v);
    }
    if let Some(v) = fields.year {
        update_field!("ReleaseYear", v);
    }
    if let Some(v) = &fields.path {
        update_field!("FolderPath", v);
        update_field!("FileNameL", file_name_of(v));
    }
    if let Some(v) = fields.bpm {
        let bpm_int = (v * 100.0) as i32;
        update_field!("BPM", bpm_int);
    }
    if let Some(v) = &key_id {
        update_field!("KeyID", v);
    }
    if let Some(v) = fields.rating {
        update_field!("Rating", v);
    }
    if let Some(v) = &fields.comment {
        update_field!("Commnt", v);
    }
    Ok(())
}

pub(crate) async fn handle_tracks_update(
    pool: &SqlitePool,
    track_id: &str,
    fields: TrackFields,
    execute: bool,
) -> (serde_json::Value, i32) {
    let (cur_title, cur_artist) = match validate(pool, track_id, &fields).await {
        Ok(t) => t,
        Err(e) => return e,
    };
    let track = serde_json::json!({ "id": track_id, "title": cur_title, "artist": cur_artist });
    let changes = serde_json::Value::Object(fields.changes());

    if !execute {
        let plan = serde_json::json!({
            "action": "update_track",
            "track": track,
            "changes": changes,
        });
        return (
            output::mutation_dry_run("tracks.update", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    if let Err(e) = apply(pool, track_id, &fields).await {
        return db_error(e);
    }
    (
        output::mutation_done(
            "tracks.update",
            serde_json::json!({ "track": track, "changes": changes }),
        ),
        output::EXIT_OK,
    )
}

// --- bulk-update ---

/// One row of a `tracks bulk-update` plan.
#[derive(Deserialize)]
struct BulkRow {
    id: String,
    fields: TrackFields,
}

fn read_plan(file: &str) -> Result<Vec<BulkRow>, (serde_json::Value, i32)> {
    let text = std::fs::read_to_string(file).map_err(|e| {
        (
            output::error(
                "usage",
                output::EXIT_USAGE,
                &format!("Cannot read plan file {}: {}", file, e),
                Some(
                    "Pass a JSON file: [{\"id\": \"...\", \"fields\": {\"title\": \"...\"}}, ...]",
                ),
            ),
            output::EXIT_USAGE,
        )
    })?;
    serde_json::from_str(&text).map_err(|e| {
        (
            output::error(
                "usage",
                output::EXIT_USAGE,
                &format!("Invalid plan JSON: {}", e),
                Some("Expected an array of {\"id\": \"...\", \"fields\": {...}}; field names are the tracks update flags in snake_case"),
            ),
            output::EXIT_USAGE,
        )
    })
}

pub(crate) async fn handle_tracks_bulk_update(
    pool: &SqlitePool,
    file: &str,
    execute: bool,
) -> (serde_json::Value, i32) {
    let rows = match read_plan(file) {
        Ok(r) => r,
        Err(e) => return e,
    };

    let mut items = Vec::with_capacity(rows.len());
    for row in &rows {
        match validate(pool, &row.id, &row.fields).await {
            Ok((title, artist)) => items.push(serde_json::json!({
                "track": { "id": row.id, "title": title, "artist": artist },
                "changes": serde_json::Value::Object(row.fields.changes()),
            })),
            Err(e) => return e,
        }
    }

    if !execute {
        let plan = serde_json::json!({
            "action": "bulk_update_tracks",
            "count": rows.len(),
            "items": items,
        });
        return (
            output::mutation_dry_run("tracks.bulk_update", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    for row in &rows {
        if let Err(e) = apply(pool, &row.id, &row.fields).await {
            return db_error(e);
        }
    }
    (
        output::mutation_done(
            "tracks.bulk_update",
            serde_json::json!({ "updated": rows.len() }),
        ),
        output::EXIT_OK,
    )
}
