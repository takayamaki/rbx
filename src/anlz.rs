//! rekordbox analysis files (ANLZ0000.DAT / .EXT).
//! A file is a PMAI header followed by tagged sections; every number is big-endian.
//! Each section starts with a fourcc, its header length and its total length.

/// One beat of a PQTZ beat grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// Position in the bar, 1-4
    pub beat: u16,
    /// Tempo at this beat, BPM * 100
    pub tempo: u16,
    /// Time of the beat in milliseconds
    pub ms: u32,
}

/// An ANLZ file that rbx cannot read.
#[derive(Debug, PartialEq, Eq)]
pub struct AnlzError(pub String);

impl std::fmt::Display for AnlzError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// One section of an ANLZ file, as byte ranges into the file.
struct Section {
    fourcc: [u8; 4],
    start: usize,
    len_header: usize,
    len_tag: usize,
}

impl Section {
    fn bytes<'a>(&self, file: &'a [u8]) -> &'a [u8] {
        &file[self.start..self.start + self.len_tag]
    }
}

fn u32_at(file: &[u8], at: usize) -> Result<u32, AnlzError> {
    file.get(at..at + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| {
            AnlzError(format!(
                "file ends at byte {} while reading a number",
                file.len()
            ))
        })
}

fn u16_at(file: &[u8], at: usize) -> Result<u16, AnlzError> {
    file.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| {
            AnlzError(format!(
                "file ends at byte {} while reading a number",
                file.len()
            ))
        })
}

/// Splits a file into its sections, after the PMAI header.
fn sections(file: &[u8]) -> Result<Vec<Section>, AnlzError> {
    let mut at = u32_at(file, 4)? as usize;
    let mut out = Vec::new();
    while at + 12 <= file.len() {
        let len_header = u32_at(file, at + 4)? as usize;
        let len_tag = u32_at(file, at + 8)? as usize;
        if len_tag < 12 || len_header > len_tag || at + len_tag > file.len() {
            return Err(AnlzError(format!("broken section at byte {}", at)));
        }
        let mut fourcc = [0u8; 4];
        fourcc.copy_from_slice(&file[at..at + 4]);
        out.push(Section {
            fourcc,
            start: at,
            len_header,
            len_tag,
        });
        at += len_tag;
    }
    Ok(out)
}

/// Reads the beats of the PQTZ beat grid.
pub fn read_beats(file: &[u8]) -> Result<Vec<Beat>, AnlzError> {
    let all = sections(file)?;
    let pqtz = all
        .iter()
        .find(|s| &s.fourcc == b"PQTZ")
        .ok_or_else(|| AnlzError("no PQTZ beat grid in this file".into()))?;
    let count = u32_at(file, pqtz.start + 0x14)? as usize;
    (0..count)
        .map(|i| {
            let at = pqtz.start + pqtz.len_header + i * 8;
            Ok(Beat {
                beat: u16_at(file, at)?,
                tempo: u16_at(file, at + 2)?,
                ms: u32_at(file, at + 4)?,
            })
        })
        .collect()
}

/// Rebuilds a file section by section. `edit` returns the new bytes of a section,
/// or None to drop it; the PMAI header is kept and its file length is updated.
fn rebuild(
    file: &[u8],
    mut edit: impl FnMut(&Section, &[u8]) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, AnlzError> {
    let header_len = u32_at(file, 4)? as usize;
    let mut out = file[..header_len].to_vec();
    for section in sections(file)? {
        if let Some(bytes) = edit(&section, section.bytes(file)) {
            out.extend(bytes);
        }
    }
    let len = out.len() as u32;
    out[8..12].copy_from_slice(&len.to_be_bytes());
    Ok(out)
}

/// Writes `beats` as the PQTZ beat grid, keeping every other section byte for byte.
pub fn replace_beats(file: &[u8], beats: &[Beat]) -> Result<Vec<u8>, AnlzError> {
    read_beats(file)?;
    rebuild(file, |section, bytes| {
        if &section.fourcc != b"PQTZ" {
            return Some(bytes.to_vec());
        }
        let mut tag = bytes[..section.len_header].to_vec();
        let len_tag = (section.len_header + beats.len() * 8) as u32;
        tag[8..12].copy_from_slice(&len_tag.to_be_bytes());
        tag[0x14..0x18].copy_from_slice(&(beats.len() as u32).to_be_bytes());
        for b in beats {
            tag.extend(b.beat.to_be_bytes());
            tag.extend(b.tempo.to_be_bytes());
            tag.extend(b.ms.to_be_bytes());
        }
        Some(tag)
    })
}
