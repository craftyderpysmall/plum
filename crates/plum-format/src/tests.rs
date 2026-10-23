use glam::{DVec2, Vec2};
use plum_core::*;

use super::*;

fn sample_board() -> Board {
    let mut b = Board::new();
    b.meta.name = "Test board".into();
    b.meta.view_center = DVec2::new(1e7, -42.5);
    b.meta.view_zoom = 0.25;
    let style = Style { stroke: Paint::Palette(3), fill: Some(Paint::Rgba(Rgba([1, 2, 3, 4]))), width: 3.5, opacity: 0.4 };
    let t = Transform { pos: DVec2::new(-1e6, 7.0), rotation: 0.3, scale: DVec2::new(-1.0, 2.0) };
    b.push(Element::new(ElementKind::Stroke(Stroke { points: vec![Vec2::ZERO, Vec2::new(1.5, 2.5)] }), t, style));
    b.push(Element::new(ElementKind::Shape(Shape { shape: ShapeKind::Ellipse, size: Vec2::new(40.0, 20.0) }), t, style));
    b.push(Element::new(
        ElementKind::Line(Line { a: Vec2::ZERO, b: Vec2::new(10.0, 0.0), arrow_start: false, arrow_end: true }),
        t,
        style,
    ));
    b.push(Element::new(
        ElementKind::Text(Text { content: "héllo\nworld".into(), font_size: 24.0, align: TextAlign::Center, size: Vec2::new(80.0, 60.0) }),
        t,
        style,
    ));
    b
}

fn to_bytes(b: &Board) -> Vec<u8> {
    let mut out = Vec::new();
    write(b, &mut out, &Registry::default()).unwrap();
    out
}

fn from_bytes(bytes: &[u8]) -> Loaded {
    read(&mut &bytes[..], &Registry::default()).unwrap()
}

/// Element equality, except stroke points may differ by the v2 grid step
/// (~1e-6 of the stroke's size).
fn assert_same(a: &[Element], b: &[Element]) {
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(b) {
        assert_eq!((x.id, x.transform, x.style), (y.id, y.transform, y.style));
        match (&x.kind, &y.kind) {
            (ElementKind::Stroke(p), ElementKind::Stroke(q)) => {
                assert_eq!(p.points.len(), q.points.len());
                let extent = p.points.iter().fold(1e-6f32, |m, v| m.max(v.abs().max_element()));
                for (u, v) in p.points.iter().zip(&q.points) {
                    assert!((*u - *v).abs().max_element() <= extent * 4e-6, "{u} vs {v}");
                }
            }
            (k, l) => assert_eq!(k, l),
        }
    }
}

#[test]
fn round_trip() {
    let b = sample_board();
    let l = from_bytes(&to_bytes(&b));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
    assert_eq!(l.board.meta, b.meta);
    assert_same(l.board.elements(), b.elements());
}

#[test]
fn rejects_wrong_magic_and_major() {
    assert!(matches!(read(&mut &b"NOTPLUM........."[..], &Registry::default()), Err(FormatError::BadMagic)));
    let mut bytes = to_bytes(&sample_board());
    bytes[8] = 9;
    assert!(matches!(read(&mut &bytes[..], &Registry::default()), Err(FormatError::UnsupportedVersion { major: 9, .. })));
}

#[test]
fn unknown_chunks_are_preserved() {
    let b = sample_board();
    let mut bytes = Vec::new();
    chunk::write_header(&mut bytes).unwrap();
    chunk::write_chunk(&mut bytes, *b"XTRA", b"future data").unwrap();
    // Append the real board's chunks after our extra one.
    bytes.extend_from_slice(&to_bytes(&b)[16..]);
    let l = from_bytes(&bytes);
    assert_eq!(l.board.meta.opaque_chunks, vec![(*b"XTRA", b"future data".to_vec())]);
    assert_eq!(l.board.len(), 4);
    // Written back on save.
    let again = from_bytes(&to_bytes(&l.board));
    assert_eq!(again.board.meta.opaque_chunks, l.board.meta.opaque_chunks);
}

#[test]
fn unknown_element_kinds_round_trip() {
    // A registry that doesn't know strokes simulates an older reader.
    let mut old = Registry::empty();
    v1::register_without_strokes_for_tests(&mut old);
    let bytes = to_bytes(&sample_board());
    let l = read(&mut &bytes[..], &old).unwrap();
    let ElementKind::Unknown(u) = &l.board.elements()[0].kind else { panic!("expected unknown") };
    assert_eq!(&u.tag, b"STRK");
    // The older reader writes it back; a full reader recovers the stroke.
    let mut out = Vec::new();
    write(&l.board, &mut out, &old).unwrap();
    let full = from_bytes(&out);
    assert!(matches!(full.board.elements()[0].kind, ElementKind::Stroke(_)));
    assert_eq!(full.board.elements()[0].kind, sample_board().elements()[0].kind);
}

#[test]
fn corrupt_chunk_is_skipped() {
    let b = sample_board();
    let mut bytes = to_bytes(&b);
    // META is the first chunk: flip a byte inside its payload.
    bytes[16 + 9 + 1] ^= 0xFF;
    let l = from_bytes(&bytes);
    assert_eq!(l.warnings.len(), 1, "{:?}", l.warnings);
    assert_eq!(l.board.len(), 4);
    assert_eq!(l.board.meta.name, "Untitled");
}

#[test]
fn truncated_file_loads_prefix() {
    let bytes = to_bytes(&sample_board());
    let l = from_bytes(&bytes[..bytes.len() - 5]);
    assert_eq!(l.board.len(), 4);
    assert_eq!(l.warnings.len(), 1);
}

#[test]
fn large_boards_compress_and_round_trip() {
    let mut b = Board::new();
    for i in 0..5000 {
        let pts = (0..50).map(|j| Vec2::new(j as f32, (j as f32 * 0.1).sin())).collect();
        b.push(Element::new(ElementKind::Stroke(Stroke { points: pts }), Transform::at(DVec2::new(i as f64, 0.0)), Style::default()));
    }
    let bytes = to_bytes(&b);
    let raw = 5000 * 50 * 8;
    assert!(bytes.len() < raw, "expected compression: {} >= {raw}", bytes.len());
    let l = from_bytes(&bytes);
    assert_same(l.board.elements(), b.elements());
}

#[test]
fn stroke_v2_is_compact_and_precise() {
    let codec = Registry::default();
    let c = codec.element(&v1::STROKE).unwrap();
    // A realistic pen stroke: ~1-3 unit steps over a 400-unit loop.
    let pts: Vec<Vec2> = (0..400).map(|i| Vec2::from_angle(i as f32 * 0.0157) * 200.0 + Vec2::new(0.37, -0.11)).collect();
    let kind = ElementKind::Stroke(Stroke { points: pts.clone() });
    let v2 = c.encode(&kind).unwrap();
    let v1 = postcard::to_allocvec(&pts.iter().map(|p| p.to_array()).collect::<Vec<_>>()).unwrap();
    assert!(v2.len() * 10 < v1.len() * 6, "v2 {} bytes vs v1 {}", v2.len(), v1.len());
    let ElementKind::Stroke(back) = c.decode(2, &v2).unwrap() else { panic!() };
    for (a, b) in pts.iter().zip(&back.points) {
        assert!((*a - *b).abs().max_element() < 1e-3, "{a} vs {b}");
    }
    // Binary-fraction coordinates survive exactly.
    let exact = ElementKind::Stroke(Stroke { points: vec![Vec2::ZERO, Vec2::new(1.5, -2.25)] });
    assert_eq!(c.decode(2, &c.encode(&exact).unwrap()).unwrap(), exact);
}

#[test]
fn stroke_v1_payloads_still_decode() {
    let c = Registry::default();
    let c = c.element(&v1::STROKE).unwrap();
    let body = postcard::to_allocvec(&vec![[0.1f32, 0.2], [3.3, -4.4]]).unwrap();
    let ElementKind::Stroke(s) = c.decode(1, &body).unwrap() else { panic!() };
    assert_eq!(s.points, vec![Vec2::new(0.1, 0.2), Vec2::new(3.3, -4.4)]);
}

#[test]
fn legacy_elem_chunks_still_load() {
    // A 1.0 file: ELEM records with fixed RGBA colors and no opacity.
    let body = postcard::to_allocvec(&serde_shape()).unwrap();
    let rec = v1::ElemRecordV1 {
        id: 7,
        pos: [1.0, 2.0],
        rotation: 0.5,
        scale: [1.0, -1.0],
        stroke: [9, 8, 7, 255],
        fill: None,
        width: 2.0,
        bounds: [-5.0, -5.0, 5.0, 5.0],
        kind_tag: *b"SHAP",
        kind_version: 1,
        body,
    };
    let mut bytes = Vec::new();
    chunk::write_header(&mut bytes).unwrap();
    chunk::write_chunk(&mut bytes, v1::ELEM, &postcard::to_allocvec(&vec![rec]).unwrap()).unwrap();
    chunk::write_end(&mut bytes).unwrap();
    let l = from_bytes(&bytes);
    let el = &l.board.elements()[0];
    assert_eq!(el.id, ElementId(7));
    assert_eq!(el.style, Style { stroke: Paint::Rgba(Rgba([9, 8, 7, 255])), fill: None, width: 2.0, opacity: 1.0 });
    assert!(matches!(el.kind, ElementKind::Shape(_)));
    // Re-saving writes the new chunk only.
    let resaved = to_bytes(&l.board);
    assert!(resaved.windows(4).any(|w| w == v1::ELM2) && !resaved.windows(4).any(|w| w == v1::ELEM));
}

/// Wire form of a 10×10 rect shape payload (ShapeV1).
fn serde_shape() -> (u8, [f32; 2]) {
    (0, [10.0, 10.0])
}


#[test]
fn save_and_load_path() {
    let dir = std::env::temp_dir().join(format!("plum-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("board.plum");
    save_to_path(&sample_board(), &path).unwrap();
    let l = load_from_path(&path).unwrap();
    assert_eq!(l.board.len(), 4);
    assert!(!path.with_extension("plum.tmp").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
