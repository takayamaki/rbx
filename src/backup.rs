//! Library backup in the same shape as rekordbox's own
//! (Preferences → Advanced → Database → Library backup).

/// DIRECTORY_ARRAY in contents.xml: the roots of the tracks' file paths.
/// A root is the user folder the file is in, or else its drive ("F:").
pub fn directory_array<'a>(
    paths: impl IntoIterator<Item = &'a str>,
    user_dirs: &[&str],
) -> Vec<String> {
    let mut roots: Vec<String> = Vec::new();
    for path in paths {
        let root = match user_dirs.iter().find(|d| path.starts_with(*d)) {
            Some(dir) => dir.to_string(),
            None => path[..2].to_string(),
        };
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}
