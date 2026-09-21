use rbx::helpers::{allocate_usns, now_datetime};
use rbx::output;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::cli::TrackMytagsAction;
use crate::commands::{db_error, resolve_tag_name, resolve_track_summary};
use crate::rows::TrackMyTagRow;

pub(crate) async fn handle_track_mytags(
    pool: &SqlitePool,
    action: TrackMytagsAction,
) -> (serde_json::Value, i32) {
    match action {
        TrackMytagsAction::List { track_id } => {
            if resolve_track_summary(pool, &track_id)
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
            match sqlx::query_as::<_, TrackMyTagRow>(
                "SELECT DISTINCT t.ID as tag_id, t.Name as tag_name, p.Name as category_name \
                 FROM djmdSongMyTag smt \
                 JOIN djmdMyTag t ON smt.MyTagID = t.ID \
                 LEFT JOIN djmdMyTag p ON t.ParentID = p.ID \
                 WHERE smt.ContentID = ? AND smt.rb_local_deleted = 0 AND t.rb_local_deleted = 0",
            )
            .bind(&track_id)
            .fetch_all(pool)
            .await
            {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (
                        output::success("track_mytags", serde_json::Value::Array(items)),
                        output::EXIT_OK,
                    )
                }
                Err(e) => db_error(e),
            }
        }
        TrackMytagsAction::Add {
            track_id,
            tag_ids,
            execute,
        } => handle_track_mytag_add(pool, &track_id, &tag_ids, execute).await,
        TrackMytagsAction::Remove {
            track_id,
            tag_ids,
            execute,
        } => handle_track_mytag_remove(pool, &track_id, &tag_ids, execute).await,
    }
}

async fn handle_track_mytag_add(
    pool: &SqlitePool,
    track_id: &str,
    tag_ids: &[String],
    execute: bool,
) -> (serde_json::Value, i32) {
    let (title, artist) = match resolve_track_summary(pool, track_id).await {
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

    let mut tags = Vec::new();
    let mut skipped = Vec::new();
    for tid in tag_ids {
        let tag_name = match resolve_tag_name(pool, tid).await {
            Ok(Some(n)) => n,
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("My Tag not found: {}", tid),
                        Some("Use 'rbx mytags list' to see available tags"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
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
            output::error(
                "conflict",
                output::EXIT_CONFLICT,
                "All specified tags are already assigned",
                None,
            ),
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
        output::mutation_done(
            "tracks.mytags.add",
            serde_json::json!({
                "track": { "id": track_id, "title": title, "artist": artist },
                "added": added,
                "skipped": skipped,
            }),
        ),
        output::EXIT_OK,
    )
}

async fn handle_track_mytag_remove(
    pool: &SqlitePool,
    track_id: &str,
    tag_ids: &[String],
    execute: bool,
) -> (serde_json::Value, i32) {
    let (title, _) = match resolve_track_summary(pool, track_id).await {
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

    let mut targets = Vec::new();
    for tid in tag_ids {
        let tag_name = match resolve_tag_name(pool, tid).await {
            Ok(Some(n)) => n,
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("My Tag not found: {}", tid),
                        Some("Use 'rbx mytags list' to see available tags"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
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
            "UPDATE djmdSongMyTag SET rb_local_deleted = 1, updated_at = ? WHERE ID = ?",
        )
        .bind(&now)
        .bind(row_id)
        .execute(pool)
        .await;
    }
    sqlx::query("COMMIT").execute(pool).await.ok();

    (
        output::mutation_done(
            "tracks.mytags.remove",
            serde_json::json!({
                "track": { "id": track_id, "title": title },
                "removed_count": targets.len(),
            }),
        ),
        output::EXIT_OK,
    )
}
