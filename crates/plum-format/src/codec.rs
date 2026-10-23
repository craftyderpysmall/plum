//! Extension points. Every chunk type and every element kind is a codec
//! registered by 4-byte tag; new features add codecs instead of editing
//! existing ones.

use std::collections::HashMap;

use plum_core::{Board, BoardMeta, Element, ElementKind};

use crate::FormatError;

/// Encodes one element kind's payload (the part after the shared element header).
pub trait ElementCodec: Send + Sync {
    fn tag(&self) -> [u8; 4];
    /// Payload version written by this build; bump when the payload layout changes.
    fn version(&self) -> u8;
    fn handles(&self, kind: &ElementKind) -> bool;
    fn encode(&self, kind: &ElementKind) -> Result<Vec<u8>, FormatError>;
    /// Must accept every version up to and including [`Self::version`].
    fn decode(&self, version: u8, bytes: &[u8]) -> Result<ElementKind, FormatError>;
}

/// State accumulated while reading a file.
#[derive(Default)]
pub struct LoadCtx {
    pub meta: BoardMeta,
    pub elements: Vec<Element>,
    pub warnings: Vec<String>,
}

/// Encodes/decodes a top-level chunk.
pub trait ChunkCodec: Send + Sync {
    fn tag(&self) -> [u8; 4];
    /// Payloads to write for this board, in order (a codec may emit several chunks or none).
    fn write(&self, board: &Board, registry: &Registry) -> Result<Vec<Vec<u8>>, FormatError>;
    fn read(&self, payload: &[u8], ctx: &mut LoadCtx, registry: &Registry) -> Result<(), FormatError>;
}

/// Tag → codec tables. [`Registry::default`] holds the built-in v1 codecs.
pub struct Registry {
    chunks: Vec<Box<dyn ChunkCodec>>,
    chunk_by_tag: HashMap<[u8; 4], usize>,
    elements: Vec<Box<dyn ElementCodec>>,
    element_by_tag: HashMap<[u8; 4], usize>,
}

impl Registry {
    pub fn empty() -> Self {
        Self { chunks: Vec::new(), chunk_by_tag: HashMap::new(), elements: Vec::new(), element_by_tag: HashMap::new() }
    }

    /// Register a chunk codec. Chunks are written in registration order.
    pub fn register_chunk(&mut self, c: Box<dyn ChunkCodec>) -> &mut Self {
        self.chunk_by_tag.insert(c.tag(), self.chunks.len());
        self.chunks.push(c);
        self
    }

    pub fn register_element(&mut self, c: Box<dyn ElementCodec>) -> &mut Self {
        self.element_by_tag.insert(c.tag(), self.elements.len());
        self.elements.push(c);
        self
    }

    pub fn chunk_codecs(&self) -> impl Iterator<Item = &dyn ChunkCodec> {
        self.chunks.iter().map(|c| c.as_ref())
    }

    pub fn chunk(&self, tag: &[u8; 4]) -> Option<&dyn ChunkCodec> {
        self.chunk_by_tag.get(tag).map(|&i| self.chunks[i].as_ref())
    }

    pub fn element(&self, tag: &[u8; 4]) -> Option<&dyn ElementCodec> {
        self.element_by_tag.get(tag).map(|&i| self.elements[i].as_ref())
    }

    pub fn element_for(&self, kind: &ElementKind) -> Option<&dyn ElementCodec> {
        self.elements.iter().find(|c| c.handles(kind)).map(|c| c.as_ref())
    }
}

impl Default for Registry {
    fn default() -> Self {
        let mut r = Self::empty();
        crate::v1::register(&mut r);
        r
    }
}
