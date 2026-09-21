use rbx::helpers::{allocate_usns, generate_numeric_id, now_datetime};
use rbx::output;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::cli::TrackCuesAction;
use crate::commands::{db_error, resolve_track_summary};
use crate::rows::CueRow;

pub(crate) async fn handle_track_cues(
    pool: &SqlitePool,
    action: TrackCuesAction,
) -> (serde_json::Value, i32) {
    match action {
        TrackCuesAction::List { track_id } => {
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
            match sqlx::query_as::<_, CueRow>(
                "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
                 OutMsec as out_msec, Kind as kind, Color as color, Comment as comment \
                 FROM djmdCue WHERE ContentID = ? AND rb_local_deleted = 0 \
                 ORDER BY Kind, InMsec",
            )
            .bind(&track_id)
            .fetch_all(pool)
            .await
            {
                Ok(rows) => {
                    let items: Vec<_> = rows.iter().map(|r| r.to_json()).collect();
                    (
                        output::success("track_cues", serde_json::Value::Array(items)),
                        output::EXIT_OK,
                    )
                }
                Err(e) => db_error(e),
            }
        }
        TrackCuesAction::Add {
            track_id,
            msec,
            kind,
            slot,
            comment,
            execute,
        } => handle_track_cue_add(pool, &track_id, msec, &kind, slot, comment, execute).await,
        TrackCuesAction::Update {
            cue_id,
            msec,
            comment,
            execute,
        } => handle_track_cue_update(pool, &cue_id, msec, comment, execute).await,
        TrackCuesAction::Delete { cue_id, execute } => {
            handle_track_cue_delete(pool, &cue_id, execute).await
        }
    }
}

async fn handle_track_cue_add(
    pool: &SqlitePool,
    track_id: &str,
    msec: i64,
    kind: &str,
    slot: Option<i32>,
    comment: Option<String>,
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

    let kind_int = match kind {
        "memory" => 0,
        "hot" => match slot {
            Some(s) if (1..=8).contains(&s) => s,
            Some(s) => {
                return (
                    output::error(
                        "validation",
                        output::EXIT_CONFLICT,
                        &format!("Hot cue slot must be 1-8, got: {}", s),
                        None,
                    ),
                    output::EXIT_CONFLICT,
                )
            }
            None => {
                return (
                    output::error(
                        "validation",
                        output::EXIT_CONFLICT,
                        "Hot cue requires --slot (1-8)",
                        None,
                    ),
                    output::EXIT_CONFLICT,
                )
            }
        },
        _ => {
            return (
                output::error(
                    "validation",
                    output::EXIT_CONFLICT,
                    &format!("Unknown cue kind: {} (use 'memory' or 'hot')", kind),
                    None,
                ),
                output::EXIT_CONFLICT,
            )
        }
    };

    // Check for slot conflict on hot cues
    if kind_int >= 1 {
        let existing = sqlx::query_as::<_, (String,)>(
            "SELECT ID FROM djmdCue WHERE ContentID = ? AND Kind = ? AND rb_local_deleted = 0",
        )
        .bind(track_id)
        .bind(kind_int)
        .fetch_optional(pool)
        .await;
        if let Ok(Some(_)) = existing {
            return (
                output::error(
                    "conflict",
                    output::EXIT_CONFLICT,
                    &format!(
                        "Hot cue slot {} is already occupied on '{}'",
                        kind_int, title
                    ),
                    Some(&format!(
                        "Use 'rbx tracks cues list {}' to see existing cues",
                        track_id
                    )),
                ),
                output::EXIT_CONFLICT,
            );
        }
    }

    let plan = serde_json::json!({
        "action": "add_cue",
        "track": { "id": track_id, "title": title, "artist": artist },
        "kind": kind,
        "slot": slot,
        "in_msec": msec,
        "comment": comment,
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.cues.add", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let new_id = match generate_numeric_id(pool, "djmdCue").await {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    let new_uuid = Uuid::new_v4().to_string();
    let now = now_datetime();
    let usn = match allocate_usns(pool, 1).await {
        Ok(v) => v,
        Err(e) => return db_error(e),
    };
    let content_uuid: String = match sqlx::query_as::<_, (String,)>(
        "SELECT COALESCE(UUID, '') FROM djmdContent WHERE ID = ?",
    )
    .bind(track_id)
    .fetch_one(pool)
    .await
    {
        Ok((u,)) => u,
        Err(e) => return db_error(e),
    };
    match sqlx::query(
        "INSERT INTO djmdCue (ID, ContentID, InMsec, InFrame, InMpegFrame, InMpegAbs, \
         OutMsec, OutFrame, OutMpegFrame, OutMpegAbs, Kind, Color, ColorTableIndex, \
         ActiveLoop, Comment, BeatLoopSize, CueMicrosec, \
         ContentUUID, UUID, rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
         created_at, updated_at) \
         VALUES (?, ?, ?, 0, 0, 0, -1, 0, 0, 0, ?, -1, 0, 0, ?, 0, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)"
    )
    .bind(&new_id).bind(track_id).bind(msec)
    .bind(kind_int).bind(comment.as_deref().unwrap_or(""))
    .bind(msec * 1000) // CueMicrosec = msec * 1000
    .bind(&content_uuid).bind(&new_uuid).bind(usn)
    .bind(&now).bind(&now)
    .execute(pool).await {
        Ok(_) => (
            output::mutation_done("tracks.cues.add", serde_json::json!({
                "cue_id": new_id,
                "track": { "id": track_id, "title": title, "artist": artist },
                "kind": kind,
                "slot": slot,
                "in_msec": msec,
                "comment": comment,
            })),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}

async fn handle_track_cue_update(
    pool: &SqlitePool,
    cue_id: &str,
    msec: Option<i64>,
    comment: Option<String>,
    execute: bool,
) -> (serde_json::Value, i32) {
    let cue = match sqlx::query_as::<_, CueRow>(
        "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
         OutMsec as out_msec, Kind as kind, Color as color, Comment as comment \
         FROM djmdCue WHERE ID = ? AND rb_local_deleted = 0",
    )
    .bind(cue_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("Cue not found: {}", cue_id),
                    None,
                ),
                output::EXIT_NOT_FOUND,
            )
        }
        Err(e) => return db_error(e),
    };

    if msec.is_none() && comment.is_none() {
        return (
            output::error(
                "usage",
                output::EXIT_USAGE,
                "No fields specified to update",
                Some("Use --msec or --comment"),
            ),
            output::EXIT_USAGE,
        );
    }

    let mut changes = serde_json::Map::new();
    if let Some(v) = msec {
        changes.insert("in_msec".into(), serde_json::json!(v));
    }
    if let Some(ref v) = comment {
        changes.insert("comment".into(), serde_json::json!(v));
    }

    let plan = serde_json::json!({
        "action": "update_cue",
        "cue": cue.to_json(),
        "changes": serde_json::Value::Object(changes.clone()),
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.cues.update", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();
    if let Some(v) = msec {
        let _ = sqlx::query(
            "UPDATE djmdCue SET InMsec = ?, CueMicrosec = ?, updated_at = ? WHERE ID = ?",
        )
        .bind(v)
        .bind(v * 1000)
        .bind(&now)
        .bind(cue_id)
        .execute(pool)
        .await;
    }
    if let Some(ref v) = comment {
        let _ = sqlx::query("UPDATE djmdCue SET Comment = ?, updated_at = ? WHERE ID = ?")
            .bind(v)
            .bind(&now)
            .bind(cue_id)
            .execute(pool)
            .await;
    }

    (
        output::mutation_done(
            "tracks.cues.update",
            serde_json::json!({
                "cue": cue.to_json(),
                "changes": serde_json::Value::Object(changes),
            }),
        ),
        output::EXIT_OK,
    )
}

async fn handle_track_cue_delete(
    pool: &SqlitePool,
    cue_id: &str,
    execute: bool,
) -> (serde_json::Value, i32) {
    let cue = match sqlx::query_as::<_, CueRow>(
        "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
         OutMsec as out_msec, Kind as kind, Color as color, Comment as comment \
         FROM djmdCue WHERE ID = ? AND rb_local_deleted = 0",
    )
    .bind(cue_id)
    .fetch_optional(pool)
    .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                output::error(
                    "not_found",
                    output::EXIT_NOT_FOUND,
                    &format!("Cue not found: {}", cue_id),
                    None,
                ),
                output::EXIT_NOT_FOUND,
            )
        }
        Err(e) => return db_error(e),
    };

    let plan = serde_json::json!({
        "action": "delete_cue",
        "cue": cue.to_json(),
    });

    if !execute {
        return (
            output::mutation_dry_run("tracks.cues.delete", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let now = now_datetime();
    match sqlx::query("UPDATE djmdCue SET rb_local_deleted = 1, updated_at = ? WHERE ID = ?")
        .bind(&now)
        .bind(cue_id)
        .execute(pool)
        .await
    {
        Ok(_) => (
            output::mutation_done(
                "tracks.cues.delete",
                serde_json::json!({
                    "cue": cue.to_json(),
                }),
            ),
            output::EXIT_OK,
        ),
        Err(e) => db_error(e),
    }
}

// --- describe ---

use crate::describe::{describe_command, flag, mutation_result_schema};

pub(crate) fn describe(action: &str) -> Option<serde_json::Value> {
    Some(match action {
        "cues list" => describe_command(
            "tracks cues list",
            &[flag("track_id", "string", true, "Track ID")],
            &serde_json::json!({
                "type": "array", "items": cue_schema(),
            }),
            &["rbx tracks cues list TRACK_ID"],
        ),
        "cues add" => describe_command(
            "tracks cues add",
            &[
                flag("track_id", "string", true, "Track ID"),
                flag("msec", "integer", true, "Position in milliseconds"),
                flag(
                    "--kind",
                    "string",
                    false,
                    "Cue type: 'memory' (default) or 'hot'",
                ),
                flag(
                    "--slot",
                    "integer",
                    false,
                    "Hot cue slot (1-8, required for hot cues)",
                ),
                flag("--comment", "string", false, "Cue comment/name"),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually apply the change (default: dry-run)",
                ),
            ],
            &mutation_result_schema("tracks.cues.add"),
            &[
                "rbx tracks cues add TRACK_ID 12345",
                "rbx tracks cues add TRACK_ID 12345 --kind hot --slot 1 --comment 'Drop' --execute",
            ],
        ),
        "cues update" => describe_command(
            "tracks cues update",
            &[
                flag("cue_id", "string", true, "Cue ID"),
                flag("--msec", "integer", false, "New position in milliseconds"),
                flag("--comment", "string", false, "New comment"),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually apply the change (default: dry-run)",
                ),
            ],
            &mutation_result_schema("tracks.cues.update"),
            &[
                "rbx tracks cues update CUE_ID --msec 15000 --comment 'Verse'",
                "rbx tracks cues update CUE_ID --comment 'Chorus' --execute",
            ],
        ),
        "cues delete" => describe_command(
            "tracks cues delete",
            &[
                flag("cue_id", "string", true, "Cue ID"),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually apply the change (default: dry-run)",
                ),
            ],
            &mutation_result_schema("tracks.cues.delete"),
            &[
                "rbx tracks cues delete CUE_ID",
                "rbx tracks cues delete CUE_ID --execute",
            ],
        ),
        _ => return None,
    })
}

fn cue_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "track_id": { "type": "string" },
            "kind": { "type": "string", "enum": ["memory", "hot", "other"] },
            "slot": { "type": "integer|null", "description": "Hot cue slot (1-8), null for memory cues" },
            "in_msec": { "type": "integer|null", "description": "Cue position in milliseconds" },
            "out_msec": { "type": "integer|null", "description": "Loop end in milliseconds, null if not a loop" },
            "color": { "type": "integer|null" },
            "comment": { "type": "string|null" },
        },
    })
}
