use rbx::helpers::{allocate_usns, generate_numeric_id, now_datetime};
use rbx::output;
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

/// Fields accepted by `tracks update`. `None` = leave the column alone.
#[derive(Default)]
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
        self.title.is_none()
            && self.artist.is_none()
            && self.genre.is_none()
            && self.album.is_none()
            && self.track_no.is_none()
            && self.disc_no.is_none()
            && self.year.is_none()
            && self.path.is_none()
            && self.bpm.is_none()
            && self.key.is_none()
            && self.rating.is_none()
            && self.comment.is_none()
    }
}

/// rekordbox stores the basename of FolderPath in FileNameL; keep them in sync.
fn file_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub(crate) async fn handle_tracks_update(
    pool: &SqlitePool,
    track_id: &str,
    fields: TrackFields,
    execute: bool,
) -> (serde_json::Value, i32) {
    let TrackFields {
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
    } = &fields;
    // Verify track exists
    let (cur_title, cur_artist) = match resolve_track_summary(pool, track_id).await {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("Track not found: {}", track_id),
                    Some("Use 'rbx tracks list' to see available tracks"),
                ),
                output::EXIT_NOT_FOUND,
            )
        }
        Err(e) => return db_error(e),
    };

    if fields.is_empty() {
        return (
            output::error("usage", output::EXIT_USAGE,
                "No fields specified to update",
                Some("Use --title, --artist, --genre, --album, --track-no, --disc-no, --year, --path, --bpm, --key, --rating, or --comment")),
            output::EXIT_USAGE,
        );
    }

    // Validate key name if provided
    if let Some(ref k) = key {
        match resolve_key_id(pool, k).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Unknown key: {}", k),
                        Some("Use 'rbx query \"SELECT ScaleName FROM djmdKey\"' to see valid keys"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
            Err(e) => return db_error(e),
        }
    }

    if let Some(r) = *rating {
        if !(0..=5).contains(&r) {
            return (
                output::error(
                    "validation",
                    output::EXIT_CONFLICT,
                    &format!("Rating must be 0-5, got: {}", r),
                    None,
                ),
                output::EXIT_CONFLICT,
            );
        }
    }

    let mut changes = serde_json::Map::new();
    if let Some(v) = title {
        changes.insert("title".into(), serde_json::json!(v));
    }
    if let Some(v) = artist {
        changes.insert("artist".into(), serde_json::json!(v));
    }
    if let Some(v) = genre {
        changes.insert("genre".into(), serde_json::json!(v));
    }
    if let Some(v) = album {
        changes.insert("album".into(), serde_json::json!(v));
    }
    if let Some(v) = track_no {
        changes.insert("track_no".into(), serde_json::json!(v));
    }
    if let Some(v) = disc_no {
        changes.insert("disc_no".into(), serde_json::json!(v));
    }
    if let Some(v) = year {
        changes.insert("year".into(), serde_json::json!(v));
    }
    if let Some(v) = path {
        changes.insert("path".into(), serde_json::json!(v));
    }
    if let Some(v) = bpm {
        changes.insert("bpm".into(), serde_json::json!(v));
    }
    if let Some(v) = key {
        changes.insert("key".into(), serde_json::json!(v));
    }
    if let Some(v) = rating {
        changes.insert("rating".into(), serde_json::json!(v));
    }
    if let Some(v) = comment {
        changes.insert("comment".into(), serde_json::json!(v));
    }

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
        match resolve_fk(
            v,
            |n| async move { resolve_or_create_artist(pool, &n).await },
        )
        .await
        {
            Ok(id) => Some(id),
            Err(e) => return db_error(e),
        }
    } else {
        None
    };

    let genre_id = if let Some(v) = genre {
        match resolve_fk(
            v,
            |n| async move { resolve_or_create_genre(pool, &n).await },
        )
        .await
        {
            Ok(id) => Some(id),
            Err(e) => return db_error(e),
        }
    } else {
        None
    };

    let album_id = if let Some(v) = album {
        match resolve_fk(
            v,
            |n| async move { resolve_or_create_album(pool, &n).await },
        )
        .await
        {
            Ok(id) => Some(id),
            Err(e) => return db_error(e),
        }
    } else {
        None
    };

    let key_id = if let Some(v) = key {
        match resolve_key_id(pool, v).await {
            Ok(Some(id)) => Some(id),
            _ => unreachable!(),
        }
    } else {
        None
    };

    // Execute individual UPDATEs per field
    let now = now_datetime();
    let mut errors = Vec::new();

    macro_rules! update_field {
        ($col:expr, $val:expr) => {
            let sql = format!(
                "UPDATE djmdContent SET {} = ?, updated_at = ? WHERE ID = ?",
                $col
            );
            if let Err(e) = sqlx::query(&sql)
                .bind($val)
                .bind(&now)
                .bind(track_id)
                .execute(pool)
                .await
            {
                errors.push(e.to_string());
            }
        };
    }

    if let Some(v) = title {
        update_field!("Title", v);
    }
    if let Some(ref v) = artist_id {
        update_field!("ArtistID", v);
    }
    if let Some(ref v) = genre_id {
        update_field!("GenreID", v);
    }
    if let Some(ref v) = album_id {
        update_field!("AlbumID", v);
    }
    if let Some(v) = track_no {
        update_field!("TrackNo", v);
    }
    if let Some(v) = disc_no {
        update_field!("DiscNo", v);
    }
    if let Some(v) = year {
        update_field!("ReleaseYear", v);
    }
    if let Some(v) = path {
        update_field!("FolderPath", v);
        update_field!("FileNameL", file_name_of(v));
    }
    if let Some(v) = bpm {
        let bpm_int = (v * 100.0) as i32;
        update_field!("BPM", &bpm_int);
    }
    if let Some(ref v) = key_id {
        update_field!("KeyID", v);
    }
    if let Some(v) = rating {
        update_field!("Rating", v);
    }
    if let Some(v) = comment {
        update_field!("Commnt", v);
    }

    if !errors.is_empty() {
        return db_error(sqlx::Error::Protocol(errors.join("; ")));
    }

    (
        output::mutation_done(
            "tracks.update",
            serde_json::json!({
                "track": { "id": track_id, "title": cur_title, "artist": cur_artist },
                "changes": serde_json::Value::Object(changes),
            }),
        ),
        output::EXIT_OK,
    )
}
