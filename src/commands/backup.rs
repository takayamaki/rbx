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

/// rekordbox's order for the files in one analysis or artwork folder. Other files follow by name.
const FOLDER_ORDER: [&str; 7] = [
    "ANLZ0000.DAT",
    "ANLZ0000.EXT",
    "ANLZ0000.2EX",
    "ANLZ0000.3EX",
    "artwork.jpg",
    "artwork_s.jpg",
    "artwork_m.jpg",
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

pub(crate) async fn handle_backup(
    pool: &SqlitePool,
    db_path: &Path,
    action: BackupAction,
) -> (serde_json::Value, i32) {
    match action {
        BackupAction::Create { out, like, execute } => {
            let entries = match plan(pool, db_path, &like).await {
                Ok(entries) => entries,
                Err(e) => return e,
            };
            if !execute {
                return (
                    output::mutation_dry_run(
                        "backup.create",
                        serde_json::json!({ "out": out, "entries": entries.len() }),
                        "Add --execute to write the zip",
                    ),
                    output::EXIT_OK,
                );
            }
            match write_zip(&out, &entries) {
                Ok(()) => (
                    output::mutation_done(
                        "backup.create",
                        serde_json::json!({ "out": out, "entries": entries.len() }),
                    ),
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

/// The zip's entries in rekordbox's order.
async fn plan(
    pool: &SqlitePool,
    db_path: &Path,
    like: &Path,
) -> Result<Vec<Entry>, (serde_json::Value, i32)> {
    let db_dir = db_path.parent().unwrap_or(Path::new("."));
    let mut entries = vec![Entry {
        name: "master.db".to_string(),
        source: Source::File(db_path.to_path_buf()),
    }];
    for name in LIBRARY_FILES {
        entries.push(Entry {
            name: name.to_string(),
            source: Source::File(db_dir.join(name)),
        });
    }
    let settings_dir = settings_dir(pool).await?;
    for name in SETTINGS_FILES {
        entries.push(Entry {
            name: name.to_string(),
            source: Source::File(settings_dir.join(name)),
        });
    }
    entries.extend(track_folders(pool, db_dir).await?);
    entries.push(Entry {
        name: "contents.xml".to_string(),
        source: Source::Bytes(contents_xml(pool, like).await?.into_bytes()),
    });
    Ok(entries)
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

/// The analysis and artwork folders that tracks refer to, track by track.
/// Their paths are relative to <master.db dir>/share.
/// Folders no track refers to are left out, as rekordbox does.
async fn track_folders(
    pool: &SqlitePool,
    db_dir: &Path,
) -> Result<Vec<Entry>, (serde_json::Value, i32)> {
    let rows = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        "SELECT AnalysisDataPath, ImagePath FROM djmdContent \
         WHERE rb_local_deleted = 0 ORDER BY rowid",
    )
    .fetch_all(pool)
    .await
    .map_err(db_error)?;
    let share = db_dir.join("share");
    let mut seen = std::collections::HashSet::new();
    let mut entries = Vec::new();
    for path in rows
        .into_iter()
        .flat_map(|(anlz, image)| [anlz, image])
        .flatten()
    {
        let Some((folder, _)) = path.trim_start_matches('/').rsplit_once('/') else {
            continue;
        };
        if !seen.insert(folder.to_string()) {
            continue;
        }
        let Ok(dir) = std::fs::read_dir(share.join(folder)) else {
            continue;
        };
        let mut files: Vec<String> = dir
            .flatten()
            .filter(|f| f.file_type().is_ok_and(|t| t.is_file()))
            .map(|f| f.file_name().to_string_lossy().into_owned())
            .collect();
        files.sort_by_key(|f| {
            let rank = FOLDER_ORDER.iter().position(|o| o == f);
            (rank.unwrap_or(FOLDER_ORDER.len()), f.clone())
        });
        for file in files {
            entries.push(Entry {
                name: format!("share/{}/{}", folder, file),
                source: Source::File(share.join(folder).join(&file)),
            });
        }
    }
    Ok(entries)
}

/// rekordbox records its settings dir in agentRegistry.
async fn settings_dir(pool: &SqlitePool) -> Result<PathBuf, (serde_json::Value, i32)> {
    let row = sqlx::query_as::<_, (Option<String>,)>(
        "SELECT str_1 FROM agentRegistry WHERE registry_id = 'SyncSettingsRootPath'",
    )
    .fetch_optional(pool)
    .await
    .map_err(db_error)?;
    match row.and_then(|(p,)| p).filter(|p| !p.is_empty()) {
        Some(p) => Ok(PathBuf::from(p)),
        None => Err((
            output::error(
                "config",
                output::EXIT_CONFIG,
                "agentRegistry has no SyncSettingsRootPath",
                Some("Pass the rekordbox settings dir with --settings-dir"),
            ),
            output::EXIT_CONFIG,
        )),
    }
}

fn write_zip(out: &Path, entries: &[Entry]) -> Result<(), Box<dyn std::error::Error>> {
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    let options = SimpleFileOptions::default();
    for entry in entries {
        zip.start_file(entry.name.as_str(), options)?;
        match &entry.source {
            Source::File(path) => zip.write_all(&std::fs::read(path)?)?,
            Source::Bytes(bytes) => zip.write_all(bytes)?,
        }
    }
    zip.finish()?;
    Ok(())
}
