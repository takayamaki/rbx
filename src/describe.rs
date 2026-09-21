use rbx::output;

pub(crate) fn handle_describe(
    resource: Option<String>,
    action: Option<String>,
) -> serde_json::Value {
    match (resource.as_deref(), action.as_deref()) {
        (None, _) => describe_root(),

        // tracks
        (Some("tracks"), None) => describe_resource("tracks", &[
            ("list", "List all tracks (excludes streaming-only)"),
            ("get", "Get a single track by ID"),
            ("search", "Search tracks by title or artist name"),
            ("filter", "Filter tracks by BPM range, key, and/or tag"),
            ("update", "Update track fields (dry-run by default)"),
            ("cues list", "List cue points on a track"),
            ("cues add", "Add a cue point (dry-run by default)"),
            ("cues update", "Update a cue point (dry-run by default)"),
            ("cues delete", "Delete a cue point (dry-run by default)"),
            ("mytags list", "List My Tags assigned to a track"),
            ("mytags add", "Add a My Tag to a track (dry-run by default)"),
            ("mytags remove", "Remove a My Tag from a track (dry-run by default)"),
        ]),
        (Some("tracks"), Some("list")) => describe_command("tracks list", &[], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &["rbx tracks list"]),
        (Some("tracks"), Some("get")) => describe_command("tracks get", &[
            flag("id", "string", true, "Track ID (from djmdContent.ID)"),
        ], &track_schema(), &["rbx tracks get 12345"]),
        (Some("tracks"), Some("search")) => describe_command("tracks search", &[
            flag("query", "string", true, "Search term (matched against title and artist)"),
        ], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &["rbx tracks search 'Butterfly'"]),
        (Some("tracks"), Some("filter")) => describe_command("tracks filter", &[
            flag("--bpm-min", "number", false, "Minimum BPM (inclusive)"),
            flag("--bpm-max", "number", false, "Maximum BPM (inclusive)"),
            flag("--key", "string", false, "Musical key (e.g. '8A')"),
            flag("--tag", "string", false, "My Tag ID to filter by"),
        ], &serde_json::json!({
            "type": "array", "items": track_schema(),
        }), &[
            "rbx tracks filter --bpm-min 125 --bpm-max 135",
            "rbx tracks filter --key 8A --tag TAG_ID",
            "rbx tracks filter --bpm-min 120 --bpm-max 140 --key 8A",
        ]),
        (Some("tracks"), Some("update")) => describe_command("tracks update", &[
            flag("id", "string", true, "Track ID"),
            flag("--title", "string", false, "Track title"),
            flag("--artist", "string", false, "Artist name (resolved or created in djmdArtist; \"\" clears)"),
            flag("--genre", "string", false, "Genre name (resolved or created in djmdGenre; \"\" clears)"),
            flag("--album", "string", false, "Album name (resolved or created in djmdAlbum; \"\" clears)"),
            flag("--track-no", "integer", false, "Track number within the disc"),
            flag("--disc-no", "integer", false, "Disc number"),
            flag("--year", "integer", false, "Release year"),
            flag("--path", "string", false, "Full file path as rekordbox stores it (FolderPath; FileNameL follows its basename)"),
            flag("--bpm", "number", false, "BPM as decimal (e.g. 128.0)"),
            flag("--key", "string", false, "Musical key (e.g. '8A', '1B')"),
            flag("--rating", "integer", false, "Rating (0-5)"),
            flag("--comment", "string", false, "Comment text"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.update"), &[
            "rbx tracks update TRACK_ID --title 'New Title' --bpm 128.0",
            "rbx tracks update TRACK_ID --artist 'Artist' --key '8A' --execute",
        ]),
        (Some("tracks"), Some("cues list")) => describe_command("tracks cues list", &[
            flag("track_id", "string", true, "Track ID"),
        ], &serde_json::json!({
            "type": "array", "items": cue_schema(),
        }), &["rbx tracks cues list TRACK_ID"]),
        (Some("tracks"), Some("cues add")) => describe_command("tracks cues add", &[
            flag("track_id", "string", true, "Track ID"),
            flag("msec", "integer", true, "Position in milliseconds"),
            flag("--kind", "string", false, "Cue type: 'memory' (default) or 'hot'"),
            flag("--slot", "integer", false, "Hot cue slot (1-8, required for hot cues)"),
            flag("--comment", "string", false, "Cue comment/name"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.cues.add"), &[
            "rbx tracks cues add TRACK_ID 12345",
            "rbx tracks cues add TRACK_ID 12345 --kind hot --slot 1 --comment 'Drop' --execute",
        ]),
        (Some("tracks"), Some("cues update")) => describe_command("tracks cues update", &[
            flag("cue_id", "string", true, "Cue ID"),
            flag("--msec", "integer", false, "New position in milliseconds"),
            flag("--comment", "string", false, "New comment"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.cues.update"), &[
            "rbx tracks cues update CUE_ID --msec 15000 --comment 'Verse'",
            "rbx tracks cues update CUE_ID --comment 'Chorus' --execute",
        ]),
        (Some("tracks"), Some("cues delete")) => describe_command("tracks cues delete", &[
            flag("cue_id", "string", true, "Cue ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.cues.delete"), &[
            "rbx tracks cues delete CUE_ID",
            "rbx tracks cues delete CUE_ID --execute",
        ]),
        (Some("tracks"), Some("mytags list")) => describe_command("tracks mytags list", &[
            flag("track_id", "string", true, "Track ID"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "tag_id": { "type": "string" },
                    "tag_name": { "type": "string|null" },
                    "category_name": { "type": "string|null" },
                },
            },
        }), &["rbx tracks mytags list TRACK_ID"]),
        (Some("tracks"), Some("mytags add")) => describe_command("tracks mytags add", &[
            flag("track_id", "string", true, "Track ID"),
            flag("tag_id", "string", true, "My Tag ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.mytags.add"), &[
            "rbx tracks mytags add TRACK_ID TAG_ID",
            "rbx tracks mytags add TRACK_ID TAG_ID --execute",
        ]),
        (Some("tracks"), Some("mytags remove")) => describe_command("tracks mytags remove", &[
            flag("track_id", "string", true, "Track ID"),
            flag("tag_id", "string", true, "My Tag ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("tracks.mytags.remove"), &[
            "rbx tracks mytags remove TRACK_ID TAG_ID",
            "rbx tracks mytags remove TRACK_ID TAG_ID --execute",
        ]),

        // playlists
        (Some("playlists"), None) => describe_resource("playlists", &[
            ("list", "List all playlists and folders"),
            ("tracks list", "List tracks in a specific playlist"),
            ("tracks add", "Add a track to a playlist (dry-run by default)"),
            ("tracks remove", "Remove a track from a playlist (dry-run by default)"),
            ("search", "Find playlists containing a specific track"),
            ("create", "Create a new playlist (dry-run by default)"),
            ("delete", "Delete a playlist (dry-run by default)"),
        ]),
        (Some("playlists"), Some("list")) => describe_command("playlists list", &[], &serde_json::json!({
            "type": "array", "items": playlist_schema(),
        }), &["rbx playlists list"]),
        (Some("playlists"), Some("tracks list")) => describe_command("playlists tracks list", &[
            flag("playlist_id", "string", true, "Playlist ID"),
        ], &serde_json::json!({
            "type": "array", "items": playlist_track_schema(),
        }), &["rbx playlists tracks list PLAYLIST_ID"]),
        (Some("playlists"), Some("tracks add")) => describe_command("playlists tracks add", &[
            flag("playlist_id", "string", true, "Playlist ID"),
            flag("track_id", "string", true, "Track ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.tracks.add"), &[
            "rbx playlists tracks add PLAYLIST_ID TRACK_ID",
            "rbx playlists tracks add PLAYLIST_ID TRACK_ID --execute",
        ]),
        (Some("playlists"), Some("tracks remove")) => describe_command("playlists tracks remove", &[
            flag("playlist_id", "string", true, "Playlist ID"),
            flag("track_id", "string", true, "Track ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.tracks.remove"), &[
            "rbx playlists tracks remove PLAYLIST_ID TRACK_ID",
            "rbx playlists tracks remove PLAYLIST_ID TRACK_ID --execute",
        ]),
        (Some("playlists"), Some("search")) => describe_command("playlists search", &[
            flag("track_id", "string", true, "Track ID to search for"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "playlist_id": { "type": "string" },
                    "playlist_name": { "type": "string" },
                    "track_no": { "type": "integer" },
                },
            },
        }), &["rbx playlists search TRACK_ID"]),
        (Some("playlists"), Some("create")) => describe_command("playlists create", &[
            flag("name", "string", true, "Playlist name"),
            flag("--parent", "string", false, "Parent folder ID (omit for top-level)"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.create"), &[
            "rbx playlists create 'My Playlist'",
            "rbx playlists create 'My Playlist' --parent FOLDER_ID --execute",
        ]),
        (Some("playlists"), Some("delete")) => describe_command("playlists delete", &[
            flag("id", "string", true, "Playlist ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("playlists.delete"), &[
            "rbx playlists delete PLAYLIST_ID",
            "rbx playlists delete PLAYLIST_ID --execute",
        ]),

        // mytags
        (Some("mytags"), None) => describe_resource("mytags", &[
            ("list", "List all My Tag categories and tags"),
            ("tracks", "List tracks with a specific My Tag"),
            ("create", "Create a new My Tag or category (dry-run by default)"),
            ("delete", "Delete a My Tag (dry-run by default)"),
        ]),
        (Some("mytags"), Some("list")) => describe_command("mytags list", &[], &serde_json::json!({
            "type": "array", "items": mytag_schema(),
        }), &["rbx mytags list"]),
        (Some("mytags"), Some("tracks")) => describe_command("mytags tracks", &[
            flag("id", "string", true, "My Tag ID"),
        ], &serde_json::json!({
            "type": "array", "items": mytag_track_schema(),
        }), &["rbx mytags tracks 12345"]),
        (Some("mytags"), Some("create")) => describe_command("mytags create", &[
            flag("name", "string", true, "Tag name"),
            flag("--parent", "string", false, "Parent category ID (omit for top-level category)"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("mytags.create"), &[
            "rbx mytags create 'My Category'",
            "rbx mytags create 'My Tag' --parent CATEGORY_ID --execute",
        ]),
        (Some("mytags"), Some("delete")) => describe_command("mytags delete", &[
            flag("id", "string", true, "My Tag ID"),
            flag("--execute", "bool", false, "Actually apply the change (default: dry-run)"),
        ], &mutation_result_schema("mytags.delete"), &[
            "rbx mytags delete TAG_ID",
            "rbx mytags delete TAG_ID --execute",
        ]),

        // history
        (Some("history"), None) => describe_resource("history", &[
            ("list", "List play history sessions (most recent first)"),
            ("tracks", "List tracks in a history session"),
        ]),
        (Some("history"), Some("list")) => describe_command("history list", &[
            flag("--limit", "integer", false, "Max sessions to return (default: 20)"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "date": { "type": "string" },
                    "track_count": { "type": "integer" },
                },
            },
        }), &["rbx history list", "rbx history list --limit 5"]),
        (Some("history"), Some("tracks")) => describe_command("history tracks", &[
            flag("id", "string", true, "History session ID"),
        ], &serde_json::json!({
            "type": "array", "items": {
                "type": "object",
                "properties": {
                    "id": { "type": "string" },
                    "title": { "type": "string|null" },
                    "artist": { "type": "string|null" },
                    "bpm": { "type": "number|null" },
                    "key": { "type": "string|null" },
                },
            },
        }), &["rbx history tracks HISTORY_ID"]),

        // query
        (Some("query"), None) | (Some("query"), Some(_)) => describe_command("query", &[
            flag("sql", "string", true,
                "SQL to execute. Read-only by default: single SELECT / WITH / PRAGMA / EXPLAIN statement"),
            flag("--unsafe-write", "bool", false,
                "Allow arbitrary SQL incl. writes. Bypasses rekordbox invariants \
                 (USN allocation, timestamp format, numeric IDs, masterPlaylists6.xml sync) — \
                 prefer dedicated commands"),
        ], &serde_json::json!({
            "type": "array",
            "items": { "type": "object", "description": "Dynamic columns based on query" },
        }), &[
            "rbx query 'SELECT ID, Title FROM djmdContent LIMIT 5'",
            "rbx query 'PRAGMA table_info(djmdContent)'",
        ]),

        (Some(r), _) => output::error(
            "not_found", output::EXIT_NOT_FOUND,
            &format!("Unknown resource: {}", r),
            Some("Use 'rbx describe' to see available resources"),
        ),
    }
}

fn describe_root() -> serde_json::Value {
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "resources": [
            { "name": "tracks", "description": "Query tracks and manage their My Tag assignments" },
            { "name": "playlists", "description": "Query playlists and their contents" },
            { "name": "mytags", "description": "Manage My Tag categories and tags (CRUD)" },
            { "name": "history", "description": "Query play history sessions" },
            { "name": "query", "description": "Run raw SQL against master.db (read-only: SELECT/WITH/PRAGMA/EXPLAIN)" },
        ],
        "global_flags": [
            { "name": "--db", "type": "path", "required": true, "env": "RBX_DB_PATH",
              "description": "Path to rekordbox master.db" },
        ],
        "discovery_sequence": [
            "rbx describe",
            "rbx describe <resource>",
            "rbx describe <resource> <action>",
            "rbx <resource> <action> [args]",
        ],
    })
}

fn describe_resource(name: &str, actions: &[(&str, &str)]) -> serde_json::Value {
    let acts: Vec<_> = actions
        .iter()
        .map(|(n, d)| {
            serde_json::json!({
                "name": n, "description": d,
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "resource": name,
        "actions": acts,
    })
}

fn describe_command(
    command: &str,
    flags: &[serde_json::Value],
    output_schema: &serde_json::Value,
    examples: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "schema_version": output::SCHEMA_VERSION,
        "kind": "describe",
        "command": command,
        "flags": flags,
        "output_schema": output_schema,
        "examples": examples,
    })
}

fn flag(name: &str, typ: &str, required: bool, desc: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "type": typ, "required": required, "description": desc })
}

fn track_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "title": { "type": "string|null" },
            "artist": { "type": "string|null" },
            "duration_sec": { "type": "integer|null" },
            "bpm": { "type": "number|null", "description": "BPM as decimal (e.g. 128.0)" },
            "key": { "type": "string|null", "description": "Musical key (e.g. '8A', '1B')" },
            "folder_path": { "type": "string|null" },
        },
    })
}

fn playlist_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "name": { "type": "string|null" },
            "kind": { "type": "string", "enum": ["folder", "playlist"] },
            "parent_id": { "type": "string|null" },
        },
    })
}

fn playlist_track_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "track_no": { "type": "integer" },
            "id": { "type": "string" },
            "title": { "type": "string|null" },
            "artist": { "type": "string|null" },
            "bpm": { "type": "number|null" },
            "key": { "type": "string|null" },
        },
    })
}

fn mytag_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "seq": { "type": "integer|null" },
            "name": { "type": "string|null" },
            "kind": { "type": "string", "enum": ["category", "tag"] },
            "parent_id": { "type": "string|null" },
        },
    })
}

fn mytag_track_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "title": { "type": "string|null" },
            "artist": { "type": "string|null" },
            "bpm": { "type": "number|null" },
            "key": { "type": "string|null" },
        },
    })
}

fn cue_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "id": { "type": "string" },
            "track_id": { "type": "string" },
            "kind": { "type": "string", "enum": ["memory", "hot", "other"] },
            "slot": { "type": "integer|null", "description": "Hot cue slot (1-8), null for memory cues" },
            "in_msec": { "type": "integer|null", "description": "Cue position in milliseconds" },
            "out_msec": { "type": "integer|null", "description": "Loop end in milliseconds, null if not a loop" },
            "color": { "type": "integer|null" },
            "comment": { "type": "string|null" },
        },
    })
}

fn mutation_result_schema(kind: &str) -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "description": format!("When dry_run=true, includes 'plan' and 'next_step'. When dry_run=false, includes 'result'. kind='{}'", kind),
        "properties": {
            "dry_run": { "type": "boolean" },
            "plan": { "type": "object", "description": "Present when dry_run=true" },
            "result": { "type": "object", "description": "Present when dry_run=false" },
            "next_step": { "type": "string", "description": "Present when dry_run=true" },
        },
    })
}
