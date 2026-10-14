//! Library backup in the same shape as rekordbox's own
//! (Preferences → Advanced → Database → Library backup).

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
        let head = format!("<VALUE name=\"{}\" val=\"", name);
        let Some(start) = out.find(&head).map(|i| i + head.len()) else {
            continue;
        };
        let Some(len) = out[start..].find('"') else {
            continue;
        };
        out.replace_range(start..start + len, value);
    }
    out
}

/// DB_ID in contents.xml: djmdProperty.DBID (an unsigned 32-bit number) read as signed.
pub fn db_id(dbid: &str) -> Option<i32> {
    dbid.parse::<u32>().ok().map(|n| n as i32)
}
