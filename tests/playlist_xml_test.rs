use std::path::PathBuf;

use rbx::playlist_xml::{add_node, remove_node};

const FIXTURE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>

<MASTER_PLAYLIST Version="3.0.0" AutomaticSync="0">
  <PRODUCT Name="rekordbox" Version="7.2.7" Company="Pioneer DJ"/>
  <PLAYLISTS>
    <NODE Id="7DE634D2" ParentId="0" Attribute="1" Timestamp="0" Lib_Type="0" CheckType="0"/>
    <NODE Id="20D42DD" ParentId="7DE634D2" Attribute="0" Timestamp="1783243026596" Lib_Type="0" CheckType="0"/>
  </PLAYLISTS>
</MASTER_PLAYLIST>
"#;

fn write_fixture(dir: &tempfile::TempDir) -> PathBuf {
    let path = dir.path().join("masterPlaylists6.xml");
    std::fs::write(&path, FIXTURE).unwrap();
    path
}

#[test]
fn add_node_inserts_hex_node_before_closing_tag() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_fixture(&dir);

    // 198736747 = 0xBD87B6B
    let added = add_node(&path, "198736747", "root", 0, 1783478672000).unwrap();
    assert!(added);

    let content = std::fs::read_to_string(&path).unwrap();
    let expected_line = "    <NODE Id=\"BD87B6B\" ParentId=\"0\" Attribute=\"0\" Timestamp=\"1783478672000\" Lib_Type=\"0\" CheckType=\"0\"/>";
    assert!(content.contains(expected_line), "node line missing:\n{}", content);

    // Exactly one line added right before </PLAYLISTS>; the rest is untouched.
    let restored: String = content
        .lines()
        .filter(|l| *l != expected_line)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    assert_eq!(restored, FIXTURE);

    let idx_node = content.find(expected_line).unwrap();
    let idx_close = content.find("  </PLAYLISTS>").unwrap();
    assert!(idx_node < idx_close);
}

#[test]
fn add_node_with_numeric_parent_uses_parent_hex() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_fixture(&dir);

    // parent 2112238802 = 0x7DE634D2
    add_node(&path, "198736747", "2112238802", 0, 42).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("<NODE Id=\"BD87B6B\" ParentId=\"7DE634D2\""));
}

#[test]
fn add_node_returns_false_for_existing_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_fixture(&dir);

    // 0x20D42DD is already present in the fixture
    let existing_id = u64::from_str_radix("20D42DD", 16).unwrap().to_string();
    let added = add_node(&path, &existing_id, "root", 0, 42).unwrap();
    assert!(!added);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), FIXTURE);
}

#[test]
fn remove_node_removes_exactly_the_matching_line() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_fixture(&dir);

    let target_id = u64::from_str_radix("20D42DD", 16).unwrap().to_string();
    let removed = remove_node(&path, &target_id).unwrap();
    assert!(removed);

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(!content.contains("Id=\"20D42DD\""));
    assert!(content.contains("Id=\"7DE634D2\""));
    assert!(content.contains("</PLAYLISTS>"));

    // Second removal finds nothing.
    let removed_again = remove_node(&path, &target_id).unwrap();
    assert!(!removed_again);
}

#[test]
fn id_matching_is_not_fooled_by_parent_id() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_fixture(&dir);

    // 0x7DE634D2 appears in the fixture ONLY as a ParentId of the second node
    // (and as the Id of the first). Target a hex that exists solely as a
    // ParentId: remove the first node so 7DE634D2 remains only as ParentId.
    let folder_id = u64::from_str_radix("7DE634D2", 16).unwrap().to_string();
    remove_node(&path, &folder_id).unwrap();
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("ParentId=\"7DE634D2\""));
    assert!(!content.contains("<NODE Id=\"7DE634D2\""));

    // add_node must not treat the remaining ParentId reference as a duplicate.
    let added = add_node(&path, &folder_id, "root", 1, 42).unwrap();
    assert!(added, "ParentId occurrence must not count as an existing Id");

    // remove_node must remove the node whose Id matches, not the child that
    // merely references it as ParentId.
    let removed = remove_node(&path, &folder_id).unwrap();
    assert!(removed);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("ParentId=\"7DE634D2\""));
    assert!(!content.contains("<NODE Id=\"7DE634D2\""));
}

#[test]
fn missing_file_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("masterPlaylists6.xml");

    assert!(!add_node(&path, "198736747", "root", 0, 42).unwrap());
    assert!(!remove_node(&path, "198736747").unwrap());
}

#[test]
fn non_numeric_playlist_id_returns_false() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_fixture(&dir);

    let uuid_id = "bf5881f9-af4e-4faf-94bb-3db8a88ca9a0";
    assert!(!add_node(&path, uuid_id, "root", 0, 42).unwrap());
    assert!(!remove_node(&path, uuid_id).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), FIXTURE);
}
