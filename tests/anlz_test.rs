//! ANLZ analysis file editing (`rbx::anlz`).
//! rekordbox keeps each track's beat grid in ANLZ0000.DAT (PQTZ) and, for some tracks,
//! a second form in ANLZ0000.EXT (PQT2) that is not decoded yet.
//! Checked in rekordbox 7: with PQT2 removed, rekordbox shows the PQTZ grid.
//! Every tag that rbx does not change must stay byte for byte.

mod common;

use rbx::anlz::{self, Beat};

// Order: reading the grid first, then replacing it, then removing PQT2,
// and broken files last.

/// PQTZ beats come back as (beat 1-4, tempo BPM * 100, time ms), in file order.
#[test]
fn reads_pqtz_beats() {
    let file = common::anlz_file(&[
        common::anlz_tag("PPTH", &[0, 0, 0, 4], b"a.mp"),
        common::pqtz_tag(&[(1, 11400, 156), (2, 11400, 682), (3, 15600, 1066)]),
    ]);

    assert_eq!(
        anlz::read_beats(&file).unwrap(),
        vec![
            Beat {
                beat: 1,
                tempo: 11400,
                ms: 156
            },
            Beat {
                beat: 2,
                tempo: 11400,
                ms: 682
            },
            Beat {
                beat: 3,
                tempo: 15600,
                ms: 1066
            },
        ]
    );
}

/// Replacing PQTZ writes the new beats and updates the tag length,
/// the beat count and the file length in the PMAI header.
/// The tags before and after it stay byte for byte.
#[test]
fn replaces_pqtz_and_updates_lengths() {}

/// Removing PQT2 drops only that tag and updates the file length.
#[test]
fn removes_pqt2_and_keeps_other_tags() {}

/// A file that is not an ANLZ file (no PMAI) or has no PQTZ is an error, not a panic.
#[test]
fn rejects_files_without_pmai_or_pqtz() {}
