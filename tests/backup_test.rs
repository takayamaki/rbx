//! `rbx backup create`: a library backup in the same shape as the one
//! rekordbox writes from Preferences → Advanced → Database → Library backup.
//!
//! Checked against a backup made by rekordbox 7.2.18 (44,614 entries).

mod common;

mod fixture {
    use std::io::{Read, Write};
    use std::path::{Path, PathBuf};

    use assert_cmd::Command;
    use tempfile::TempDir;

    /// The files rekordbox keeps next to master.db that go into the backup.
    pub const LIBRARY_FILES: [&str; 5] = [
        "smartlist.xml",
        "masterPlaylists6.xml",
        "automixPlaylist6.xml",
        "networkAnalyze6.db",
        "networkRecommend.db",
    ];

    /// The files rekordbox copies from its settings dir, in its order.
    pub const SETTINGS_FILES: [&str; 13] = [
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

    /// A library laid out like a rekordbox install, all in one tempdir.
    pub struct Library {
        pub _dir: TempDir,
        pub db: PathBuf,
        pub like: PathBuf,
        pub out: PathBuf,
    }

    pub const LIKE_CONTENTS_XML: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n\
        <PROPERTIES>\r\n\
        \x20 <VALUE name=\"DB_ID\" val=\"1\"/>\r\n\
        \x20 <VALUE name=\"CONTENT_ID_MAX\" val=\"0\"/>\r\n\
        \x20 <VALUE name=\"CONTENT_NUM\" val=\"0\"/>\r\n\
        \x20 <VALUE name=\"BACKUP_RB_VER\" val=\"70218\"/>\r\n\
        \x20 <VALUE name=\"BACKUP_FILE_VER\" val=\"4\"/>\r\n\
        \x20 <VALUE name=\"DIRECTORY_ARRAY\" val=\"\"/>\r\n\
        \x20 <VALUE name=\"userMusicDirectory\" val=\"C:/Music\"/>\r\n\
        \x20 <VALUE name=\"userHomeDirectory\" val=\"C:/Users/dj\"/>\r\n\
        </PROPERTIES>\r\n";

    pub async fn library() -> Library {
        let (db, dir) = crate::common::setup_db().await;
        let db_dir = db.parent().unwrap();
        for name in LIBRARY_FILES {
            std::fs::write(db_dir.join(name), format!("{} body", name)).unwrap();
        }
        // rekordbox's own rolling copies; not part of a backup
        std::fs::write(db_dir.join("master.backup.db"), "old").unwrap();

        let settings = dir.path().join("Pioneer").join("rekordbox6");
        std::fs::create_dir_all(settings.join("LightingDB")).unwrap();
        for name in SETTINGS_FILES {
            std::fs::write(settings.join(name), format!("{} body", name)).unwrap();
        }
        // a settings file rekordbox does not back up
        std::fs::write(settings.join("EditSettings.xml"), "edit").unwrap();
        set_registry(&db, "SyncSettingsRootPath", settings.to_str().unwrap()).await;

        let like = dir.path().join("like.zip");
        write_zip(&like, &[("contents.xml", LIKE_CONTENTS_XML.as_bytes())]);
        let out = dir.path().join("out").join("rekordbox_bak.zip");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        Library {
            _dir: dir,
            db,
            like,
            out,
        }
    }

    /// Gives a track analysis and artwork folders under <db dir>/share, like rekordbox does.
    pub async fn add_track_files(db: &Path, track_id: &str, folder: &str) -> Vec<String> {
        let share = db.parent().unwrap().join("share");
        let mut names = Vec::new();
        for (kind, files) in [
            ("USBANLZ", ["ANLZ0000.DAT", "ANLZ0000.EXT", "ANLZ0000.2EX"]),
            ("Artwork", ["artwork.jpg", "artwork_s.jpg", "artwork_m.jpg"]),
        ] {
            let dir = share.join("PIONEER").join(kind).join(folder);
            std::fs::create_dir_all(&dir).unwrap();
            // written in reverse, so the zip order cannot come from the creation order
            for file in files.iter().rev() {
                std::fs::write(dir.join(file), format!("{} {}", folder, file)).unwrap();
            }
            for file in files {
                names.push(format!("share/PIONEER/{}/{}/{}", kind, folder, file));
            }
        }
        let pool = crate::common::open_pool(db).await;
        sqlx::query("UPDATE djmdContent SET AnalysisDataPath = ?, ImagePath = ? WHERE ID = ?")
            .bind(format!("/PIONEER/USBANLZ/{}/ANLZ0000.DAT", folder))
            .bind(format!("/PIONEER/Artwork/{}/artwork.jpg", folder))
            .bind(track_id)
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        names
    }

    pub async fn set_registry(db: &Path, registry_id: &str, value: &str) {
        let pool = crate::common::open_pool(db).await;
        sqlx::query(
            "INSERT OR REPLACE INTO agentRegistry (registry_id, str_1, created_at, updated_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(registry_id)
        .bind(value)
        .bind(crate::common::SEED_TS)
        .bind(crate::common::SEED_TS)
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    pub fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }

    pub fn rbx_cmd(db_path: &Path) -> Command {
        let mut cmd = Command::cargo_bin("rbx").unwrap();
        cmd.env_remove("RBX_DB_PATH");
        cmd.arg("--db").arg(db_path);
        cmd
    }

    /// `rbx backup create <out> --like <like> --execute`
    pub fn create(lib: &Library) -> Command {
        let mut cmd = rbx_cmd(&lib.db);
        cmd.args(["backup", "create"])
            .arg(&lib.out)
            .arg("--like")
            .arg(&lib.like)
            .arg("--execute");
        cmd
    }

    pub fn zip_names(path: &Path) -> Vec<String> {
        let zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        zip.file_names().map(|n| n.unwrap().to_string()).collect()
    }

    pub fn zip_read(path: &Path, name: &str) -> Vec<u8> {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
        let mut body = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut body).unwrap();
        body
    }
}

mod create {
    use crate::fixture::*;

    #[tokio::test]
    async fn zips_master_db_and_the_library_files_beside_it() {
        let lib = library().await;

        create(&lib).assert().code(0);

        let names = zip_names(&lib.out);
        assert_eq!(names[0], "master.db");
        assert_eq!(
            zip_read(&lib.out, "master.db"),
            std::fs::read(&lib.db).unwrap()
        );
        for name in LIBRARY_FILES {
            assert_eq!(
                zip_read(&lib.out, name),
                format!("{} body", name).into_bytes(),
                "{}",
                name
            );
        }
        assert!(!names.contains(&"master.backup.db".to_string()));
    }

    #[tokio::test]
    async fn dry_run_lists_what_would_be_zipped_and_writes_nothing() {
        let lib = library().await;

        let assert = rbx_cmd(&lib.db)
            .args(["backup", "create"])
            .arg(&lib.out)
            .arg("--like")
            .arg(&lib.like)
            .assert()
            .code(0);

        let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
        assert_eq!(json["dry_run"], true);
        assert!(json["next_step"].as_str().unwrap().contains("--execute"));
        assert!(!lib.out.exists());

        create(&lib).assert().code(0);
        assert_eq!(json["plan"]["entries"], zip_names(&lib.out).len());
    }

    #[tokio::test]
    async fn zips_the_settings_files_from_the_settings_dir_in_agent_registry() {
        let lib = library().await;

        create(&lib).assert().code(0);

        let names = zip_names(&lib.out);
        let start = 1 + LIBRARY_FILES.len();
        assert_eq!(names[start..start + SETTINGS_FILES.len()], SETTINGS_FILES);
        for name in SETTINGS_FILES {
            assert_eq!(
                zip_read(&lib.out, name),
                format!("{} body", name).into_bytes()
            );
        }
        assert!(!names.contains(&"EditSettings.xml".to_string()));
    }

    #[tokio::test]
    async fn zips_the_analysis_and_artwork_folders_that_tracks_refer_to() {
        let lib = library().await;
        let mut expected = add_track_files(&lib.db, "101", "e01/39bfa-1560").await;
        expected.extend(add_track_files(&lib.db, "102", "5f7/75f48-01aa").await);

        create(&lib).assert().code(0);

        let names = zip_names(&lib.out);
        let share: Vec<&String> = names.iter().filter(|n| n.starts_with("share/")).collect();
        assert_eq!(share, expected.iter().collect::<Vec<_>>());
        for name in &expected {
            let body = String::from_utf8(zip_read(&lib.out, name)).unwrap();
            assert!(name.ends_with(body.split(' ').nth(1).unwrap()));
        }
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn writes_contents_xml_from_the_like_backup_with_the_library_values_recomputed() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn leaves_out_analysis_folders_no_track_refers_to() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn keeps_file_modification_times() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn zips_playlists3_sync_from_the_older_rekordbox_settings_dir() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn zips_only_the_rekordbox_key_mappings() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn reports_files_and_folders_that_are_missing_on_disk() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn settings_dir_flag_overrides_agent_registry() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn refuses_to_overwrite_an_existing_file() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn refuses_while_master_db_has_a_write_ahead_log() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn fails_when_the_like_backup_has_no_contents_xml() {
        todo!()
    }
}

mod directory_array {
    use rbx::backup::directory_array;

    const USER_DIRS: [&str; 2] = ["D:/My Music", "C:/Users/dj"];

    #[test]
    fn files_under_a_user_directory_are_grouped_by_that_directory() {
        let paths = ["D:/My Music/album/01.m4a", "D:/My Music/02.mp3"];
        assert_eq!(directory_array(paths, &USER_DIRS), ["D:/My Music"]);
    }

    #[test]
    fn files_outside_user_directories_are_grouped_by_drive() {
        let paths = ["F:/DJ/01.m4a", "D:/DJ/02.m4a", "F:/DJ/03.m4a"];
        assert_eq!(directory_array(paths, &USER_DIRS), ["F:", "D:"]);
    }

    #[test]
    #[ignore = "todo"]
    fn lists_roots_in_the_order_they_first_appear() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn the_longest_matching_user_directory_wins() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn a_directory_does_not_match_a_longer_name_that_starts_with_it() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn streaming_tracks_are_not_files() {
        todo!()
    }
}

mod local_path {
    #[test]
    #[ignore = "todo"]
    fn maps_a_windows_path_through_the_mount_the_db_drive_uses() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn keeps_a_posix_path_as_it_is() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn drive_letters_match_in_either_case() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn fails_when_the_db_path_does_not_end_with_the_windows_db_dir() {
        todo!()
    }
}

mod manifest {
    #[test]
    #[ignore = "todo"]
    fn replaces_the_library_values_and_keeps_every_other_byte() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn db_id_is_the_dbid_as_a_signed_32_bit_number() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn escapes_xml_special_characters_in_values() {
        todo!()
    }
}
