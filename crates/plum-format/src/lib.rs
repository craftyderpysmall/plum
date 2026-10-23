//! The `.plum` file format: a header followed by tagged, checksummed,
//! optionally LZ4-compressed chunks. See [`chunk`] for the byte layout and
//! [`codec`] for the extension points.
//!
//! Compatibility rules:
//! * different **major** version → refuse to load; different minor → fine.
//! * unknown chunk → kept verbatim in [`plum_core::BoardMeta::opaque_chunks`] and re-written on save.
//! * unknown element kind (or newer kind version) → loaded as
//!   [`plum_core::ElementKind::Unknown`], drawn as a placeholder, re-written unchanged.
//! * corrupt chunk (bad CRC) → skipped with a warning; the rest of the file loads.

pub mod chunk;
pub mod codec;
pub mod v1;

use std::fs;
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use plum_core::Board;

pub use codec::{ChunkCodec, ElementCodec, LoadCtx, Registry};

pub const EXTENSION: &str = "plum";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("i/o error: {0}")]
    Io(#[from] io::Error),
    #[error("not a .plum file")]
    BadMagic,
    #[error("unsupported .plum version {major}.{minor}")]
    UnsupportedVersion { major: u16, minor: u16 },
    #[error("file is truncated")]
    Truncated,
    #[error("decode error: {0}")]
    Decode(String),
}

/// A loaded board plus any non-fatal problems found while reading it.
pub struct Loaded {
    pub board: Board,
    pub warnings: Vec<String>,
}

pub fn write(board: &Board, w: &mut impl Write, reg: &Registry) -> Result<(), FormatError> {
    chunk::write_header(w)?;
    for codec in reg.chunk_codecs() {
        for payload in codec.write(board, reg)? {
            chunk::write_chunk(w, codec.tag(), &payload)?;
        }
    }
    for (tag, payload) in &board.meta.opaque_chunks {
        chunk::write_chunk(w, *tag, payload)?;
    }
    chunk::write_end(w)?;
    Ok(())
}

pub fn read(r: &mut impl Read, reg: &Registry) -> Result<Loaded, FormatError> {
    chunk::read_header(r)?;
    let mut ctx = LoadCtx::default();
    loop {
        match chunk::read_chunk(r) {
            Ok(chunk::ChunkRead::End) => break,
            Ok(chunk::ChunkRead::Corrupt { tag, reason }) => {
                ctx.warnings.push(format!("skipped corrupt {} chunk ({reason})", chunk::tag_str(&tag)));
            }
            Ok(chunk::ChunkRead::Ok { tag, payload }) => match reg.chunk(&tag) {
                Some(codec) => {
                    if let Err(e) = codec.read(&payload, &mut ctx, reg) {
                        ctx.warnings.push(format!("skipped {} chunk: {e}", chunk::tag_str(&tag)));
                    }
                }
                None => ctx.meta.opaque_chunks.push((tag, payload)),
            },
            // A file cut short still yields whatever was read before the cut.
            Err(FormatError::Truncated) => {
                ctx.warnings.push("file ended unexpectedly; loaded what was readable".into());
                break;
            }
            Err(e) => return Err(e),
        }
    }
    let LoadCtx { meta, elements, warnings } = ctx;
    Ok(Loaded { board: Board::from_elements(meta, elements), warnings })
}

/// Atomic save: write to a sibling temp file, then rename over the target.
pub fn save_to_path(board: &Board, path: &Path) -> Result<(), FormatError> {
    let tmp = path.with_extension("plum.tmp");
    {
        let mut w = BufWriter::new(fs::File::create(&tmp)?);
        write(board, &mut w, &Registry::default())?;
        w.flush()?;
        w.get_ref().sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })?;
    Ok(())
}

pub fn load_from_path(path: &Path) -> Result<Loaded, FormatError> {
    read(&mut BufReader::new(fs::File::open(path)?), &Registry::default())
}


#[cfg(test)]
mod tests;
