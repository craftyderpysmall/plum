//! Built-in codecs for format 1.x.
//!
//! Wire structs are defined here (not reused from `plum-core`) so the file
//! layout never changes by accident when the in-memory model evolves.

use glam::{DVec2, Vec2};
use plum_core::{Board, Element, ElementId, ElementKind, Line, Paint, Rect, Rgba, Shape, ShapeKind, Stroke, Style, Text, TextAlign, Transform, UnknownKind};
use serde::{Deserialize, Serialize};

use crate::FormatError;
use crate::codec::{ChunkCodec, ElementCodec, LoadCtx, Registry};

pub const META: [u8; 4] = *b"META";
pub const VIEW: [u8; 4] = *b"VIEW";
pub const ELEM: [u8; 4] = *b"ELEM";
pub const ELM2: [u8; 4] = *b"ELM2";

pub const STROKE: [u8; 4] = *b"STRK";
pub const SHAPE: [u8; 4] = *b"SHAP";
pub const LINE: [u8; 4] = *b"LINE";
pub const TEXT: [u8; 4] = *b"TEXT";

/// Elements per ELEM chunk: big enough for LZ4 to pay off, small enough that
/// a corrupt chunk loses little.
const ELEMENTS_PER_CHUNK: usize = 2048;

pub fn register(r: &mut Registry) {
    r.register_chunk(Box::new(MetaCodec))
        .register_chunk(Box::new(ViewCodec))
        .register_chunk(Box::new(ElemCodec))
        .register_chunk(Box::new(Elem2Codec))
        .register_element(Box::new(StrokeCodec))
        .register_element(Box::new(ShapeCodec))
        .register_element(Box::new(LineCodec))
        .register_element(Box::new(TextCodec));
}

#[cfg(test)]
pub(crate) fn register_without_strokes_for_tests(r: &mut Registry) {
    r.register_chunk(Box::new(MetaCodec))
        .register_chunk(Box::new(ViewCodec))
        .register_chunk(Box::new(ElemCodec))
        .register_chunk(Box::new(Elem2Codec))
        .register_element(Box::new(ShapeCodec))
        .register_element(Box::new(LineCodec))
        .register_element(Box::new(TextCodec));
}

fn enc<T: Serialize>(v: &T) -> Result<Vec<u8>, FormatError> {
    postcard::to_allocvec(v).map_err(|e| FormatError::Decode(e.to_string()))
}

fn dec<'a, T: Deserialize<'a>>(b: &'a [u8]) -> Result<T, FormatError> {
    postcard::from_bytes(b).map_err(|e| FormatError::Decode(e.to_string()))
}

// ---------------------------------------------------------------- META / VIEW

#[derive(Serialize, Deserialize)]
struct MetaV1 {
    name: String,
    created: u64,
    modified: u64,
}

struct MetaCodec;

impl ChunkCodec for MetaCodec {
    fn tag(&self) -> [u8; 4] {
        META
    }

    fn write(&self, b: &Board, _: &Registry) -> Result<Vec<Vec<u8>>, FormatError> {
        let m = &b.meta;
        Ok(vec![enc(&MetaV1 { name: m.name.clone(), created: m.created, modified: m.modified })?])
    }

    fn read(&self, p: &[u8], ctx: &mut LoadCtx, _: &Registry) -> Result<(), FormatError> {
        let m: MetaV1 = dec(p)?;
        ctx.meta.name = m.name;
        ctx.meta.created = m.created;
        ctx.meta.modified = m.modified;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct ViewV1 {
    center: [f64; 2],
    zoom: f64,
}

struct ViewCodec;

impl ChunkCodec for ViewCodec {
    fn tag(&self) -> [u8; 4] {
        VIEW
    }

    fn write(&self, b: &Board, _: &Registry) -> Result<Vec<Vec<u8>>, FormatError> {
        Ok(vec![enc(&ViewV1 { center: b.meta.view_center.to_array(), zoom: b.meta.view_zoom })?])
    }

    fn read(&self, p: &[u8], ctx: &mut LoadCtx, _: &Registry) -> Result<(), FormatError> {
        let v: ViewV1 = dec(p)?;
        ctx.meta.view_center = DVec2::from(v.center);
        ctx.meta.view_zoom = if v.zoom.is_finite() && v.zoom > 0.0 { v.zoom } else { 1.0 };
        Ok(())
    }
}

// ---------------------------------------------------------------- ELEM

/// Fields every element record shares, independent of how style is stored.
struct RecordCore {
    id: u64,
    pos: [f64; 2],
    rotation: f64,
    scale: [f64; 2],
    bounds: [f32; 4],
    kind_tag: [u8; 4],
    kind_version: u8,
    body: Vec<u8>,
}

impl RecordCore {
    fn from_element(el: &Element, reg: &Registry) -> Result<Self, FormatError> {
        let (kind_tag, kind_version, body) = match &el.kind {
            ElementKind::Unknown(u) => (u.tag, u.kind_version, u.bytes.clone()),
            k => {
                let c = reg.element_for(k).ok_or_else(|| FormatError::Decode(format!("no codec for {}", k.name())))?;
                (c.tag(), c.version(), c.encode(k)?)
            }
        };
        let b = el.local_bounds();
        let t = &el.transform;
        Ok(Self {
            id: el.id.0,
            pos: t.pos.to_array(),
            rotation: t.rotation,
            scale: t.scale.to_array(),
            bounds: [b.min.x, b.min.y, b.max.x, b.max.y],
            kind_tag,
            kind_version,
            body,
        })
    }

    fn into_element(self, style: Style, reg: &Registry, warnings: &mut Vec<String>) -> Element {
        let r = self;
        let bounds = Rect::new(Vec2::new(r.bounds[0], r.bounds[1]), Vec2::new(r.bounds[2], r.bounds[3]));
        let unknown = |body: Vec<u8>| ElementKind::Unknown(UnknownKind { tag: r.kind_tag, kind_version: r.kind_version, bytes: body, bounds });
        let kind = match reg.element(&r.kind_tag) {
            Some(c) if r.kind_version <= c.version() => match c.decode(r.kind_version, &r.body) {
                Ok(k) => k,
                Err(e) => {
                    warnings.push(format!("element {}: {e}; kept as opaque data", r.id));
                    unknown(r.body)
                }
            },
            _ => unknown(r.body),
        };
        let mut el = Element::new(kind, Transform { pos: DVec2::from(r.pos), rotation: r.rotation, scale: DVec2::from(r.scale) }, style);
        el.id = ElementId(r.id);
        el
    }
}

fn write_batches<R: Serialize>(b: &Board, reg: &Registry, f: impl Fn(&Element, RecordCore) -> R) -> Result<Vec<Vec<u8>>, FormatError> {
    b.elements()
        .chunks(ELEMENTS_PER_CHUNK)
        .map(|batch| {
            let recs = batch.iter().map(|e| Ok(f(e, RecordCore::from_element(e, reg)?))).collect::<Result<Vec<_>, FormatError>>()?;
            enc(&recs)
        })
        .collect()
}

// ELEM (format 1.0): fixed RGBA colors, no opacity. Read-only since 1.1.

#[derive(Serialize, Deserialize)]
pub(crate) struct ElemRecordV1 {
    pub(crate) id: u64,
    pub(crate) pos: [f64; 2],
    pub(crate) rotation: f64,
    pub(crate) scale: [f64; 2],
    pub(crate) stroke: [u8; 4],
    pub(crate) fill: Option<[u8; 4]>,
    pub(crate) width: f32,
    /// Local bounds, so readers that don't know `kind_tag` can still draw a placeholder.
    pub(crate) bounds: [f32; 4],
    pub(crate) kind_tag: [u8; 4],
    pub(crate) kind_version: u8,
    pub(crate) body: Vec<u8>,
}

struct ElemCodec;

impl ChunkCodec for ElemCodec {
    fn tag(&self) -> [u8; 4] {
        ELEM
    }

    /// Superseded by ELM2; nothing is written.
    fn write(&self, _: &Board, _: &Registry) -> Result<Vec<Vec<u8>>, FormatError> {
        Ok(Vec::new())
    }

    fn read(&self, p: &[u8], ctx: &mut LoadCtx, reg: &Registry) -> Result<(), FormatError> {
        let recs: Vec<ElemRecordV1> = dec(p)?;
        for r in recs {
            let style = Style { stroke: Paint::Rgba(Rgba(r.stroke)), fill: r.fill.map(|f| Paint::Rgba(Rgba(f))), width: r.width, opacity: 1.0 };
            let core = RecordCore { id: r.id, pos: r.pos, rotation: r.rotation, scale: r.scale, bounds: r.bounds, kind_tag: r.kind_tag, kind_version: r.kind_version, body: r.body };
            ctx.elements.push(core.into_element(style, reg, &mut ctx.warnings));
        }
        Ok(())
    }
}

// ELM2 (format 1.1): colors may reference the theme palette; per-element opacity.

/// Variant order is part of the file format: append only.
#[derive(Clone, Copy, Serialize, Deserialize)]
enum PaintV2 {
    Rgba([u8; 4]),
    Palette(u8),
}

impl From<Paint> for PaintV2 {
    fn from(p: Paint) -> Self {
        match p {
            Paint::Rgba(c) => PaintV2::Rgba(c.0),
            Paint::Palette(i) => PaintV2::Palette(i),
        }
    }
}

impl From<PaintV2> for Paint {
    fn from(p: PaintV2) -> Self {
        match p {
            PaintV2::Rgba(c) => Paint::Rgba(Rgba(c)),
            PaintV2::Palette(i) => Paint::Palette(i),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ElemRecordV2 {
    id: u64,
    pos: [f64; 2],
    rotation: f64,
    scale: [f64; 2],
    stroke: PaintV2,
    fill: Option<PaintV2>,
    width: f32,
    opacity: f32,
    bounds: [f32; 4],
    kind_tag: [u8; 4],
    kind_version: u8,
    body: Vec<u8>,
}

struct Elem2Codec;

impl ChunkCodec for Elem2Codec {
    fn tag(&self) -> [u8; 4] {
        ELM2
    }

    fn write(&self, b: &Board, reg: &Registry) -> Result<Vec<Vec<u8>>, FormatError> {
        write_batches(b, reg, |e, c| ElemRecordV2 {
            id: c.id,
            pos: c.pos,
            rotation: c.rotation,
            scale: c.scale,
            stroke: e.style.stroke.into(),
            fill: e.style.fill.map(Into::into),
            width: e.style.width,
            opacity: e.style.opacity,
            bounds: c.bounds,
            kind_tag: c.kind_tag,
            kind_version: c.kind_version,
            body: c.body,
        })
    }

    fn read(&self, p: &[u8], ctx: &mut LoadCtx, reg: &Registry) -> Result<(), FormatError> {
        let recs: Vec<ElemRecordV2> = dec(p)?;
        for r in recs {
            let opacity = if r.opacity.is_finite() { r.opacity.clamp(0.0, 1.0) } else { 1.0 };
            let style = Style { stroke: r.stroke.into(), fill: r.fill.map(Into::into), width: r.width, opacity };
            let core = RecordCore { id: r.id, pos: r.pos, rotation: r.rotation, scale: r.scale, bounds: r.bounds, kind_tag: r.kind_tag, kind_version: r.kind_version, body: r.body };
            ctx.elements.push(core.into_element(style, reg, &mut ctx.warnings));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- element kinds

/// Stroke payload v1: raw f32 points (8 bytes each). Read-only.
#[derive(Serialize, Deserialize)]
struct StrokeV1 {
    points: Vec<[f32; 2]>,
}

/// Stroke payload v2: points on a grid of `2^exp` local units, stored as
/// second-order deltas (the change in step from one point to the next).
/// Pen strokes are smooth, so these are tiny numbers, and postcard writes each
/// `i32` as a zigzag varint: usually 1–2 bytes per coordinate instead of 4.
/// The grid is ~1/260,000 of the stroke's size: far below a pixel at any zoom.
#[derive(Serialize, Deserialize)]
struct StrokeV2 {
    exp: i8,
    deltas: Vec<[i32; 2]>,
}

/// Grid resolution relative to the stroke's extent.
const STROKE_GRID_BITS: i32 = 18;

fn stroke_grid_exp(points: &[Vec2]) -> i8 {
    let extent = points.iter().fold(0f32, |m, p| m.max(p.x.abs()).max(p.y.abs()));
    if !(extent.is_finite() && extent > 0.0) {
        return -(STROKE_GRID_BITS as i8);
    }
    // Power-of-two step, so coordinates that are binary fractions survive exactly.
    (extent.log2().ceil() as i32 - STROKE_GRID_BITS).clamp(-126, 100) as i8
}

struct StrokeCodec;

impl ElementCodec for StrokeCodec {
    fn tag(&self) -> [u8; 4] {
        STROKE
    }
    fn version(&self) -> u8 {
        2
    }
    fn handles(&self, k: &ElementKind) -> bool {
        matches!(k, ElementKind::Stroke(_))
    }
    fn encode(&self, k: &ElementKind) -> Result<Vec<u8>, FormatError> {
        let ElementKind::Stroke(s) = k else { unreachable!() };
        let exp = stroke_grid_exp(&s.points);
        let inv = (-(exp as f32)).exp2();
        let q = |v: f32| (v * inv).round().clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        let (mut prev, mut step) = ([0i32; 2], [0i32; 2]);
        let deltas = s
            .points
            .iter()
            .map(|p| {
                let cur = [q(p.x), q(p.y)];
                let d = [cur[0].wrapping_sub(prev[0]), cur[1].wrapping_sub(prev[1])];
                let dd = [d[0].wrapping_sub(step[0]), d[1].wrapping_sub(step[1])];
                (prev, step) = (cur, d);
                dd
            })
            .collect();
        enc(&StrokeV2 { exp, deltas })
    }
    fn decode(&self, version: u8, b: &[u8]) -> Result<ElementKind, FormatError> {
        let points = if version < 2 {
            dec::<StrokeV1>(b)?.points.into_iter().map(Vec2::from).collect()
        } else {
            let s: StrokeV2 = dec(b)?;
            let step = (s.exp as f32).exp2();
            let (mut acc, mut vel) = ([0i32; 2], [0i32; 2]);
            s.deltas
                .into_iter()
                .map(|dd| {
                    vel = [vel[0].wrapping_add(dd[0]), vel[1].wrapping_add(dd[1])];
                    acc = [acc[0].wrapping_add(vel[0]), acc[1].wrapping_add(vel[1])];
                    Vec2::new(acc[0] as f32 * step, acc[1] as f32 * step)
                })
                .collect()
        };
        Ok(ElementKind::Stroke(Stroke { points }))
    }
}

#[derive(Serialize, Deserialize)]
struct ShapeV1 {
    /// 0 rect, 1 ellipse, 2 diamond, 3 triangle.
    shape: u8,
    size: [f32; 2],
}

struct ShapeCodec;

impl ElementCodec for ShapeCodec {
    fn tag(&self) -> [u8; 4] {
        SHAPE
    }
    fn version(&self) -> u8 {
        1
    }
    fn handles(&self, k: &ElementKind) -> bool {
        matches!(k, ElementKind::Shape(_))
    }
    fn encode(&self, k: &ElementKind) -> Result<Vec<u8>, FormatError> {
        let ElementKind::Shape(s) = k else { unreachable!() };
        let shape = match s.shape {
            ShapeKind::Rect => 0,
            ShapeKind::Ellipse => 1,
            ShapeKind::Diamond => 2,
            ShapeKind::Triangle => 3,
        };
        enc(&ShapeV1 { shape, size: s.size.to_array() })
    }
    fn decode(&self, _: u8, b: &[u8]) -> Result<ElementKind, FormatError> {
        let s: ShapeV1 = dec(b)?;
        let shape = match s.shape {
            0 => ShapeKind::Rect,
            1 => ShapeKind::Ellipse,
            2 => ShapeKind::Diamond,
            3 => ShapeKind::Triangle,
            n => return Err(FormatError::Decode(format!("unknown shape {n}"))),
        };
        Ok(ElementKind::Shape(Shape { shape, size: Vec2::from(s.size) }))
    }
}

#[derive(Serialize, Deserialize)]
struct LineV1 {
    a: [f32; 2],
    b: [f32; 2],
    arrow_start: bool,
    arrow_end: bool,
}

struct LineCodec;

impl ElementCodec for LineCodec {
    fn tag(&self) -> [u8; 4] {
        LINE
    }
    fn version(&self) -> u8 {
        1
    }
    fn handles(&self, k: &ElementKind) -> bool {
        matches!(k, ElementKind::Line(_))
    }
    fn encode(&self, k: &ElementKind) -> Result<Vec<u8>, FormatError> {
        let ElementKind::Line(l) = k else { unreachable!() };
        enc(&LineV1 { a: l.a.to_array(), b: l.b.to_array(), arrow_start: l.arrow_start, arrow_end: l.arrow_end })
    }
    fn decode(&self, _: u8, b: &[u8]) -> Result<ElementKind, FormatError> {
        let l: LineV1 = dec(b)?;
        Ok(ElementKind::Line(Line { a: Vec2::from(l.a), b: Vec2::from(l.b), arrow_start: l.arrow_start, arrow_end: l.arrow_end }))
    }
}

#[derive(Serialize, Deserialize)]
struct TextV1 {
    content: String,
    font_size: f32,
    /// 0 left, 1 center, 2 right.
    align: u8,
    size: [f32; 2],
}

struct TextCodec;

impl ElementCodec for TextCodec {
    fn tag(&self) -> [u8; 4] {
        TEXT
    }
    fn version(&self) -> u8 {
        1
    }
    fn handles(&self, k: &ElementKind) -> bool {
        matches!(k, ElementKind::Text(_))
    }
    fn encode(&self, k: &ElementKind) -> Result<Vec<u8>, FormatError> {
        let ElementKind::Text(t) = k else { unreachable!() };
        let align = match t.align {
            TextAlign::Left => 0,
            TextAlign::Center => 1,
            TextAlign::Right => 2,
        };
        enc(&TextV1 { content: t.content.clone(), font_size: t.font_size, align, size: t.size.to_array() })
    }
    fn decode(&self, _: u8, b: &[u8]) -> Result<ElementKind, FormatError> {
        let t: TextV1 = dec(b)?;
        let align = match t.align {
            1 => TextAlign::Center,
            2 => TextAlign::Right,
            _ => TextAlign::Left,
        };
        Ok(ElementKind::Text(Text { content: t.content, font_size: t.font_size, align, size: Vec2::from(t.size) }))
    }
}
