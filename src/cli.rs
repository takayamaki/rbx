use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "rbx",
    about = "CLI tool for rekordbox master.db",
    after_help = "For machine-readable schema info, use: rbx describe [resource] [action]"
)]
pub(crate) struct Cli {
    /// Path to rekordbox master.db
    #[arg(long, env = "RBX_DB_PATH")]
    pub(crate) db: Option<PathBuf>,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// Query and manage tracks
    Tracks {
        #[command(subcommand)]
        action: TracksAction,
    },
    /// Query playlists
    Playlists {
        #[command(subcommand)]
        action: PlaylistsAction,
    },
    /// Manage My Tags (categories and tags themselves)
    Mytags {
        #[command(subcommand)]
        action: MytagsAction,
    },
    /// Query play history
    History {
        #[command(subcommand)]
        action: HistoryAction,
    },
    /// Run raw SQL query (read-only by default)
    Query {
        /// SQL to execute
        sql: String,
        /// Allow writes. Bypasses all rekordbox invariants (USN allocation,
        /// timestamp format, numeric IDs, masterPlaylists6.xml sync) —
        /// prefer the dedicated commands
        #[arg(long)]
        unsafe_write: bool,
    },
    /// Describe available commands and their schemas
    Describe {
        /// Resource name (e.g. "tracks", "playlists", "mytags")
        resource: Option<String>,
        /// Action name (e.g. "list", "get", "search")
        action: Option<String>,
    },
}

#[derive(Subcommand)]
pub(crate) enum TracksAction {
    /// List all tracks
    List,
    /// Get a track by ID
    Get { id: String },
    /// Search tracks by title or artist
    Search { query: String },
    /// Filter tracks by BPM, key, and/or tag
    Filter {
        #[arg(long)]
        bpm_min: Option<f64>,
        #[arg(long)]
        bpm_max: Option<f64>,
        /// Musical key (e.g. "8A")
        #[arg(long)]
        key: Option<String>,
        /// My Tag ID to filter by
        #[arg(long)]
        tag: Option<String>,
    },
    /// Update track fields (dry-run by default)
    Update {
        /// Track ID
        id: String,
        #[arg(long)]
        title: Option<String>,
        /// Artist name (resolved or created in djmdArtist; "" clears)
        #[arg(long)]
        artist: Option<String>,
        /// Genre name (resolved or created in djmdGenre; "" clears)
        #[arg(long)]
        genre: Option<String>,
        /// Album name (resolved or created in djmdAlbum; "" clears)
        #[arg(long)]
        album: Option<String>,
        /// Track number within the disc
        #[arg(long)]
        track_no: Option<i32>,
        /// Disc number
        #[arg(long)]
        disc_no: Option<i32>,
        /// Release year (e.g. 2018)
        #[arg(long)]
        year: Option<i32>,
        /// Full file path as rekordbox stores it (e.g. "F:/Music/a.m4a"); FileNameL follows
        #[arg(long)]
        path: Option<String>,
        /// BPM as decimal (e.g. 128.0)
        #[arg(long)]
        bpm: Option<f64>,
        /// Musical key (e.g. "8A", "1B")
        #[arg(long)]
        key: Option<String>,
        /// Rating (0-5)
        #[arg(long)]
        rating: Option<i32>,
        /// Comment text
        #[arg(long)]
        comment: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Update many tracks from a JSON plan file (dry-run by default)
    BulkUpdate {
        /// Plan file: [{"id": "...", "fields": {"title": "...", ...}}, ...]. "-" reads stdin
        file: String,
        /// Actually apply the changes
        #[arg(long)]
        execute: bool,
    },
    /// Manage My Tags on a track
    Mytags {
        #[command(subcommand)]
        action: TrackMytagsAction,
    },
    /// Manage cue points on a track
    Cues {
        #[command(subcommand)]
        action: TrackCuesAction,
    },
}

#[derive(Subcommand)]
pub(crate) enum TrackCuesAction {
    /// List cue points on a track
    List { track_id: String },
    /// Add a cue point (dry-run by default)
    Add {
        track_id: String,
        /// Position in milliseconds
        msec: i64,
        /// Cue type: "memory" (default) or "hot"
        #[arg(long, default_value = "memory")]
        kind: String,
        /// Hot cue slot (1-8, required for hot cues)
        #[arg(long)]
        slot: Option<i32>,
        /// Cue comment/name
        #[arg(long)]
        comment: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Update a cue point (dry-run by default)
    Update {
        /// Cue ID
        cue_id: String,
        /// Position in milliseconds
        #[arg(long)]
        msec: Option<i64>,
        /// Cue comment/name
        #[arg(long)]
        comment: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Delete a cue point (dry-run by default)
    Delete {
        /// Cue ID
        cue_id: String,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum TrackMytagsAction {
    /// List My Tags assigned to a track
    List { track_id: String },
    /// Add My Tags to a track (dry-run by default)
    Add {
        track_id: String,
        /// One or more tag IDs
        tag_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Remove My Tags from a track (dry-run by default)
    Remove {
        track_id: String,
        /// One or more tag IDs
        tag_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum PlaylistsAction {
    /// List all playlists (folders and playlists)
    List,
    /// Manage tracks in a playlist
    Tracks {
        #[command(subcommand)]
        action: PlaylistTracksAction,
    },
    /// Find playlists containing a specific track
    Search { track_id: String },
    /// Create a new playlist (dry-run by default)
    Create {
        /// Playlist name
        name: String,
        /// Parent folder ID (omit for top-level)
        #[arg(long)]
        parent: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Delete a playlist (dry-run by default)
    Delete {
        id: String,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum PlaylistTracksAction {
    /// List tracks in a playlist
    List { playlist_id: String },
    /// Add tracks to a playlist (dry-run by default)
    Add {
        playlist_id: String,
        /// One or more track IDs
        track_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Remove tracks from a playlist (dry-run by default)
    Remove {
        playlist_id: String,
        /// One or more track IDs
        track_ids: Vec<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum HistoryAction {
    /// List play history sessions
    List {
        /// Max number of sessions to return
        #[arg(long, default_value = "20")]
        limit: i32,
    },
    /// List tracks in a history session
    Tracks { id: String },
}

#[derive(Subcommand)]
pub(crate) enum MytagsAction {
    /// List all My Tags (categories and tags)
    List,
    /// List tracks with a specific My Tag
    Tracks { id: String },
    /// Create a new My Tag (dry-run by default)
    Create {
        /// Tag name
        name: String,
        /// Parent category ID (required for tags, omit for top-level categories)
        #[arg(long)]
        parent: Option<String>,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
    /// Delete a My Tag (dry-run by default)
    Delete {
        /// My Tag ID
        id: String,
        /// Actually apply the change
        #[arg(long)]
        execute: bool,
    },
}

pub(crate) fn needs_write(cmd: &Commands) -> bool {
    matches!(
        cmd,
        Commands::Query {
            unsafe_write: true,
            ..
        } | Commands::Tracks {
            action: TracksAction::Update { execute: true, .. }
                | TracksAction::BulkUpdate { execute: true, .. }
                | TracksAction::Mytags {
                    action: TrackMytagsAction::Add { execute: true, .. }
                        | TrackMytagsAction::Remove { execute: true, .. },
                }
                | TracksAction::Cues {
                    action: TrackCuesAction::Add { execute: true, .. }
                        | TrackCuesAction::Update { execute: true, .. }
                        | TrackCuesAction::Delete { execute: true, .. },
                }
        } | Commands::Mytags {
            action: MytagsAction::Create { execute: true, .. }
                | MytagsAction::Delete { execute: true, .. },
        } | Commands::Playlists {
            action: PlaylistsAction::Create { execute: true, .. }
                | PlaylistsAction::Delete { execute: true, .. }
                | PlaylistsAction::Tracks {
                    action: PlaylistTracksAction::Add { execute: true, .. }
                        | PlaylistTracksAction::Remove { execute: true, .. }
                },
        }
    )
}
