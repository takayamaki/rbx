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
        pub settings: PathBuf,
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
        let older = dir.path().join("Pioneer").join("rekordbox");
        std::fs::create_dir_all(&older).unwrap();
        std::fs::write(older.join("playlists3.sync"), "sync body").unwrap();

        let like = dir.path().join("like.zip");
        write_zip(&like, &[("contents.xml", LIKE_CONTENTS_XML.as_bytes())]);
        let out = dir.path().join("out").join("rekordbox_bak.zip");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        Library {
            _dir: dir,
            db,
            settings,
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
        let start = 1 + LIBRARY_FILES.len() + 1; // after playlists3.sync
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
        let first = add_track_files(&lib.db, "101", "e01/39bfa-1560").await;
        let second = add_track_files(&lib.db, "102", "5f7/75f48-01aa").await;
        // rekordbox: the analysis folders of all tracks, then their artwork folders
        let (anlz, artwork): (Vec<String>, Vec<String>) = first
            .into_iter()
            .chain(second)
            .partition(|n| n.contains("/USBANLZ/"));
        let expected: Vec<String> = anlz.into_iter().chain(artwork).collect();

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
    async fn writes_contents_xml_from_the_like_backup_with_the_library_values_recomputed() {
        let lib = library().await;
        let pool = crate::common::open_pool(&lib.db).await;
        for (id, path) in [("103", "F:/DJ/03.m4a"), ("104", "spotify:track:0")] {
            sqlx::query("INSERT INTO djmdContent (ID, FolderPath) VALUES (?, ?)")
                .bind(id)
                .bind(path)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool.close().await;

        create(&lib).assert().code(0);

        let names = zip_names(&lib.out);
        assert_eq!(names.last().unwrap(), "contents.xml");
        let expected = LIKE_CONTENTS_XML
            .replace("\"DB_ID\" val=\"1\"", "\"DB_ID\" val=\"-1095663931\"")
            .replace("\"CONTENT_NUM\" val=\"0\"", "\"CONTENT_NUM\" val=\"3\"")
            .replace(
                "\"DIRECTORY_ARRAY\" val=\"\"",
                "\"DIRECTORY_ARRAY\" val=\"C:/Music,F:\"",
            );
        assert_eq!(
            String::from_utf8(zip_read(&lib.out, "contents.xml")).unwrap(),
            expected
        );
    }

    /// rekordbox's backup left out 14 analysis folders that no track named.
    #[tokio::test]
    async fn leaves_out_analysis_folders_no_track_refers_to() {
        let lib = library().await;
        add_track_files(&lib.db, "101", "e01/39bfa-1560").await;
        let orphan = add_track_files(&lib.db, "102", "c42/25e98-e4d3").await;
        add_track_files(&lib.db, "102", "5f7/75f48-01aa").await;

        create(&lib).assert().code(0);

        let names = zip_names(&lib.out);
        for name in orphan {
            assert!(!names.contains(&name), "{}", name);
        }
    }

    /// Zip times are local time, as rekordbox writes them.
    #[tokio::test]
    async fn keeps_file_modification_times() {
        let lib = library().await;
        let path = lib.db.parent().unwrap().join("smartlist.xml");
        let mtime = chrono::NaiveDate::from_ymd_opt(2026, 8, 11)
            .unwrap()
            .and_hms_opt(20, 18, 34)
            .unwrap();
        let system_time: std::time::SystemTime =
            mtime.and_local_timezone(chrono::Local).unwrap().into();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(system_time)
            .unwrap();

        create(&lib).assert().code(0);

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&lib.out).unwrap()).unwrap();
        let entry = zip.by_name("smartlist.xml").unwrap();
        let zipped = chrono::NaiveDateTime::try_from(entry.last_modified().unwrap()).unwrap();
        assert_eq!(zipped, mtime);
    }

    /// rekordbox 6 and 7 keep it in Pioneer/rekordbox, next to the rekordbox6 settings dir.
    #[tokio::test]
    async fn zips_playlists3_sync_from_the_older_rekordbox_settings_dir() {
        let lib = library().await;

        create(&lib).assert().code(0);

        let names = zip_names(&lib.out);
        assert_eq!(names[1 + LIBRARY_FILES.len()], "playlists3.sync");
        assert_eq!(zip_read(&lib.out, "playlists3.sync"), b"sync body");
    }

    /// rekordbox puts them after the share/ folders, just before contents.xml.
    #[tokio::test]
    async fn zips_only_the_rekordbox_key_mappings() {
        let lib = library().await;
        add_track_files(&lib.db, "101", "e01/39bfa-1560").await;
        let mappings = lib.settings.join("KeyMappings");
        std::fs::create_dir_all(&mappings).unwrap();
        for name in [
            "rekordbox_9999999999999.mappings",
            "rekordbox_0000000000000.mappings",
            "rekordboxedit_0000000000000.mappings",
        ] {
            std::fs::write(mappings.join(name), name).unwrap();
        }

        create(&lib).assert().code(0);

        let names = zip_names(&lib.out);
        let n = names.len();
        assert_eq!(
            names[n - 3..n - 1],
            [
                "KeyMappings/rekordbox_0000000000000.mappings",
                "KeyMappings/rekordbox_9999999999999.mappings",
            ]
        );
        assert!(names[n - 4].starts_with("share/"));
    }

    /// rekordbox's own backup skipped 14 artwork folders that tracks named but were gone.
    #[tokio::test]
    async fn reports_files_and_folders_that_are_missing_on_disk() {
        let lib = library().await;
        std::fs::remove_file(lib.settings.join("DEVSETTING.DAT")).unwrap();
        let pool = crate::common::open_pool(&lib.db).await;
        sqlx::query("UPDATE djmdContent SET AnalysisDataPath = ?, ImagePath = ? WHERE ID = '101'")
            .bind("/PIONEER/USBANLZ/aaa/gone/ANLZ0000.DAT")
            .bind("/PIONEER/Artwork/aaa/gone/artwork.jpg")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;

        let assert = create(&lib).assert().code(0);

        let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
        assert_eq!(
            json["result"]["missing"],
            serde_json::json!([
                "DEVSETTING.DAT",
                "share/PIONEER/USBANLZ/aaa/gone",
                "share/PIONEER/Artwork/aaa/gone",
            ])
        );
        assert!(!zip_names(&lib.out).contains(&"DEVSETTING.DAT".to_string()));
    }

    /// Under WSL: master.db is F:\PIONEER\Master to rekordbox and /mnt/f/PIONEER/Master to rbx.
    #[tokio::test]
    async fn finds_the_settings_dir_through_the_windows_drive_mounts() {
        let lib = library().await;
        let root = lib.db.parent().unwrap().join("mnt");
        let db_dir = root.join("f").join("PIONEER").join("Master");
        std::fs::create_dir_all(&db_dir).unwrap();
        let db = db_dir.join("master.db");
        std::fs::copy(&lib.db, &db).unwrap();
        let settings = root.join("c/Users/dj/AppData/Roaming/Pioneer/rekordbox6");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(settings.join("rekordbox3.settings"), "on C").unwrap();
        set_registry(
            &db,
            "SyncSettingsRootPath",
            r"C:\Users\dj\AppData\Roaming\Pioneer\rekordbox6",
        )
        .await;
        set_registry(&db, "SyncAnalysisDataRootPath", r"F:\PIONEER\Master\share").await;

        rbx_cmd(&db)
            .args(["backup", "create"])
            .arg(&lib.out)
            .arg("--like")
            .arg(&lib.like)
            .arg("--execute")
            .assert()
            .code(0);

        assert_eq!(zip_read(&lib.out, "rekordbox3.settings"), b"on C");
    }

    #[tokio::test]
    async fn settings_dir_flag_overrides_agent_registry() {
        let lib = library().await;
        set_registry(&lib.db, "SyncSettingsRootPath", "/nowhere/rekordbox6").await;

        let assert = create(&lib)
            .arg("--settings-dir")
            .arg(&lib.settings)
            .assert()
            .code(0);

        let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
        assert_eq!(json["result"]["missing"], serde_json::json!([]));
        assert!(zip_names(&lib.out).contains(&"rekordbox3.settings".to_string()));
    }

    #[tokio::test]
    async fn refuses_to_overwrite_an_existing_file() {
        let lib = library().await;
        std::fs::write(&lib.out, "an older backup").unwrap();

        let assert = create(&lib).assert().code(5);

        let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
        assert_eq!(json["error"]["category"], "conflict");
        assert_eq!(std::fs::read(&lib.out).unwrap(), b"an older backup");
    }

    /// A non-empty master.db-wal holds changes that are not in master.db yet:
    /// rekordbox is open, or did not close cleanly.
    #[tokio::test]
    async fn refuses_while_master_db_has_a_write_ahead_log() {
        let lib = library().await;
        let wal = lib.db.with_file_name("master.db-wal");
        std::fs::write(&wal, "pending pages").unwrap();

        let assert = create(&lib).assert().code(5);

        let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
        assert_eq!(json["error"]["category"], "conflict");
        assert!(!lib.out.exists());

        std::fs::write(&wal, "").unwrap();
        create(&lib).assert().code(0);
    }

    #[tokio::test]
    async fn fails_when_the_like_backup_has_no_contents_xml() {
        let lib = library().await;
        std::fs::remove_file(&lib.like).unwrap();
        write_zip(&lib.like, &[("master.db", b"not a backup")]);

        let assert = create(&lib).assert().code(4);

        let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout).unwrap();
        assert!(json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("contents.xml"));
        assert!(!lib.out.exists());
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

    /// rekordbox lists the roots in djmdContent row order.
    #[test]
    fn lists_roots_in_the_order_they_first_appear() {
        let paths = [
            "F:/DJ/01.m4a",
            "D:/My Music/02.m4a",
            "F:/DJ/03.m4a",
            "C:/Users/dj/04.mp3",
        ];
        assert_eq!(
            directory_array(paths, &USER_DIRS),
            ["F:", "D:/My Music", "C:/Users/dj"]
        );
    }

    #[test]
    fn the_longest_matching_user_directory_wins() {
        let user_dirs = ["C:/Users/dj", "C:/Users/dj/Music"];
        let paths = ["C:/Users/dj/Music/01.mp3"];
        assert_eq!(directory_array(paths, &user_dirs), ["C:/Users/dj/Music"]);
    }

    #[test]
    fn a_directory_does_not_match_a_longer_name_that_starts_with_it() {
        let paths = ["D:/My Music2/01.mp3"];
        assert_eq!(directory_array(paths, &USER_DIRS), ["D:"]);
    }

    #[test]
    fn streaming_tracks_are_not_files() {
        let paths = [
            "spotify:track:09IStsImFySgyp0pIQdqAc",
            "apple-music:tracks:1456063107",
            "F:/DJ/01.m4a",
        ];
        assert_eq!(directory_array(paths, &USER_DIRS), ["F:"]);
    }
}

mod local_path {
    use std::path::{Path, PathBuf};

    use rbx::backup::local_path;

    /// agentRegistry has Windows paths. Under WSL, master.db at F:\PIONEER\Master
    /// is opened as /mnt/f/PIONEER/Master/master.db, which tells where drives are mounted.
    #[test]
    fn maps_a_windows_path_through_the_mount_the_db_drive_uses() {
        let path = local_path(
            r"C:\Users\dj\AppData\Roaming\Pioneer\rekordbox6",
            r"F:\PIONEER\Master",
            Path::new("/mnt/windows/f/PIONEER/Master"),
        );
        assert_eq!(
            path,
            Ok(PathBuf::from(
                "/mnt/windows/c/Users/dj/AppData/Roaming/Pioneer/rekordbox6"
            ))
        );
    }

    /// rekordbox on macOS stores POSIX paths.
    #[test]
    fn keeps_a_posix_path_as_it_is() {
        let path = local_path(
            "/Users/dj/Library/Pioneer/rekordbox6",
            "/Users/dj/Library/Pioneer/rekordbox",
            Path::new("/Users/dj/Library/Pioneer/rekordbox"),
        );
        assert_eq!(
            path,
            Ok(PathBuf::from("/Users/dj/Library/Pioneer/rekordbox6"))
        );
    }

    #[test]
    fn drive_letters_match_in_either_case() {
        let path = local_path(
            r"c:\Users\dj",
            r"f:\PIONEER\Master",
            Path::new("/mnt/F/PIONEER/Master"),
        );
        assert_eq!(path, Ok(PathBuf::from("/mnt/C/Users/dj")));
    }

    #[test]
    fn fails_when_the_db_path_does_not_end_with_the_windows_db_dir() {
        for local in ["/data/rekordbox", "/mnt/d/PIONEER/Master"] {
            let path = local_path(r"C:\Users\dj", r"F:\PIONEER\Master", Path::new(local));
            assert!(path.is_err(), "{}", local);
        }
    }
}

mod manifest {
    use rbx::backup::manifest;

    const TEMPLATE: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\r\n\r\n\
        <PROPERTIES>\r\n\
        \x20 <VALUE name=\"DB_ID\" val=\"-1095663931\"/>\r\n\
        \x20 <VALUE name=\"CONTENT_NUM\" val=\"10668\"/>\r\n\
        \x20 <VALUE name=\"BACKUP_RB_VER\" val=\"70218\"/>\r\n\
        \x20 <VALUE name=\"DIRECTORY_ARRAY\" val=\"F:,D:\"/>\r\n\
        </PROPERTIES>\r\n";

    #[test]
    fn replaces_the_library_values_and_keeps_every_other_byte() {
        let out = manifest(
            TEMPLATE,
            &[
                ("DB_ID", "7".to_string()),
                ("CONTENT_NUM", "2".to_string()),
                ("DIRECTORY_ARRAY", "D:/My Music".to_string()),
            ],
        );
        let expected = TEMPLATE
            .replace("\"-1095663931\"", "\"7\"")
            .replace("\"10668\"", "\"2\"")
            .replace("\"F:,D:\"", "\"D:/My Music\"");
        assert_eq!(out, expected);
    }

    /// djmdProperty.DBID 3199303365 was DB_ID -1095663931 in rekordbox's backup.
    #[test]
    fn db_id_is_the_dbid_as_a_signed_32_bit_number() {
        assert_eq!(rbx::backup::db_id("3199303365"), Some(-1095663931));
        assert_eq!(rbx::backup::db_id("12345"), Some(12345));
        assert_eq!(rbx::backup::db_id("x"), None);
    }

    #[test]
    fn escapes_xml_special_characters_in_values() {
        let out = manifest(
            TEMPLATE,
            &[("DIRECTORY_ARRAY", "D:/R&B <\"x\">".to_string())],
        );
        assert!(out.contains("val=\"D:/R&amp;B &lt;&quot;x&quot;&gt;\""));
    }
}
