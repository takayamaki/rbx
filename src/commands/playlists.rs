use std::path::Path;

use rbx::helpers::{allocate_usns, generate_numeric_id, now_datetime};
use rbx::{output, playlist_xml};
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::cli::{PlaylistTracksAction, PlaylistsAction};
use crate::commands::{db_error, resolve_track_summary};
use crate::rows::{PlaylistRow, PlaylistTrackRow};

async fn resolve_playlist_name(pool: &SqlitePool, id: &str) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_as::<_, (String,)>("SELECT Name FROM djmdPlaylist WHERE ID = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map(|r| r.map(|(n,)| n))
}

pub(crate) async fn handle_playlists(
    pool: &SqlitePool,
    db_path: &Path,
    action: PlaylistsAction,
) -> (serde_json::Value, i32) {
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

async fn handle_playlist_tracks(
    pool: &SqlitePool,
    action: PlaylistTracksAction,
) -> (serde_json::Value, i32) {
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
                 ORDER BY sp.TrackNo",
            )
            .bind(&playlist_id)
            .fetch_all(pool)
            .await
            {
                Ok(rows) if rows.is_empty() => (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Playlist not found or empty: {}", playlist_id),
                        Some("Use 'rbx playlists list' to see available playlists"),
                    ),
                    output::EXIT_NOT_FOUND,
                ),
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (
                        output::success("playlist_tracks", serde_json::Value::Array(items)),
                        output::EXIT_OK,
                    )
                }
                Err(e) => db_error(e),
            }
        }
        PlaylistTracksAction::Add {
            playlist_id,
            track_ids,
            execute,
        } => handle_playlist_track_add(pool, &playlist_id, &track_ids, execute).await,
        PlaylistTracksAction::Remove {
            playlist_id,
            track_ids,
            execute,
        } => handle_playlist_track_remove(pool, &playlist_id, &track_ids, execute).await,
    }
}

async fn handle_playlist_track_add(
    pool: &SqlitePool,
    playlist_id: &str,
    track_ids: &[String],
    execute: bool,
) -> (serde_json::Value, i32) {
    let pl_name = match resolve_playlist_name(pool, playlist_id).await {
        Ok(Some(n)) => n,
        Ok(None) => {
            return (
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("Playlist not found: {}", playlist_id),
                    Some("Use 'rbx playlists list' to see available playlists"),
                ),
                output::EXIT_NOT_FOUND,
            )
        }
        Err(e) => return db_error(e),
    };

    let mut entries = Vec::new();
    for tid in track_ids {
        match resolve_track_summary(pool, tid).await {
            Ok(Some((title, artist))) => entries.push(serde_json::json!({
                "id": tid, "title": title, "artist": artist,
            })),
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Track not found: {}", tid),
                        Some("Use 'rbx tracks list' to see available tracks"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
            Err(e) => return db_error(e),
        }
    }

    let max_track_no = match sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(TrackNo) FROM djmdSongPlaylist WHERE PlaylistID = ?",
    )
    .bind(playlist_id)
    .fetch_one(pool)
    .await
    {
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
            output::mutation_dry_run("playlists.tracks.add", plan, "Add --execute to apply"),
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
        output::mutation_done(
            "playlists.tracks.add",
            serde_json::json!({
                "playlist": { "id": playlist_id, "name": pl_name },
                "added": results,
            }),
        ),
        output::EXIT_OK,
    )
}

async fn handle_playlist_track_remove(
    pool: &SqlitePool,
    playlist_id: &str,
    track_ids: &[String],
    execute: bool,
) -> (serde_json::Value, i32) {
    let pl_name = match resolve_playlist_name(pool, playlist_id).await {
        Ok(Some(n)) => n,
        Ok(None) => {
            return (
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("Playlist not found: {}", playlist_id),
                    Some("Use 'rbx playlists list' to see available playlists"),
                ),
                output::EXIT_NOT_FOUND,
            )
        }
        Err(e) => return db_error(e),
    };

    let mut targets = Vec::new();
    for tid in track_ids {
        let (title, _) = match resolve_track_summary(pool, tid).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Track not found: {}", tid),
                        Some("Use 'rbx tracks list' to see available tracks"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
            Err(e) => return db_error(e),
        };
        let existing = match sqlx::query_as::<_, (String, i32)>(
            "SELECT ID, TrackNo FROM djmdSongPlaylist WHERE PlaylistID = ? AND ContentID = ?",
        )
        .bind(playlist_id)
        .bind(tid)
        .fetch_optional(pool)
        .await
        {
            Ok(Some(r)) => r,
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Track '{}' is not in playlist '{}'", title, pl_name),
                        Some("Use 'rbx playlists tracks list <playlist_id>' to see tracks"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
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
            output::mutation_dry_run("playlists.tracks.remove", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    sqlx::query("BEGIN").execute(pool).await.ok();
    for (_, row_id, _) in &targets {
        let _ = sqlx::query("DELETE FROM djmdSongPlaylist WHERE ID = ?")
            .bind(row_id)
            .execute(pool)
            .await;
    }

    // Renumber all remaining tracks sequentially
    let remaining = sqlx::query_as::<_, (String,)>(
        "SELECT ID FROM djmdSongPlaylist WHERE PlaylistID = ? ORDER BY TrackNo",
    )
    .bind(playlist_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (i, (row_id,)) in remaining.iter().enumerate() {
        let _ = sqlx::query("UPDATE djmdSongPlaylist SET TrackNo = ? WHERE ID = ?")
            .bind((i + 1) as i32)
            .bind(row_id)
            .execute(pool)
            .await;
    }
    sqlx::query("COMMIT").execute(pool).await.ok();

    (
        output::mutation_done(
            "playlists.tracks.remove",
            serde_json::json!({
                "playlist": { "id": playlist_id, "name": pl_name },
                "removed_count": targets.len(),
            }),
        ),
        output::EXIT_OK,
    )
}

// --- Handlers: playlists search ---

async fn handle_playlists_search(pool: &SqlitePool, track_id: &str) -> (serde_json::Value, i32) {
    if resolve_track_summary(pool, track_id)
        .await
        .ok()
        .flatten()
        .is_none()
    {
        return (
            output::error(
                "not_found",
                output::EXIT_NOT_FOUND,
                &format!("Track not found: {}", track_id),
                Some("Use 'rbx tracks list' to see available tracks"),
            ),
            output::EXIT_NOT_FOUND,
        );
    }

    match sqlx::query_as::<_, (String, String, i32)>(
        "SELECT p.ID, COALESCE(p.Name, ''), sp.TrackNo \
         FROM djmdSongPlaylist sp \
         JOIN djmdPlaylist p ON sp.PlaylistID = p.ID \
         WHERE sp.ContentID = ? \
         ORDER BY p.Name",
    )
    .bind(track_id)
    .fetch_all(pool)
    .await
    {
        Ok(rows) => {
            let items: Vec<_> = rows
                .iter()
                .map(|(id, name, track_no)| {
                    serde_json::json!({
                        "playlist_id": id,
                        "playlist_name": name,
                        "track_no": track_no,
                    })
                })
                .collect();
            (
                output::success("track_playlists", serde_json::Value::Array(items)),
                output::EXIT_OK,
            )
        }
        Err(e) => db_error(e),
    }
}

// --- Handlers: playlists create/delete ---

async fn handle_playlists_create(
    pool: &SqlitePool,
    db_path: &Path,
    name: &str,
    parent_id: Option<&str>,
    execute: bool,
) -> (serde_json::Value, i32) {
    if let Some(pid) = parent_id {
        match resolve_playlist_name(pool, pid).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Parent folder not found: {}", pid),
                        Some("Use 'rbx playlists list' to see available folders"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
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
            output::mutation_dry_run("playlists.create", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let new_id = match generate_numeric_id(pool, "djmdPlaylist").await {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let now_ms = chrono::Utc::now().timestamp_millis();
    let usn = match allocate_usns(pool, 1).await {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    // Seq: max within same parent + 1
    let seq = match sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(Seq) FROM djmdPlaylist WHERE ParentID = ? AND rb_local_deleted = 0",
    )
    .bind(parent)
    .fetch_one(pool)
    .await
    {
        Ok((s,)) => s.unwrap_or(0) + 1,
        Err(e) => return db_error(e),
    };
    // Attribute 0 = playlist (not folder)
    match sqlx::query(
        "INSERT INTO djmdPlaylist (ID, Seq, Name, Attribute, ParentID, UUID, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, 0, ?, ?, 0, 0, 0, 0, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(seq)
    .bind(name)
    .bind(parent)
    .bind(&new_uuid)
    .bind(usn)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    {
        Ok(_) => {
            let xml_path = playlist_xml::xml_path_for(db_path);
            let xml_updated =
                playlist_xml::add_node(&xml_path, &new_id, parent, 0, now_ms).unwrap_or(false);
            (
                output::mutation_done(
                    "playlists.create",
                    serde_json::json!({
                        "id": new_id,
                        "name": name,
                        "kind": "playlist",
                        "parent_id": parent,
                        "seq": seq,
                        "playlist_xml_updated": xml_updated,
                    }),
                ),
                output::EXIT_OK,
            )
        }
        Err(e) => db_error(e),
    }
}

async fn handle_playlists_delete(
    pool: &SqlitePool,
    db_path: &Path,
    playlist_id: &str,
    execute: bool,
) -> (serde_json::Value, i32) {
    let pl = match sqlx::query_as::<_, PlaylistRow>(
        "SELECT ID as id, Name as name, Attribute as attribute, ParentID as parent_id \
         FROM djmdPlaylist WHERE ID = ?",
    )
    .bind(playlist_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("Playlist not found: {}", playlist_id),
                    Some("Use 'rbx playlists list' to see available playlists"),
                ),
                output::EXIT_NOT_FOUND,
            )
        }
        Err(e) => return db_error(e),
    };

    // Count tracks that would be removed
    let track_count =
        sqlx::query_as::<_, (i32,)>("SELECT COUNT(*) FROM djmdSongPlaylist WHERE PlaylistID = ?")
            .bind(playlist_id)
            .fetch_one(pool)
            .await
            .map(|(c,)| c)
            .unwrap_or(0);

    let plan = serde_json::json!({
        "action": "delete_playlist",
        "playlist": pl.to_json(),
        "track_count": track_count,
    });

    if !execute {
        return (
            output::mutation_dry_run("playlists.delete", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    // Delete track entries first
    let _ = sqlx::query("DELETE FROM djmdSongPlaylist WHERE PlaylistID = ?")
        .bind(playlist_id)
        .execute(pool)
        .await;

    match sqlx::query("DELETE FROM djmdPlaylist WHERE ID = ?")
        .bind(playlist_id)
        .execute(pool)
        .await
    {
        Ok(_) => {
            let xml_path = playlist_xml::xml_path_for(db_path);
            let xml_updated = playlist_xml::remove_node(&xml_path, playlist_id).unwrap_or(false);
            (
                output::mutation_done(
                    "playlists.delete",
                    serde_json::json!({
                        "playlist": pl.to_json(),
                        "tracks_removed": track_count,
                        "playlist_xml_updated": xml_updated,
                    }),
                ),
                output::EXIT_OK,
            )
        }
        Err(e) => db_error(e),
    }
}

// --- describe ---

use crate::describe::{describe_command, describe_resource, flag, mutation_result_schema};

pub(crate) fn describe(action: Option<&str>) -> Option<serde_json::Value> {
    Some(match action {
        None => describe_resource(
            "playlists",
            &[
                ("list", "List all playlists and folders"),
                ("tracks list", "List tracks in a specific playlist"),
                (
                    "tracks add",
                    "Add a track to a playlist (dry-run by default)",
                ),
                (
                    "tracks remove",
                    "Remove a track from a playlist (dry-run by default)",
                ),
                ("search", "Find playlists containing a specific track"),
                ("create", "Create a new playlist (dry-run by default)"),
                ("delete", "Delete a playlist (dry-run by default)"),
            ],
        ),
        Some("list") => describe_command(
            "playlists list",
            &[],
            &serde_json::json!({
                "type": "array", "items": playlist_schema(),
            }),
            &["rbx playlists list"],
        ),
        Some("tracks list") => describe_command(
            "playlists tracks list",
            &[flag("playlist_id", "string", true, "Playlist ID")],
            &serde_json::json!({
                "type": "array", "items": playlist_track_schema(),
            }),
            &["rbx playlists tracks list PLAYLIST_ID"],
        ),
        Some("tracks add") => describe_command(
            "playlists tracks add",
            &[
                flag("playlist_id", "string", true, "Playlist ID"),
                flag("track_id", "string", true, "Track ID"),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually apply the change (default: dry-run)",
                ),
            ],
            &mutation_result_schema("playlists.tracks.add"),
            &[
                "rbx playlists tracks add PLAYLIST_ID TRACK_ID",
                "rbx playlists tracks add PLAYLIST_ID TRACK_ID --execute",
            ],
        ),
        Some("tracks remove") => describe_command(
            "playlists tracks remove",
            &[
                flag("playlist_id", "string", true, "Playlist ID"),
                flag("track_id", "string", true, "Track ID"),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually apply the change (default: dry-run)",
                ),
            ],
            &mutation_result_schema("playlists.tracks.remove"),
            &[
                "rbx playlists tracks remove PLAYLIST_ID TRACK_ID",
                "rbx playlists tracks remove PLAYLIST_ID TRACK_ID --execute",
            ],
        ),
        Some("search") => describe_command(
            "playlists search",
            &[flag("track_id", "string", true, "Track ID to search for")],
            &serde_json::json!({
                "type": "array", "items": {
                    "type": "object",
                    "properties": {
                        "playlist_id": { "type": "string" },
                        "playlist_name": { "type": "string" },
                        "track_no": { "type": "integer" },
                    },
                },
            }),
            &["rbx playlists search TRACK_ID"],
        ),
        Some("create") => describe_command(
            "playlists create",
            &[
                flag("name", "string", true, "Playlist name"),
                flag(
                    "--parent",
                    "string",
                    false,
                    "Parent folder ID (omit for top-level)",
                ),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually apply the change (default: dry-run)",
                ),
            ],
            &mutation_result_schema("playlists.create"),
            &[
                "rbx playlists create 'My Playlist'",
                "rbx playlists create 'My Playlist' --parent FOLDER_ID --execute",
            ],
        ),
        Some("delete") => describe_command(
            "playlists delete",
            &[
                flag("id", "string", true, "Playlist ID"),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually apply the change (default: dry-run)",
                ),
            ],
            &mutation_result_schema("playlists.delete"),
            &[
                "rbx playlists delete PLAYLIST_ID",
                "rbx playlists delete PLAYLIST_ID --execute",
            ],
        ),

        // mytags
        _ => return None,
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
