use sqlx::FromRow;

#[derive(Debug, FromRow)]
pub(crate) struct TrackRow {
    id: String,
    title: Option<String>,
    artist_name: Option<String>,
    duration: Option<i32>,
    bpm: Option<i32>,
    key_name: Option<String>,
    folder_path: Option<String>,
}

impl TrackRow {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "title": self.title,
            "artist": self.artist_name,
            "duration_sec": self.duration,
            "bpm": self.bpm.map(|b| b as f64 / 100.0),
            "key": self.key_name,
            "folder_path": self.folder_path,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct PlaylistRow {
    id: String,
    name: Option<String>,
    attribute: Option<i32>,
    parent_id: Option<String>,
}

impl PlaylistRow {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let kind = if self.attribute == Some(1) {
            "folder"
        } else {
            "playlist"
        };
        serde_json::json!({
            "id": self.id,
            "name": self.name,
            "kind": kind,
            "parent_id": self.parent_id,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct PlaylistTrackRow {
    track_no: i32,
    content_id: String,
    title: Option<String>,
    artist_name: Option<String>,
    bpm: Option<i32>,
    key_name: Option<String>,
}

impl PlaylistTrackRow {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "track_no": self.track_no,
            "id": self.content_id,
            "title": self.title,
            "artist": self.artist_name,
            "bpm": self.bpm.map(|b| b as f64 / 100.0),
            "key": self.key_name,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct MyTagRow {
    id: String,
    seq: Option<i32>,
    name: Option<String>,
    attribute: Option<i32>,
    parent_id: Option<String>,
}

impl MyTagRow {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let kind = if self.attribute == Some(1) {
            "category"
        } else {
            "tag"
        };
        serde_json::json!({
            "id": self.id,
            "seq": self.seq,
            "name": self.name,
            "kind": kind,
            "parent_id": self.parent_id,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct MyTagTrackRow {
    content_id: String,
    title: Option<String>,
    artist_name: Option<String>,
    bpm: Option<i32>,
    key_name: Option<String>,
}

impl MyTagTrackRow {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.content_id,
            "title": self.title,
            "artist": self.artist_name,
            "bpm": self.bpm.map(|b| b as f64 / 100.0),
            "key": self.key_name,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct TrackMyTagRow {
    tag_id: String,
    tag_name: Option<String>,
    category_name: Option<String>,
}

impl TrackMyTagRow {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "tag_id": self.tag_id,
            "tag_name": self.tag_name,
            "category_name": self.category_name,
        })
    }
}

#[derive(Debug, FromRow)]
pub(crate) struct CueRow {
    id: String,
    content_id: String,
    in_msec: Option<i64>,
    out_msec: Option<i64>,
    kind: Option<i32>,
    color: Option<i32>,
    comment: Option<String>,
}

impl CueRow {
    pub(crate) fn to_json(&self) -> serde_json::Value {
        let kind_str = match self.kind {
            Some(0) => "memory",
            Some(k @ 1..=8) => {
                return serde_json::json!({
                    "id": self.id,
                    "track_id": self.content_id,
                    "kind": "hot",
                    "slot": k,
                    "in_msec": self.in_msec,
                    "out_msec": self.out_msec.filter(|&v| v >= 0),
                    "color": self.color.filter(|&v| v >= 0),
                    "comment": self.comment,
                })
            }
            _ => "other",
        };
        serde_json::json!({
            "id": self.id,
            "track_id": self.content_id,
            "kind": kind_str,
            "in_msec": self.in_msec,
            "out_msec": self.out_msec.filter(|&v| v >= 0),
            "color": self.color.filter(|&v| v >= 0),
            "comment": self.comment,
        })
    }
}
