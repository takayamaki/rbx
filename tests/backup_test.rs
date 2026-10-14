//! `rbx backup create`: a library backup in the same shape as the one
//! rekordbox writes from Preferences → Advanced → Database → Library backup.
//!
//! Checked against a backup made by rekordbox 7.2.18 (44,614 entries).

mod common;

mod create {
    #[tokio::test]
    #[ignore = "todo"]
    async fn zips_master_db_and_the_library_files_beside_it() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn dry_run_lists_what_would_be_zipped_and_writes_nothing() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn zips_the_settings_files_from_the_settings_dir_in_agent_registry() {
        todo!()
    }

    #[tokio::test]
    #[ignore = "todo"]
    async fn zips_the_analysis_and_artwork_folders_that_tracks_refer_to() {
        todo!()
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
    #[test]
    #[ignore = "todo"]
    fn files_under_a_user_directory_are_grouped_by_that_directory() {
        todo!()
    }

    #[test]
    #[ignore = "todo"]
    fn files_outside_user_directories_are_grouped_by_drive() {
        todo!()
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
