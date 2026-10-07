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
