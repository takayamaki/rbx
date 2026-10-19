//! `backup create`: a library backup in the same shape as the one rekordbox writes
//! from Preferences → Advanced → Database → Library backup.

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rbx::{backup, output};
use sqlx::sqlite::SqlitePool;
use zip::write::SimpleFileOptions;

use crate::cli::BackupAction;
use crate::commands::db_error;

/// Files next to master.db that rekordbox puts at the top of the zip, after master.db.
const LIBRARY_FILES: [&str; 5] = [
    "smartlist.xml",
    "masterPlaylists6.xml",
    "automixPlaylist6.xml",
    "networkAnalyze6.db",
    "networkRecommend.db",
];

/// Files from rekordbox's settings dir (agentRegistry SyncSettingsRootPath), in its order.
const SETTINGS_FILES: [&str; 13] = [
    "rekordbox3.settings",
    "browseSetting.xml",
    "PlaySettings.xml",
    "activations.xml",
    "SamplerSettings1.xml",
    "LightingDB/macro.db3",
    "LightingDB/user.db3",
    "MYSETTING.DAT",
    "MYSETTING2.DAT",
    "DEVSETTING.DAT",
    "DJMMYSETTING.DAT",
    "rekordbox_export_settings.json",
    "cue_personal_trend.json",
];

/// rekordbox's order for the files in one analysis or artwork folder, by name ending.
/// Analysis files are ANLZ0000.* or ANLZ0001.*. Other files follow by name.
const FOLDER_ORDER: [&str; 7] = [
    ".DAT",
    ".EXT",
    ".2EX",
    ".3EX",
    "/artwork.jpg",
    "/artwork_s.jpg",
    "/artwork_m.jpg",
];

/// One file in the zip: its name there and where its bytes come from.
struct Entry {
    name: String,
    source: Source,
}

enum Source {
    File(PathBuf),
    Bytes(Vec<u8>),
}

/// The zip's entries in rekordbox's order, and what was not found on disk.
#[derive(Default)]
struct Plan {
    entries: Vec<Entry>,
    missing: Vec<String>,
}

impl Plan {
    /// Adds a file, or notes it as missing when it is not on disk.
    fn file(&mut self, name: &str, path: PathBuf) {
        if path.is_file() {
            self.entries.push(Entry {
                name: name.to_string(),
                source: Source::File(path),
            });
        } else {
            self.missing.push(name.to_string());
        }
    }

    fn summary(&self, out: &Path) -> serde_json::Value {
        serde_json::json!({
            "out": out,
            "entries": self.entries.len(),
            "missing": self.missing,
        })
    }
}

pub(crate) async fn handle_backup(
    pool: &SqlitePool,
    db_path: &Path,
    action: BackupAction,
) -> (serde_json::Value, i32) {
    match action {
        BackupAction::Create {
            out,
            like,
            settings_dir,
            execute,
        } => {
            if let Err(e) = preflight(&out, db_path) {
                return e;
            }
            let plan = match plan(pool, db_path, &like, settings_dir).await {
                Ok(plan) => plan,
                Err(e) => return e,
            };
            if !execute {
                return (
                    output::mutation_dry_run(
                        "backup.create",
                        plan.summary(&out),
                        "Add --execute to write the zip",
                    ),
                    output::EXIT_OK,
                );
            }
            match write_zip(&out, &plan.entries) {
                Ok(()) => (
                    output::mutation_done("backup.create", plan.summary(&out)),
                    output::EXIT_OK,
                ),
                Err(e) => (
                    output::error("io", output::EXIT_GENERAL, &e.to_string(), None),
                    output::EXIT_GENERAL,
                ),
            }
        }
    }
}

/// Stops before reading anything when the backup cannot be made safely.
fn preflight(out: &Path, db_path: &Path) -> Result<(), (serde_json::Value, i32)> {
    let conflict = |message: &str, next_step: &str| {
        Err((
            output::error("conflict", output::EXIT_CONFLICT, message, Some(next_step)),
            output::EXIT_CONFLICT,
        ))
    };
    if out.exists() {
        return conflict(
            &format!("{} already exists", out.display()),
            "Choose another file name, or move the old backup away",
        );
    }
    // master.db is in WAL mode. A closed rekordbox leaves an empty -wal;
    // pages in it are changes that master.db alone does not have yet.
    let wal = db_path.with_file_name("master.db-wal");
    if std::fs::metadata(&wal).is_ok_and(|m| m.len() > 0) {
        return conflict(
            &format!(
                "{} is not empty: rekordbox is open, or did not close cleanly",
                wal.display()
            ),
            "Close rekordbox (or open and close it once), then try again",
        );
    }
    Ok(())
}

async fn plan(
    pool: &SqlitePool,
    db_path: &Path,
    like: &Path,
    settings_dir: Option<PathBuf>,
) -> Result<Plan, (serde_json::Value, i32)> {
    let db_dir = db_path.parent().unwrap_or(Path::new("."));
    let mut plan = Plan::default();
    plan.file("master.db", db_path.to_path_buf());
    for name in LIBRARY_FILES {
        plan.file(name, db_dir.join(name));
    }
    let settings_dir = match settings_dir {
        Some(dir) => dir,
        None => registry_settings_dir(pool, db_dir).await?,
    };
    // rekordbox 6 and 7 still keep this one in the older Pioneer/rekordbox dir
    let older = settings_dir
        .parent()
        .unwrap_or(Path::new("."))
        .join("rekordbox");
    plan.file("playlists3.sync", older.join("playlists3.sync"));
    for name in SETTINGS_FILES {
        plan.file(name, settings_dir.join(name));
    }
    track_folders(pool, db_dir, &mut plan).await?;
    plan.entries.extend(key_mappings(&settings_dir));
    plan.entries.push(Entry {
        name: "contents.xml".to_string(),
        source: Source::Bytes(contents_xml(pool, like).await?.into_bytes()),
    });
    Ok(plan)
}

/// rekordbox's keyboard mappings (rekordbox_*.mappings), by name.
/// The edit-mode ones (rekordboxedit_*) are not in rekordbox's backup.
fn key_mappings(settings_dir: &Path) -> Vec<Entry> {
    let dir = settings_dir.join("KeyMappings");
    let Ok(files) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = files
        .flatten()
        .map(|f| f.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("rekordbox_") && n.ends_with(".mappings"))
        .collect();
    names.sort();
    names
        .into_iter()
        .map(|n| Entry {
            name: format!("KeyMappings/{}", n),
            source: Source::File(dir.join(&n)),
        })
        .collect()
}

/// contents.xml: the one in the --like backup, with the library values recomputed.
async fn contents_xml(pool: &SqlitePool, like: &Path) -> Result<String, (serde_json::Value, i32)> {
    let template = read_like(like).map_err(|e| {
        (
            output::error(
                "config",
                output::EXIT_CONFIG,
                &format!("Cannot read contents.xml in {}: {}", like.display(), e),
                Some("Pass a backup zip made by rekordbox with --like"),
            ),
            output::EXIT_CONFIG,
        )
    })?;
    let (dbid,) = sqlx::query_as::<_, (String,)>("SELECT DBID FROM djmdProperty")
        .fetch_one(pool)
        .await
        .map_err(db_error)?;
    let paths = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT FolderPath FROM djmdContent WHERE rb_local_deleted = 0 ORDER BY rowid",
    )
    .fetch_all(pool)
    .await
    .map_err(db_error)?;
    let paths: Vec<String> = paths.into_iter().filter_map(|(p,)| p).collect();
    let content_num = paths.iter().filter(|p| backup::is_local_file(p)).count();
    let user_dirs = backup::user_dirs(&template);
    let user_dirs: Vec<&str> = user_dirs.iter().map(String::as_str).collect();
    let roots = backup::directory_array(paths.iter().map(String::as_str), &user_dirs);
    Ok(backup::manifest(
        &template,
        &[
            (
                "DB_ID",
                backup::db_id(&dbid).unwrap_or_default().to_string(),
            ),
            ("CONTENT_NUM", content_num.to_string()),
            ("DIRECTORY_ARRAY", roots.join(",")),
        ],
    ))
}

fn read_like(like: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let mut zip = zip::ZipArchive::new(File::open(like)?)?;
    let mut template = String::new();
    zip.by_name("contents.xml")?.read_to_string(&mut template)?;
    Ok(template)
}

/// The analysis and artwork folders that tracks refer to.
/// Their paths are relative to <master.db dir>/share.
/// Folders no track refers to are left out, as rekordbox does.
async fn track_folders(
    pool: &SqlitePool,
    db_dir: &Path,
    plan: &mut Plan,
) -> Result<(), (serde_json::Value, i32)> {
    let rows = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT AnalysisDataPath, ImagePath FROM djmdContent \
         WHERE rb_local_deleted = 0 ORDER BY rowid",
    )
    .fetch_all(pool)
    .await
    .map_err(db_error)?;
    let share = db_dir.join("share");
    let mut seen = std::collections::HashSet::new();
    // all the analysis folders first, then all the artwork folders, both in track order
    let (anlz, artwork): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
    for path in anlz.into_iter().chain(artwork).flatten() {
        let Some((folder, _)) = path.trim_start_matches('/').rsplit_once('/') else {
            continue;
        };
        if !seen.insert(folder.to_string()) {
            continue;
        }
        let Ok(dir) = std::fs::read_dir(share.join(folder)) else {
            plan.missing.push(format!("share/{}", folder));
            continue;
        };
        let mut files: Vec<String> = dir
            .flatten()
            .filter(|f| f.file_type().is_ok_and(|t| t.is_file()))
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .collect();
        files.sort_by_key(|f| {
            let rank = FOLDER_ORDER
                .iter()
                .position(|o| format!("/{}", f).ends_with(o));
            (rank.unwrap_or(FOLDER_ORDER.len()), f.clone())
        });
        for file in files {
            plan.entries.push(Entry {
                name: format!("share/{}/{}", folder, file),
                source: Source::File(share.join(folder).join(&file)),
            });
        }
    }
    Ok(())
}

/// rekordbox records its settings dir in agentRegistry, as a path on its own computer.
/// SyncAnalysisDataRootPath (<master.db dir>\share) tells how that maps to here.
async fn registry_settings_dir(
    pool: &SqlitePool,
    db_dir: &Path,
) -> Result<PathBuf, (serde_json::Value, i32)> {
    let config_error = |message: &str| {
        (
            output::error(
                "config",
                output::EXIT_CONFIG,
                message,
                Some("Pass the rekordbox settings dir with --settings-dir"),
            ),
            output::EXIT_CONFIG,
        )
    };
    let settings = registry_str(pool, "SyncSettingsRootPath")
        .await?
        .ok_or_else(|| config_error("agentRegistry has no SyncSettingsRootPath"))?;
    let share = registry_str(pool, "SyncAnalysisDataRootPath")
        .await?
        .unwrap_or_default();
    let db_windows_dir = share
        .strip_suffix("\\share")
        .or_else(|| share.strip_suffix("/share"))
        .unwrap_or(&share);
    backup::local_path(&settings, db_windows_dir, db_dir)
        .map_err(|e| config_error(&format!("Cannot find the settings dir {}: {}", settings, e)))
}

async fn registry_str(
    pool: &SqlitePool,
    registry_id: &str,
) -> Result<Option<String>, (serde_json::Value, i32)> {
    let row = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT str_1 FROM agentRegistry WHERE registry_id = ?",
    )
    .bind(registry_id)
    .fetch_optional(pool)
    .await
    .map_err(db_error)?;
    Ok(row.and_then(|(p,)| p).filter(|p| !p.is_empty()))
}

fn write_zip(out: &Path, entries: &[Entry]) -> Result<(), Box<dyn std::error::Error>> {
    let mut zip = zip::ZipWriter::new(File::create_new(out)?);
    for entry in entries {
        let modified = match &entry.source {
            Source::File(path) => std::fs::metadata(path)?.modified()?.into(),
            Source::Bytes(_) => chrono::Local::now(),
        };
        // zip times are local time without a zone, as rekordbox writes them
        let options = SimpleFileOptions::default()
            .last_modified_time(zip::DateTime::try_from(modified.naive_local())?);
        zip.start_file(entry.name.as_str(), options)?;
        match &entry.source {
            Source::File(path) => {
                std::io::copy(&mut File::open(path)?, &mut zip)?;
            }
            Source::Bytes(bytes) => zip.write_all(bytes)?,
        }
    }
    zip.finish()?;
    Ok(())
}

// --- describe ---

use crate::describe::{describe_command, describe_resource, flag, mutation_result_schema};

/// `rbx describe backup [action]`.
pub(crate) fn describe(action: Option<&str>) -> Option<serde_json::Value> {
    Some(match action {
        None => describe_resource(
            "backup",
            &[(
                "create",
                "Write a library backup zip in rekordbox's own format (dry-run by default)",
            )],
        ),
        Some("create") => describe_command(
            "backup create",
            &[
                flag("out", "string", true, "Zip file to write. Must not exist yet"),
                flag(
                    "--like",
                    "string",
                    true,
                    "A backup zip made by rekordbox on this computer. Its contents.xml is the template: \
                     the computer's values are kept, DB_ID / CONTENT_NUM / DIRECTORY_ARRAY are recomputed",
                ),
                flag(
                    "--settings-dir",
                    "string",
                    false,
                    "rekordbox's settings dir. Default: agentRegistry SyncSettingsRootPath, \
                     found through the drive mounts under WSL",
                ),
                flag(
                    "--execute",
                    "bool",
                    false,
                    "Actually write the zip (default: dry-run)",
                ),
            ],
            &mutation_result_schema("backup.create"),
            &[
                "rbx backup create rekordbox_bak_20261015.zip --like rekordbox_bak_20261014.zip",
                "rbx backup create rekordbox_bak_20261015.zip --like rekordbox_bak_20261014.zip --execute",
            ],
        ),
        _ => return None,
    })
}
