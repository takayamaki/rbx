//! Library backup in the same shape as rekordbox's own
//! (Preferences → Advanced → Database → Library backup).

/// DIRECTORY_ARRAY in contents.xml: the roots of the tracks' file paths.
pub fn directory_array<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    user_dirs: &[&str],
) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for path in paths {
        let root = user_dirs.iter().find(|d| path.starts_with(*d));
        if let Some(root) = root {
            if !roots.iter().any(|r| r == root) {
                roots.push(root.to_string());
            }
        }
    }
    roots
}
