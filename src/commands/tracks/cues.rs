use rbx::helpers::{generate_numeric_id, now_datetime};
use rbx::output;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

use crate::cli::TrackCuesAction;
use crate::commands::{db_error, resolve_track_summary};
use crate::rows::CueRow;

use super::content_cue;

/// rekordbox stores cue positions in frames of 1/150 s, rounded down.
fn msec_to_frame(msec: i64) -> i64 {
    msec * 150 / 1000
}

/// Refuses to place a cue on a file type whose extra position fields rbx cannot compute:
/// VBR mp3 needs InMpegFrame / InMpegAbs, FLAC needs InPointSeekInfo.
async fn check_position_format(
    pool: &SqlitePool,
    track_id: &str,
) -> Result<Option<(serde_json::Value, i32)>, sqlx::Error> {
    let (file_type,) =
        sqlx::query_as::<_, (Option<i64>,)>("SELECT FileType FROM djmdContent WHERE ID = ?")
            .bind(track_id)
            .fetch_one(pool)
            .await?;
    let format = match file_type {
        Some(1) => "mp3",
        Some(5) => "FLAC",
        _ => return Ok(None),
    };
    Ok(Some((
        output::error(
            "usage",
            output::EXIT_USAGE,
            &format!(
                "Cannot place cues on {} files yet: rekordbox also stores a position in the file \
                 (MPEG frame offsets for VBR mp3, seek info for FLAC) that rbx does not compute",
                format
            ),
            Some("Set this cue in rekordbox"),
        ),
        output::EXIT_USAGE,
    )))
}

/// rekordbox allows at most this many memory cues on one track.
const MAX_MEMORY_CUES: i64 = 10;
/// Color of a cue that never had one.
const NO_COLOR: i32 = -1;
/// Color rekordbox writes when the user clears a memory cue's color.
const CLEARED_COLOR: i32 = 255;
/// ColorTableIndex rekordbox writes when the user resets a hot cue's color.
const RESET_HOT_CUE_COLOR: i32 = 0;
pub(crate) const MEMORY_COLORS: [&str; 8] = [
    "pink", "red", "orange", "yellow", "green", "aqua", "blue", "purple",
];
const COLOR_HINT: &str = "use pink, red, orange, yellow, green, aqua, blue or purple";

/// ColorTableIndex of the 16 colors in rekordbox's hot cue color menu,
/// left to right and top to bottom.
pub(crate) const HOT_CUE_COLORS: [i32; 16] =
    [49, 56, 60, 62, 1, 5, 9, 14, 18, 22, 26, 30, 32, 38, 42, 45];
/// Names for HOT_CUE_COLORS, picked from the colors seen in the menu.
/// Colors close to a memory cue color share its name (red, blue, ...).
pub(crate) const HOT_CUE_COLOR_NAMES: [&str; 16] = [
    "violet",
    "purple",
    "lavender",
    "slateblue",
    "blue",
    "sky",
    "aqua",
    "teal",
    "emerald",
    "green",
    "lime",
    "olive",
    "yellow",
    "orange",
    "red",
    "pink",
];
const HOT_COLOR_HINT: &str = "use violet, purple, lavender, slateblue, blue, sky, aqua, teal, \
     emerald, green, lime, olive, yellow, orange, red, pink, \
     or 1-16 for the position in rekordbox's hot cue color menu";

/// Hot cue colors are given by name, or by their position (1-16) in rekordbox's color menu.
fn hot_cue_color(name: &str) -> Option<i32> {
    let position = match HOT_CUE_COLOR_NAMES.iter().position(|c| *c == name) {
        Some(i) => i,
        None => name.parse::<usize>().ok()?.checked_sub(1)?,
    };
    HOT_CUE_COLORS.get(position).copied()
}

/// Memory cue colors are Color 0-7 in rekordbox's menu order.
fn memory_color(name: &str) -> Option<i32> {
    MEMORY_COLORS
        .iter()
        .position(|c| *c == name)
        .map(|i| i as i32)
}

fn usage_error(message: &str) -> (serde_json::Value, i32) {
    (
        output::error("usage", output::EXIT_USAGE, message, None),
        output::EXIT_USAGE,
    )
}

/// Hot cue slots A-C are Kind 1-3 and D-H are Kind 5-9: rekordbox skips Kind 4.
pub(crate) fn slot_to_kind(slot: i32) -> i32 {
    if slot <= 3 {
        slot
    } else {
        slot + 1
    }
}

/// The reverse of `slot_to_kind`.
pub(crate) fn kind_to_slot(kind: i32) -> i32 {
    if kind <= 3 {
        kind
    } else {
        kind - 1
    }
}

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
                 OutMsec as out_msec, Kind as kind, Color as color, ColorTableIndex as color_table_index, Comment as comment \
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
            color,
            execute,
        } => {
            let cue = NewCue {
                msec,
                kind,
                slot,
                comment,
                color,
            };
            handle_track_cue_add(pool, &track_id, cue, execute).await
        }
        TrackCuesAction::Update {
            cue_id,
            msec,
            comment,
            color,
            execute,
        } => handle_track_cue_update(pool, &cue_id, msec, comment, color, execute).await,
        TrackCuesAction::Delete { cue_id, execute } => {
            handle_track_cue_delete(pool, &cue_id, execute).await
        }
    }
}

/// The flags of `cues add`.
struct NewCue {
    msec: i64,
    kind: String,
    slot: Option<i32>,
    comment: Option<String>,
    color: Option<String>,
}

async fn handle_track_cue_add(
    pool: &SqlitePool,
    track_id: &str,
    cue: NewCue,
    execute: bool,
) -> (serde_json::Value, i32) {
    let NewCue {
        msec,
        kind,
        slot,
        comment,
        color,
    } = cue;
    let (kind, color) = (kind.as_str(), color.as_deref());
    // Memory cues keep their color in Color, hot cues in ColorTableIndex
    let (color_value, color_table_index) = match color {
        None => (NO_COLOR, None),
        Some(name) if kind != "memory" => match hot_cue_color(name) {
            Some(v) => (NO_COLOR, Some(v)),
            None => {
                return usage_error(&format!(
                    "Unknown hot cue color: {} ({})",
                    name, HOT_COLOR_HINT
                ))
            }
        },
        Some(name) => match memory_color(name) {
            Some(v) => (v, None),
            None => return usage_error(&format!("Unknown color: {} ({})", name, COLOR_HINT)),
        },
    };
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

    match check_position_format(pool, track_id).await {
        Ok(Some(refused)) => return refused,
        Ok(None) => {}
        Err(e) => return db_error(e),
    }

    let kind_int = match kind {
        "memory" => 0,
        "hot" => match slot {
            Some(s) if (1..=8).contains(&s) => slot_to_kind(s),
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
                        slot.unwrap_or_default(),
                        title
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

    if kind_int == 0 {
        let count = sqlx::query_as::<_, (i64,)>(
            "SELECT COUNT(*) FROM djmdCue WHERE ContentID = ? AND Kind = 0 AND rb_local_deleted = 0",
        )
        .bind(track_id)
        .fetch_one(pool)
        .await;
        match count {
            Ok((n,)) if n >= MAX_MEMORY_CUES => {
                return (
                    output::error(
                        "conflict",
                        output::EXIT_CONFLICT,
                        &format!(
                            "'{}' already has {} memory cues, the most rekordbox allows",
                            title, n
                        ),
                        Some(&format!(
                            "Use 'rbx tracks cues list {}' and delete one first",
                            track_id
                        )),
                    ),
                    output::EXIT_CONFLICT,
                )
            }
            Ok(_) => {}
            Err(e) => return db_error(e),
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
    sqlx::query("BEGIN").execute(pool).await.ok();
    let written = sqlx::query(
        "INSERT INTO djmdCue (ID, ContentID, InMsec, InFrame, InMpegFrame, InMpegAbs, \
         OutMsec, OutFrame, OutMpegFrame, OutMpegAbs, Kind, Color, ColorTableIndex, \
         ActiveLoop, Comment, BeatLoopSize, CueMicrosec, \
         ContentUUID, UUID, rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, \
         created_at, updated_at) \
         VALUES (?, ?, ?, ?, 0, 0, -1, 0, 0, 0, ?, ?, ?, NULL, ?, NULL, NULL, ?, ?, 0, 0, 0, 0, ?, ?)"
    )
    .bind(&new_id).bind(track_id).bind(msec).bind(msec_to_frame(msec))
    .bind(kind_int).bind(color_value).bind(color_table_index).bind(comment.as_deref().filter(|c| !c.is_empty()))
    .bind(&content_uuid).bind(&new_uuid)
    .bind(&now).bind(&now)
    .execute(pool).await;
    let written = match written {
        Ok(_) => content_cue::sync(pool, track_id, &[new_id.as_str()], &now).await,
        Err(e) => Err(e),
    };
    match written {
        Ok(_) => {
            sqlx::query("COMMIT").execute(pool).await.ok();
            (
                output::mutation_done(
                    "tracks.cues.add",
                    serde_json::json!({
                        "cue_id": new_id,
                        "track": { "id": track_id, "title": title, "artist": artist },
                        "kind": kind,
                        "slot": slot,
                        "in_msec": msec,
                        "comment": comment,
                    }),
                ),
                output::EXIT_OK,
            )
        }
        Err(e) => {
            sqlx::query("ROLLBACK").execute(pool).await.ok();
            db_error(e)
        }
    }
}

async fn handle_track_cue_update(
    pool: &SqlitePool,
    cue_id: &str,
    msec: Option<i64>,
    comment: Option<String>,
    color: Option<String>,
    execute: bool,
) -> (serde_json::Value, i32) {
    let cue = match sqlx::query_as::<_, CueRow>(
        "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
         OutMsec as out_msec, Kind as kind, Color as color, ColorTableIndex as color_table_index, Comment as comment \
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

    if msec.is_none() && comment.is_none() && color.is_none() {
        return (
            output::error(
                "usage",
                output::EXIT_USAGE,
                "No fields specified to update",
                Some("Use --msec, --comment or --color"),
            ),
            output::EXIT_USAGE,
        );
    }
    if msec.is_some() {
        match check_position_format(pool, cue.content_id()).await {
            Ok(Some(refused)) => return refused,
            Ok(None) => {}
            Err(e) => return db_error(e),
        }
    }
    let color_change = match color.as_deref() {
        None => None,
        Some("none") if !cue.is_memory() => Some(ColorChange::HotCue(RESET_HOT_CUE_COLOR)),
        Some(name) if !cue.is_memory() => match hot_cue_color(name) {
            Some(v) => Some(ColorChange::HotCue(v)),
            None => {
                return usage_error(&format!(
                    "Unknown hot cue color: {} ({})",
                    name, HOT_COLOR_HINT
                ))
            }
        },
        Some("none") => Some(ColorChange::MemoryCue(CLEARED_COLOR)),
        Some(name) => match memory_color(name) {
            Some(v) => Some(ColorChange::MemoryCue(v)),
            None => {
                return usage_error(&format!(
                    "Unknown color: {} ({}, or none)",
                    name, COLOR_HINT
                ))
            }
        },
    };

    let mut changes = serde_json::Map::new();
    if let Some(v) = msec {
        changes.insert("in_msec".into(), serde_json::json!(v));
    }
    if let Some(ref v) = comment {
        changes.insert("comment".into(), serde_json::json!(v));
    }
    if let Some(ref v) = color {
        changes.insert("color".into(), serde_json::json!(v));
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
    sqlx::query("BEGIN").execute(pool).await.ok();
    let written = update_cue_row(pool, cue_id, msec, comment.as_deref(), color_change, &now).await;
    let written = match written {
        Ok(_) => content_cue::sync(pool, cue.content_id(), &[cue_id], &now).await,
        Err(e) => Err(e),
    };
    if let Err(e) = written {
        sqlx::query("ROLLBACK").execute(pool).await.ok();
        return db_error(e);
    }
    sqlx::query("COMMIT").execute(pool).await.ok();

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

/// A new color: memory cues keep it in Color, hot cues in ColorTableIndex.
enum ColorChange {
    MemoryCue(i32),
    HotCue(i32),
}

/// Changes the cue row in place, as rekordbox does (same ID).
async fn update_cue_row(
    pool: &SqlitePool,
    cue_id: &str,
    msec: Option<i64>,
    comment: Option<&str>,
    color: Option<ColorChange>,
    now: &str,
) -> Result<(), sqlx::Error> {
    let color_sql = match color {
        Some(ColorChange::MemoryCue(v)) => Some(("UPDATE djmdCue SET Color = ? WHERE ID = ?", v)),
        Some(ColorChange::HotCue(v)) => {
            Some(("UPDATE djmdCue SET ColorTableIndex = ? WHERE ID = ?", v))
        }
        None => None,
    };
    if let Some((sql, v)) = color_sql {
        sqlx::query(sql).bind(v).bind(cue_id).execute(pool).await?;
    }
    if let Some(v) = msec {
        sqlx::query("UPDATE djmdCue SET InMsec = ?, InFrame = ? WHERE ID = ?")
            .bind(v)
            .bind(msec_to_frame(v))
            .bind(cue_id)
            .execute(pool)
            .await?;
    }
    if let Some(v) = comment {
        sqlx::query("UPDATE djmdCue SET Comment = ? WHERE ID = ?")
            .bind(Some(v).filter(|c| !c.is_empty()))
            .bind(cue_id)
            .execute(pool)
            .await?;
    }
    sqlx::query("UPDATE djmdCue SET updated_at = ? WHERE ID = ?")
        .bind(now)
        .bind(cue_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn handle_track_cue_delete(
    pool: &SqlitePool,
    cue_id: &str,
    execute: bool,
) -> (serde_json::Value, i32) {
    let cue = match sqlx::query_as::<_, CueRow>(
        "SELECT ID as id, ContentID as content_id, InMsec as in_msec, \
         OutMsec as out_msec, Kind as kind, Color as color, ColorTableIndex as color_table_index, Comment as comment \
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

    // rekordbox removes cue rows; a real master.db has no soft-deleted cues
    let now = now_datetime();
    sqlx::query("BEGIN").execute(pool).await.ok();
    let written = sqlx::query("DELETE FROM djmdCue WHERE ID = ?")
        .bind(cue_id)
        .execute(pool)
        .await;
    let written = match written {
        Ok(_) => content_cue::sync(pool, cue.content_id(), &[cue_id], &now).await,
        Err(e) => Err(e),
    };
    match written {
        Ok(_) => {
            sqlx::query("COMMIT").execute(pool).await.ok();
            (
                output::mutation_done(
                    "tracks.cues.delete",
                    serde_json::json!({
                        "cue": cue.to_json(),
                    }),
                ),
                output::EXIT_OK,
            )
        }
        Err(e) => {
            sqlx::query("ROLLBACK").execute(pool).await.ok();
            db_error(e)
        }
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
                    "Hot cue slot 1-8 (A-H), required for hot cues",
                ),
                flag("--comment", "string", false, "Cue comment/name"),
                flag(
                    "--color",
                    "string",
                    false,
                    "Memory cue: pink, red, orange, yellow, green, aqua, blue or purple. Hot cue: violet, purple, lavender, slateblue, blue, sky, aqua, teal, emerald, green, lime, olive, yellow, orange, red, pink, or 1-16 (position in rekordbox's hot cue color menu, left to right, top to bottom)",
                ),
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
                "rbx tracks cues add TRACK_ID 12345 --color green",
                "rbx tracks cues add TRACK_ID 12345 --kind hot --slot 1 --color red --comment 'Drop' --execute",
            ],
        ),
        "cues update" => describe_command(
            "tracks cues update",
            &[
                flag("cue_id", "string", true, "Cue ID"),
                flag("--msec", "integer", false, "New position in milliseconds"),
                flag("--comment", "string", false, "New comment"),
                flag(
                    "--color",
                    "string",
                    false,
                    "Memory cue: pink, red, orange, yellow, green, aqua, blue, purple, or none. Hot cue: a hot cue color name, 1-16, or none",
                ),
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
                "rbx tracks cues update CUE_ID --color none --execute",
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
            "slot": { "type": "integer", "description": "Hot cue slot 1-8 (A-H). Only on hot cues" },
            "in_msec": { "type": "integer|null", "description": "Cue position in milliseconds" },
            "out_msec": { "type": "integer|null", "description": "Loop end in milliseconds, null if not a loop" },
            "color": {
                "type": "string|null",
                "description": "Color name (memory cue: pink, red, orange, yellow, green, aqua, blue, purple; hot cue: violet, purple, lavender, slateblue, blue, sky, aqua, teal, emerald, green, lime, olive, yellow, orange, red, pink). null for no color",
            },
            "comment": { "type": "string|null" },
        },
    })
}
