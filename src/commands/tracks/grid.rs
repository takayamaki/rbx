//! `tracks grid copy`: copies a beat grid between two tracks with the same audio,
//! e.g. an mp3 and its m4a re-encode.
//!
//! Beat grids live in the track's analysis files, not in master.db.
//! The grid is written to the target's PQTZ (.DAT), and the target's PQT2 (.EXT),
//! a second form of the grid that is not decoded yet, is removed:
//! without PQT2, rekordbox shows the PQTZ grid (checked in rekordbox 7).
//! The list BPM comes from djmdContent.BPM, so it is copied too.

use std::path::{Path, PathBuf};

use rbx::anlz::{self, Beat};
use rbx::helpers::now_datetime;
use rbx::output;
use sqlx::sqlite::SqlitePool;

use crate::cli::TrackGridAction;
use crate::commands::db_error;

pub(crate) async fn handle_track_grid(
    pool: &SqlitePool,
    db_path: &Path,
    action: TrackGridAction,
) -> (serde_json::Value, i32) {
    match action {
        TrackGridAction::Copy {
            from,
            to,
            offset_ms,
            execute,
        } => handle_grid_copy(pool, db_path, &from, &to, offset_ms, execute).await,
    }
}

/// A track's analysis files and list BPM.
struct Analysis {
    dat: PathBuf,
    ext: PathBuf,
    bpm: Option<i64>,
}

fn not_found(message: &str) -> (serde_json::Value, i32) {
    (
        output::error(
            "not_found",
            output::EXIT_NOT_FOUND,
            message,
            Some("Analyze the track in rekordbox first"),
        ),
        output::EXIT_NOT_FOUND,
    )
}

/// Analysis files are stored under <master.db dir>/share + djmdContent.AnalysisDataPath.
async fn find_analysis(
    pool: &SqlitePool,
    db_path: &Path,
    track_id: &str,
) -> Result<Analysis, (serde_json::Value, i32)> {
    let row = sqlx::query_as::<_, (Option<String>, Option<i64>)>(
        "SELECT AnalysisDataPath, BPM FROM djmdContent WHERE ID = ?",
    )
    .bind(track_id)
    .fetch_optional(pool)
    .await
    .map_err(db_error)?;
    let Some((path, bpm)) = row else {
        return Err(not_found(&format!("Track not found: {}", track_id)));
    };
    let Some(path) = path.filter(|p| !p.is_empty()) else {
        return Err(not_found(&format!(
            "Track {} has no analysis files",
            track_id
        )));
    };
    let share = db_path.parent().unwrap_or(Path::new(".")).join("share");
    let dat = share.join(path.trim_start_matches('/'));
    let ext = dat.with_extension("EXT");
    Ok(Analysis { dat, ext, bpm })
}

/// The shift (from minus to, in ms) between two grids of the same audio:
/// the median distance from each source beat to the nearest target beat with the same tempo.
fn measure_offset(from: &[Beat], to: &[Beat]) -> Option<i64> {
    let mut diffs: Vec<i64> = from
        .iter()
        .filter_map(|b| {
            let nearest = to
                .iter()
                .min_by_key(|t| (t.ms as i64 - b.ms as i64).abs())?;
            (nearest.tempo == b.tempo).then_some(b.ms as i64 - nearest.ms as i64)
        })
        .collect();
    if diffs.is_empty() {
        return None;
    }
    diffs.sort_unstable();
    Some(diffs[diffs.len() / 2])
}

async fn handle_grid_copy(
    pool: &SqlitePool,
    db_path: &Path,
    from: &str,
    to: &str,
    offset_ms: Option<i64>,
    execute: bool,
) -> (serde_json::Value, i32) {
    let source = match find_analysis(pool, db_path, from).await {
        Ok(a) => a,
        Err(e) => return e,
    };
    let target = match find_analysis(pool, db_path, to).await {
        Ok(a) => a,
        Err(e) => return e,
    };
    let read = |path: &Path| std::fs::read(path).map_err(|e| (path.to_path_buf(), e.to_string()));
    let (source_dat, target_dat, target_ext) =
        match (read(&source.dat), read(&target.dat), read(&target.ext)) {
            (Ok(a), Ok(b), Ok(c)) => (a, b, c),
            (Err((p, e)), _, _) | (_, Err((p, e)), _) | (_, _, Err((p, e))) => {
                return not_found(&format!("Cannot read {}: {}", p.display(), e))
            }
        };
    let (source_beats, target_beats) =
        match (anlz::read_beats(&source_dat), anlz::read_beats(&target_dat)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => {
                return (
                    output::error("general", output::EXIT_GENERAL, &e.0, None),
                    output::EXIT_GENERAL,
                )
            }
        };

    let offset = offset_ms.or_else(|| measure_offset(&source_beats, &target_beats));
    let Some(offset) = offset else {
        return (
            output::error(
                "usage",
                output::EXIT_USAGE,
                "The two grids share no tempo, so the offset cannot be measured",
                Some("Pass --offset-ms (from minus to, e.g. 7 when the target is 7 ms earlier)"),
            ),
            output::EXIT_USAGE,
        );
    };
    let shifted: Vec<Beat> = source_beats
        .iter()
        .map(|b| Beat {
            ms: (b.ms as i64 - offset).max(0) as u32,
            ..*b
        })
        .collect();

    let plan = serde_json::json!({
        "action": "copy_grid",
        "from": { "id": from, "beats": source_beats.len(), "bpm": source.bpm.map(|v| v as f64 / 100.0) },
        "to": { "id": to, "beats": target_beats.len(), "bpm": target.bpm.map(|v| v as f64 / 100.0) },
        "offset_ms": offset,
        "files": [target.dat.display().to_string(), target.ext.display().to_string()],
    });
    if !execute {
        return (
            output::mutation_dry_run("tracks.grid.copy", plan, "Add --execute to apply"),
            output::EXIT_OK,
        );
    }

    let new_dat = anlz::replace_beats(&target_dat, &shifted);
    let new_ext = anlz::remove_sections(&target_ext, b"PQT2");
    let (new_dat, new_ext) = match (new_dat, new_ext) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            return (
                output::error("general", output::EXIT_GENERAL, &e.0, None),
                output::EXIT_GENERAL,
            )
        }
    };
    // Keep the files as rekordbox wrote them; a later copy does not replace the first backup
    let mut backups = Vec::new();
    for (path, original) in [(&target.dat, &target_dat), (&target.ext, &target_ext)] {
        let backup = backup_path(path);
        if !backup.exists() {
            if let Err(e) = std::fs::write(&backup, original) {
                return (
                    output::error(
                        "general",
                        output::EXIT_GENERAL,
                        &format!("Cannot write backup {}: {}", backup.display(), e),
                        None,
                    ),
                    output::EXIT_GENERAL,
                );
            }
        }
        backups.push(backup.display().to_string());
    }
    for (path, bytes) in [(&target.dat, &new_dat), (&target.ext, &new_ext)] {
        if let Err(e) = std::fs::write(path, bytes) {
            return (
                output::error(
                    "general",
                    output::EXIT_GENERAL,
                    &format!("Cannot write {}: {}", path.display(), e),
                    None,
                ),
                output::EXIT_GENERAL,
            );
        }
    }
    if let Err(e) = sqlx::query("UPDATE djmdContent SET BPM = ?, updated_at = ? WHERE ID = ?")
        .bind(source.bpm)
        .bind(now_datetime())
        .bind(to)
        .execute(pool)
        .await
    {
        return db_error(e);
    }

    let mut result = plan;
    result["backups"] = backups.into();
    (
        output::mutation_done("tracks.grid.copy", result),
        output::EXIT_OK,
    )
}

/// ANLZ0000.DAT -> ANLZ0000.DAT.rbx-backup, in the same folder.
fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".rbx-backup");
    PathBuf::from(name)
}
