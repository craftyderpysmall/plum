//! Chunk framing.
//!
//! ```text
//! header : magic[8] = "PLUM\x89\r\n\x1a" | major u16 | minor u16 | flags u32     (16 bytes, LE)
//! chunk  : tag[4] | flags u8 | len u32 | stored[len] | crc32(stored) u32
//! end    : tag "END\0" | flags 0 | len 0 | crc32("")
//! ```
//! Chunk flag bit 0: `stored` is LZ4 compressed (size-prefixed block).

use std::io::{self, Read, Write};

use crate::FormatError;

pub const MAGIC: [u8; 8] = *b"PLUM\x89\r\n\x1a";
pub const MAJOR: u16 = 1;
pub const MINOR: u16 = 1;
pub const END: [u8; 4] = *b"END\0";

pub const FLAG_LZ4: u8 = 1;

/// Payloads larger than this get LZ4-compressed when it actually helps.
pub const COMPRESS_THRESHOLD: usize = 4096;

pub fn write_header(w: &mut impl Write) -> io::Result<()> {
    w.write_all(&MAGIC)?;
    w.write_all(&MAJOR.to_le_bytes())?;
    w.write_all(&MINOR.to_le_bytes())?;
    w.write_all(&0u32.to_le_bytes())
}

/// Returns `(major, minor)`.
pub fn read_header(r: &mut impl Read) -> Result<(u16, u16), FormatError> {
    let mut buf = [0u8; 16];
    r.read_exact(&mut buf).map_err(|_| FormatError::BadMagic)?;
    if buf[..8] != MAGIC {
        return Err(FormatError::BadMagic);
    }
    let major = u16::from_le_bytes([buf[8], buf[9]]);
    let minor = u16::from_le_bytes([buf[10], buf[11]]);
    if major != MAJOR {
        return Err(FormatError::UnsupportedVersion { major, minor });
    }
    Ok((major, minor))
}

pub fn write_chunk(w: &mut impl Write, tag: [u8; 4], payload: &[u8]) -> io::Result<()> {
    let compressed = (payload.len() > COMPRESS_THRESHOLD).then(|| lz4_flex::compress_prepend_size(payload));
    let (flags, stored) = match &compressed {
        Some(c) if c.len() < payload.len() => (FLAG_LZ4, c.as_slice()),
        _ => (0, payload),
    };
    let len = u32::try_from(stored.len()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "chunk exceeds 4 GiB"))?;
    w.write_all(&tag)?;
    w.write_all(&[flags])?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(stored)?;
    w.write_all(&crc32fast::hash(stored).to_le_bytes())
}

pub fn write_end(w: &mut impl Write) -> io::Result<()> {
    write_chunk(w, END, &[])
}

/// A chunk as read from disk.
pub enum ChunkRead {
    Ok { tag: [u8; 4], payload: Vec<u8> },
    /// Framing was intact but the CRC or decompression failed; the reader can continue.
    Corrupt { tag: [u8; 4], reason: String },
    End,
}

pub fn read_chunk(r: &mut impl Read) -> Result<ChunkRead, FormatError> {
    let mut head = [0u8; 9];
    r.read_exact(&mut head).map_err(|_| FormatError::Truncated)?;
    let tag = [head[0], head[1], head[2], head[3]];
    let flags = head[4];
    let len = u32::from_le_bytes([head[5], head[6], head[7], head[8]]) as usize;
    let mut stored = vec![0u8; len];
    r.read_exact(&mut stored).map_err(|_| FormatError::Truncated)?;
    let mut crc = [0u8; 4];
    r.read_exact(&mut crc).map_err(|_| FormatError::Truncated)?;
    if tag == END {
        return Ok(ChunkRead::End);
    }
    if u32::from_le_bytes(crc) != crc32fast::hash(&stored) {
        return Ok(ChunkRead::Corrupt { tag, reason: "checksum mismatch".into() });
    }
    if flags & FLAG_LZ4 != 0 {
        return Ok(match lz4_flex::decompress_size_prepended(&stored) {
            Ok(payload) => ChunkRead::Ok { tag, payload },
            Err(e) => ChunkRead::Corrupt { tag, reason: format!("lz4: {e}") },
        });
    }
    Ok(ChunkRead::Ok { tag, payload: stored })
}

pub fn tag_str(tag: &[u8; 4]) -> String {
    tag.iter().map(|&b| if b.is_ascii_graphic() { b as char } else { '?' }).collect()
}
