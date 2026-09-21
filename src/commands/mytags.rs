use rbx::helpers::{allocate_usns, generate_numeric_id, now_datetime};
use rbx::output;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::cli::MytagsAction;
use crate::commands::{db_error, resolve_tag_name};
use crate::rows::{MyTagRow, MyTagTrackRow};

pub(crate) async fn handle_mytags(
    pool: &SqlitePool,
    action: MytagsAction,
) -> (serde_json::Value, i32) {
    match action {
        MytagsAction::List => {
            match sqlx::query_as::<_, MyTagRow>(
                "SELECT ID as id, Seq as seq, Name as name, Attribute as attribute, \
                 ParentID as parent_id \
                 FROM djmdMyTag WHERE rb_local_deleted = 0 ORDER BY Seq",
            )
            .fetch_all(pool)
            .await
            {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (
                        output::success("mytags", serde_json::Value::Array(items)),
                        output::EXIT_OK,
                    )
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
                 WHERE smt.MyTagID = ? AND smt.rb_local_deleted = 0",
            )
            .bind(&id)
            .fetch_all(pool)
            .await
            {
                Ok(rows) if rows.is_empty() => (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("My Tag not found or has no tracks: {}", id),
                        Some("Use 'rbx mytags list' to see available tags"),
                    ),
                    output::EXIT_NOT_FOUND,
                ),
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (
                        output::success("mytag_tracks", serde_json::Value::Array(items)),
                        output::EXIT_OK,
                    )
                }
                Err(e) => db_error(e),
            }
        }
        MytagsAction::Create {
            name,
            parent,
            execute,
        } => handle_mytags_create(pool, &name, parent.as_deref(), execute).await,
        MytagsAction::Delete { id, execute } => handle_mytags_delete(pool, &id, execute).await,
    }
}

async fn handle_mytags_create(
    pool: &SqlitePool,
    name: &str,
    parent_id: Option<&str>,
    execute: bool,
) -> (serde_json::Value, i32) {
    // attribute: 0 = tag (has parent), 1 = category (top-level)
    let attribute = if parent_id.is_some() { 0 } else { 1 };

    if let Some(pid) = parent_id {
        match resolve_tag_name(pool, pid).await {
            Ok(Some(_)) => {}
            Ok(None) => {
                return (
                    output::error(
                        "not_found",
                        output::EXIT_NOT_FOUND,
                        &format!("Parent category not found: {}", pid),
                        Some("Use 'rbx mytags list' to see available categories"),
                    ),
                    output::EXIT_NOT_FOUND,
                )
            }
            Err(e) => return db_error(e),
        }
    }

    // Check for duplicate name under same parent
    let dup = match sqlx::query_as::<_, (String,)>(
        "SELECT ID FROM djmdMyTag WHERE Name = ? AND ParentID IS ? AND rb_local_deleted = 0",
    )
    .bind(name)
    .bind(parent_id)
    .fetch_optional(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => return db_error(e),
    };

    if let Some((existing_id,)) = dup {
        return (
            output::error(
                "conflict",
                output::EXIT_CONFLICT,
                &format!("My Tag '{}' already exists (ID: {})", name, existing_id),
                None,
            ),
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
            output::mutation_dry_run("mytags.create", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let new_id = match generate_numeric_id(pool, "djmdMyTag").await {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let usn = match allocate_usns(pool, 1).await {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };

    // Seq: max existing + 1
    let max_seq = sqlx::query_as::<_, (Option<i32>,)>(
        "SELECT MAX(Seq) FROM djmdMyTag WHERE rb_local_deleted = 0",
    )
    .fetch_one(pool)
    .await
    .map(|(s,)| s.unwrap_or(0))
    .unwrap_or(0);

    match sqlx::query(
        "INSERT INTO djmdMyTag (ID, Seq, Name, Attribute, ParentID, UUID, \
         rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)",
    )
    .bind(&new_id)
    .bind(max_seq + 1)
    .bind(name)
    .bind(attribute)
    .bind(parent_id)
    .bind(&new_uuid)
    .bind(usn)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    {
        Ok(_) => (
            output::mutation_done(
                "mytags.create",
                serde_json::json!({
                    "id": new_id,
                    "name": name,
                    "kind": kind,
                    "parent_id": parent_id,
                    "seq": max_seq + 1,
                }),
            ),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}

async fn handle_mytags_delete(
    pool: &SqlitePool,
    tag_id: &str,
    execute: bool,
) -> (serde_json::Value, i32) {
    let tag = match sqlx::query_as::<_, MyTagRow>(
        "SELECT ID as id, Seq as seq, Name as name, Attribute as attribute, ParentID as parent_id \
         FROM djmdMyTag WHERE ID = ? AND rb_local_deleted = 0",
    )
    .bind(tag_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(t)) => t,
        Ok(None) => {
            return (
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("My Tag not found: {}", tag_id),
                    Some("Use 'rbx mytags list' to see available tags"),
                ),
                output::EXIT_NOT_FOUND,
            )
        }
        Err(e) => return db_error(e),
    };

    let plan = serde_json::json!({
        "action": "delete_mytag",
        "tag": tag.to_json(),
    });

    if !execute {
        return (
            output::mutation_dry_run("mytags.delete", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();

    // Also soft-delete all associations
    let _ = sqlx::query(
        "UPDATE djmdSongMyTag SET rb_local_deleted = 1, updated_at = ? WHERE MyTagID = ? AND rb_local_deleted = 0"
    ).bind(&now).bind(tag_id).execute(pool).await;

    match sqlx::query("UPDATE djmdMyTag SET rb_local_deleted = 1, updated_at = ? WHERE ID = ?")
        .bind(&now)
        .bind(tag_id)
        .execute(pool)
        .await
    {
        Ok(_) => (
            output::mutation_done(
                "mytags.delete",
                serde_json::json!({
                    "tag": tag.to_json(),
                }),
            ),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}
