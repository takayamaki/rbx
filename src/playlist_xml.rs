use std::io;
use std::path::{Path, PathBuf};

/// rekordbox keeps a parallel playlist tree in masterPlaylists6.xml next to
/// master.db. Playlists absent from this file do not appear in rekordbox.
/// Node Id / ParentId are the numeric DB IDs in uppercase hex.
pub fn xml_path_for(db_path: &Path) -> PathBuf {
    db_path
        .parent()
        .map(|p| p.join("masterPlaylists6.xml"))
        .unwrap_or_else(|| PathBuf::from("masterPlaylists6.xml"))
}

fn to_hex(id: &str) -> Option<String> {
    id.parse::<u64>().ok().map(|n| format!("{:X}", n))
}

const CLOSING_TAG: &str = "  </PLAYLISTS>";

/// Adds a NODE element for the playlist. Returns Ok(false) if the file does
/// not exist, the id is not numeric, or a node with the same Id is present.
pub fn add_node(
    xml_path: &Path,
    playlist_id: &str,
    parent_id: &str,
    attribute: i32,
    timestamp_ms: i64,
) -> io::Result<bool> {
    if !xml_path.exists() {
        return Ok(false);
    }
    let Some(hex_id) = to_hex(playlist_id) else {
        return Ok(false);
    };
    let parent_hex = if parent_id == "root" {
        "0".to_string()
    } else {
        match to_hex(parent_id) {
            Some(h) => h,
            None => return Ok(false),
        }
    };

    let content = std::fs::read_to_string(xml_path)?;
    // Match the Id attribute specifically: a bare Id="..." check would also
    // match ParentId="..." of an existing node.
    if content.contains(&format!("<NODE Id=\"{}\"", hex_id)) {
        return Ok(false);
    }
    if !content.contains(CLOSING_TAG) {
        return Ok(false);
    }
    let node = format!(
        "    <NODE Id=\"{}\" ParentId=\"{}\" Attribute=\"{}\" Timestamp=\"{}\" Lib_Type=\"0\" CheckType=\"0\"/>\n{}",
        hex_id, parent_hex, attribute, timestamp_ms, CLOSING_TAG,
    );
    let content = content.replacen(CLOSING_TAG, &node, 1);
    std::fs::write(xml_path, content)?;
    Ok(true)
}

/// Removes the NODE element for the playlist. Returns Ok(false) if the file
/// does not exist, the id is not numeric, or no matching node is found.
pub fn remove_node(xml_path: &Path, playlist_id: &str) -> io::Result<bool> {
    if !xml_path.exists() {
        return Ok(false);
    }
    let Some(hex_id) = to_hex(playlist_id) else {
        return Ok(false);
    };
    let needle = format!("<NODE Id=\"{}\"", hex_id);

    let content = std::fs::read_to_string(xml_path)?;
    let mut removed = false;
    let mut out = String::with_capacity(content.len());
    for line in content.lines() {
        if !removed && line.contains(&needle) {
            removed = true;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !removed {
        return Ok(false);
    }
    std::fs::write(xml_path, out)?;
    Ok(true)
}
