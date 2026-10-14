//! `backup create`: a library backup in the same shape as the one rekordbox writes
//! from Preferences → Advanced → Database → Library backup.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use rbx::output;
use sqlx::sqlite::SqlitePool;
use zip::write::SimpleFileOptions;

use crate::cli::BackupAction;

/// Files next to master.db that rekordbox puts at the top of the zip, after master.db.
const LIBRARY_FILES: [&str; 5] = [
    "smartlist.xml",
    "masterPlaylists6.xml",
    "automixPlaylist6.xml",
    "networkAnalyze6.db",
    "networkRecommend.db",
];

/// One file in the zip: its name there and where it is read from.
struct Entry {
    name: String,
    source: PathBuf,
}

pub(crate) async fn handle_backup(
    _pool: &SqlitePool,
    db_path: &Path,
    action: BackupAction,
) -> (serde_json::Value, i32) {
    match action {
        BackupAction::Create { out, execute, .. } => {
            let entries = plan(db_path);
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
fn plan(db_path: &Path) -> Vec<Entry> {
    let db_dir = db_path.parent().unwrap_or(Path::new("."));
    let mut entries = vec![Entry {
        name: "master.db".to_string(),
        source: db_path.to_path_buf(),
    }];
    for name in LIBRARY_FILES {
        entries.push(Entry {
            name: name.to_string(),
            source: db_dir.join(name),
        });
    }
    entries
}

fn write_zip(out: &Path, entries: &[Entry]) -> Result<(), Box<dyn std::error::Error>> {
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    let options = SimpleFileOptions::default();
    for entry in entries {
        zip.start_file(entry.name.as_str(), options)?;
        zip.write_all(&std::fs::read(&entry.source)?)?;
    }
    zip.finish()?;
    Ok(())
}
