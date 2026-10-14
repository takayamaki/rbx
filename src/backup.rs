//! Library backup in the same shape as rekordbox's own
//! (Preferences → Advanced → Database → Library backup).

use std::ops::Range;
use std::path::{Path, PathBuf};

/// DIRECTORY_ARRAY in contents.xml: the roots of the tracks' file paths.
/// A root is the user folder the file is in, or else its drive ("F:").
pub fn directory_array<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    user_dirs: &[&str],
) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for path in paths.into_iter().filter(|p| is_local_file(p)) {
        let root = user_dirs
            .iter()
            .filter(|d| {
                path.strip_prefix(*d)
                    .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|d| d.len());
        let root = match root {
            Some(dir) => dir.to_string(),
            None => path[..2].to_string(),
        };
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}

/// A track path that is a file on a drive ("F:/DJ/01.m4a"),
/// not a streaming track ("spotify:track:…").
pub fn is_local_file(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/'
}

/// contents.xml from a template (the one in an earlier rekordbox backup),
/// with the given VALUEs replaced.
pub fn manifest(template: &str, values: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (name, value) in values {
        if let Some(range) = value_range(&out, name) {
            out.replace_range(range, &escape(value));
        }
    }
    out
}

/// DB_ID in contents.xml: djmdProperty.DBID (an unsigned 32-bit number) read as signed.
pub fn db_id(dbid: &str) -> Option<i32> {
    dbid.parse::<u32>().ok().map(|n| n as i32)
}

/// A VALUE in contents.xml.
pub fn manifest_value(manifest: &str, name: &str) -> Option<String> {
    value_range(manifest, name).map(|range| unescape(&manifest[range]))
}

/// Where the val="…" text of a VALUE is.
fn value_range(manifest: &str, name: &str) -> Option<Range<usize>> {
    let head = format!("<VALUE name=\"{}\" val=\"", name);
    let start = manifest.find(&head)? + head.len();
    let len = manifest[start..].find('"')?;
    Some(start..start + len)
}

/// The user folders (userMusicDirectory and so on) in contents.xml, for directory_array.
pub fn user_dirs(manifest: &str) -> Vec<String> {
    let mut dirs = Vec::new();
    let mut rest = manifest;
    while let Some(i) = rest.find("<VALUE name=\"user") {
        rest = &rest[i + "<VALUE name=\"".len()..];
        let Some(end) = rest.find('"') else { break };
        let name = &rest[..end];
        if name.ends_with("Directory") {
            dirs.extend(manifest_value(manifest, name));
        }
    }
    dirs
}

fn unescape(value: &str) -> String {
    value
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Where a path from rekordbox (agentRegistry) is on this computer.
/// db_windows_dir is master.db's dir as rekordbox sees it, db_local_dir as rbx opens it.
pub fn local_path(
    path: &str,
    db_windows_dir: &str,
    db_local_dir: &Path,
) -> Result<PathBuf, String> {
    let (Some(drive), Some(db_drive)) = (windows_drive(path), windows_drive(db_windows_dir)) else {
        return Ok(PathBuf::from(path));
    };
    let unmatched = || {
        format!(
            "{} is not where {} is mounted",
            db_local_dir.display(),
            db_windows_dir
        )
    };
    // "/mnt/f/PIONEER/Master" minus "PIONEER/Master" is the db drive's mount, "/mnt/f"
    let db_rest = windows_rest(db_windows_dir);
    let local = db_local_dir.to_str().ok_or_else(unmatched)?;
    let mount = local
        .trim_end_matches('/')
        .strip_suffix(db_rest.trim_matches('/'))
        .ok_or_else(unmatched)?
        .trim_end_matches('/');
    let (root, drive_dir) = mount.rsplit_once('/').ok_or_else(unmatched)?;
    if !drive_dir.eq_ignore_ascii_case(&db_drive.to_string()) {
        return Err(unmatched());
    }
    // the mount names drives in the same case as the db drive's ("f" → "c")
    let drive_dir = if drive_dir.chars().all(|c| c.is_ascii_lowercase()) {
        drive.to_ascii_lowercase()
    } else {
        drive.to_ascii_uppercase()
    };
    Ok(PathBuf::from(format!(
        "{}/{}{}",
        root,
        drive_dir,
        windows_rest(path)
    )))
}

/// The drive letter of a Windows path (`F:\…` → `F`).
fn windows_drive(path: &str) -> Option<char> {
    let mut chars = path.chars();
    let drive = chars.next().filter(char::is_ascii_alphabetic)?;
    (chars.next() == Some(':')).then_some(drive)
}

/// A Windows path after its drive, with "/" separators (`F:\PIONEER\Master` → `/PIONEER/Master`).
fn windows_rest(path: &str) -> String {
    path[2..].replace('\\', "/")
}
