//! contentCue: rekordbox keeps every cue of a track twice.
//! Once as djmdCue rows, and once as a JSON array in the track's contentCue row.
//! After any change to djmdCue, `sync` rebuilds that array from djmdCue.
//!
//! Entries for cues that did not change are kept byte for byte,
//! so rbx does not reorder the fields rekordbox wrote.
//! A changed or new cue gets a fresh entry with the fields in rekordbox's order,
//! with NULL fields and an empty comment left out.

use rbx::helpers::allocate_usns;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sqlx::sqlite::SqlitePool;
use uuid::Uuid;

/// One djmdCue row, serialized as a contentCue entry.
#[derive(sqlx::FromRow, Serialize)]
struct CueEntry {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "ContentID")]
    content_id: String,
    #[serde(rename = "ContentUUID", skip_serializing_if = "Option::is_none")]
    content_uuid: Option<String>,
    #[serde(rename = "InMsec", skip_serializing_if = "Option::is_none")]
    in_msec: Option<i64>,
    #[serde(rename = "InFrame", skip_serializing_if = "Option::is_none")]
    in_frame: Option<i64>,
    #[serde(rename = "InMpegFrame", skip_serializing_if = "Option::is_none")]
    in_mpeg_frame: Option<i64>,
    #[serde(rename = "InMpegAbs", skip_serializing_if = "Option::is_none")]
    in_mpeg_abs: Option<i64>,
    #[serde(rename = "InPointSeekInfo", skip_serializing_if = "Option::is_none")]
    in_point_seek_info: Option<String>,
    #[serde(rename = "OutMsec", skip_serializing_if = "Option::is_none")]
    out_msec: Option<i64>,
    #[serde(rename = "OutFrame", skip_serializing_if = "Option::is_none")]
    out_frame: Option<i64>,
    #[serde(rename = "OutMpegFrame", skip_serializing_if = "Option::is_none")]
    out_mpeg_frame: Option<i64>,
    #[serde(rename = "OutMpegAbs", skip_serializing_if = "Option::is_none")]
    out_mpeg_abs: Option<i64>,
    #[serde(rename = "OutPointSeekInfo", skip_serializing_if = "Option::is_none")]
    out_point_seek_info: Option<String>,
    #[serde(rename = "Kind", skip_serializing_if = "Option::is_none")]
    kind: Option<i64>,
    #[serde(rename = "Color", skip_serializing_if = "Option::is_none")]
    color: Option<i64>,
    #[serde(rename = "ColorTableIndex", skip_serializing_if = "Option::is_none")]
    color_table_index: Option<i64>,
    #[serde(rename = "ActiveLoop", skip_serializing_if = "Option::is_none")]
    active_loop: Option<i64>,
    #[serde(rename = "Comment", skip_serializing_if = "is_empty_comment")]
    comment: Option<String>,
    #[serde(rename = "BeatLoopSize", skip_serializing_if = "Option::is_none")]
    beat_loop_size: Option<i64>,
    #[serde(rename = "CueMicrosec", skip_serializing_if = "Option::is_none")]
    cue_microsec: Option<i64>,
    #[serde(rename = "UUID")]
    uuid: String,
    created_at: String,
    updated_at: String,
}

fn is_empty_comment(c: &Option<String>) -> bool {
    c.as_deref().is_none_or(str::is_empty)
}

/// djmdCue timestamps are "2026-09-30 13:43:30.853 +00:00";
/// contentCue entries use "2026-09-30T13:43:30.853+00:00".
fn to_iso(ts: &str) -> String {
    ts.replacen(' ', "T", 1).replace(" +", "+")
}

const CUE_ENTRY_QUERY: &str = "SELECT ID as id, ContentID as content_id, ContentUUID as content_uuid, \
     InMsec as in_msec, InFrame as in_frame, InMpegFrame as in_mpeg_frame, InMpegAbs as in_mpeg_abs, \
     InPointSeekInfo as in_point_seek_info, OutMsec as out_msec, OutFrame as out_frame, \
     OutMpegFrame as out_mpeg_frame, OutMpegAbs as out_mpeg_abs, OutPointSeekInfo as out_point_seek_info, \
     Kind as kind, Color as color, ColorTableIndex as color_table_index, ActiveLoop as active_loop, \
     Comment as comment, BeatLoopSize as beat_loop_size, CueMicrosec as cue_microsec, \
     UUID as uuid, created_at, updated_at \
     FROM djmdCue WHERE ContentID = ? AND rb_local_deleted = 0 ORDER BY created_at, ID";

#[derive(Deserialize)]
struct EntryId {
    #[serde(rename = "ID")]
    id: String,
}

fn decode_error(e: serde_json::Error) -> sqlx::Error {
    sqlx::Error::Decode(Box::new(e))
}

/// Rebuilds the contentCue row of `track_id` from its djmdCue rows.
/// `changed` lists the cue IDs whose entries must be written again.
/// Call it inside the same transaction as the djmdCue change.
pub(crate) async fn sync(
    pool: &SqlitePool,
    track_id: &str,
    changed: &[&str],
    now: &str,
) -> Result<(), sqlx::Error> {
    let mut cues = sqlx::query_as::<_, CueEntry>(CUE_ENTRY_QUERY)
        .bind(track_id)
        .fetch_all(pool)
        .await?;
    for cue in &mut cues {
        cue.created_at = to_iso(&cue.created_at);
        cue.updated_at = to_iso(&cue.updated_at);
    }
    let existing =
        sqlx::query_as::<_, (String,)>("SELECT Cues FROM contentCue WHERE ContentID = ?")
            .bind(track_id)
            .fetch_optional(pool)
            .await?;

    let fresh = |cue: &CueEntry| serde_json::to_string(cue).map_err(decode_error);

    let mut entries: Vec<String> = Vec::new();
    let mut written: Vec<&str> = Vec::new();
    if let Some((text,)) = &existing {
        let raws: Vec<Box<RawValue>> = serde_json::from_str(text).map_err(decode_error)?;
        for raw in raws {
            let id = serde_json::from_str::<EntryId>(raw.get())
                .map_err(decode_error)?
                .id;
            let Some(cue) = cues.iter().find(|c| c.id == id) else {
                continue;
            };
            if changed.contains(&cue.id.as_str()) {
                entries.push(fresh(cue)?);
            } else {
                entries.push(raw.get().to_string());
            }
            written.push(cue.id.as_str());
        }
    }
    for cue in &cues {
        if !written.contains(&cue.id.as_str()) {
            entries.push(fresh(cue)?);
        }
    }

    if entries.is_empty() {
        sqlx::query("DELETE FROM contentCue WHERE ContentID = ?")
            .bind(track_id)
            .execute(pool)
            .await?;
        return Ok(());
    }

    let cues_json = format!("[{}]", entries.join(","));
    let usn = allocate_usns(pool, 1).await?;
    if existing.is_some() {
        sqlx::query(
            "UPDATE contentCue SET Cues = ?, rb_cue_count = ?, rb_local_usn = ?, updated_at = ? \
             WHERE ContentID = ?",
        )
        .bind(&cues_json)
        .bind(entries.len() as i64)
        .bind(usn)
        .bind(now)
        .bind(track_id)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "INSERT INTO contentCue (ID, ContentID, Cues, rb_cue_count, UUID, \
             rb_data_status, rb_local_data_status, rb_local_deleted, rb_local_synced, rb_local_usn, \
             created_at, updated_at) \
             VALUES ((SELECT UUID FROM djmdContent WHERE ID = ?), ?, ?, ?, ?, 0, 0, 0, 0, ?, ?, ?)",
        )
        .bind(track_id)
        .bind(track_id)
        .bind(&cues_json)
        .bind(entries.len() as i64)
        .bind(Uuid::new_v4().to_string())
        .bind(usn)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}
