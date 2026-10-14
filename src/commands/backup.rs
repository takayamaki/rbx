//! `backup create`: a library backup in the same shape as the one rekordbox writes
//! from Preferences → Advanced → Database → Library backup.

use std::fs::File;
use std::io::Write;
use std::path::Path;

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

pub(crate) async fn handle_backup(
    _pool: &SqlitePool,
    db_path: &Path,
    action: BackupAction,
) -> (serde_json::Value, i32) {
    match action {
        BackupAction::Create { out, .. } => match create(db_path, &out) {
            Ok(()) => (
                output::mutation_done("backup.create", serde_json::json!({ "path": out })),
                output::EXIT_OK,
            ),
            Err(e) => (
                output::error("io", output::EXIT_GENERAL, &e.to_string(), None),
                output::EXIT_GENERAL,
            ),
        },
    }
}

fn create(db_path: &Path, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let db_dir = db_path.parent().unwrap_or(Path::new("."));
    let mut zip = zip::ZipWriter::new(File::create(out)?);
    let options = SimpleFileOptions::default();
    zip.start_file("master.db", options)?;
    zip.write_all(&std::fs::read(db_path)?)?;
    for name in LIBRARY_FILES {
        zip.start_file(name, options)?;
        zip.write_all(&std::fs::read(db_dir.join(name))?)?;
    }
    zip.finish()?;
    Ok(())
}
