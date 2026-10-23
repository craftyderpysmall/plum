//! Writes a demo board using every element kind:
//! `cargo run -p plum-format --example make_demo -- demo.plum`
//! Add `--stress N` to also scatter N random strokes (for benchmarking).

use glam::{DVec2, Vec2};
use plum_core::*;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.iter().find(|a| !a.starts_with("--") && a.parse::<usize>().is_err()).cloned().unwrap_or_else(|| "demo.plum".into());
    let stress = args.iter().position(|a| a == "--stress").and_then(|i| args.get(i + 1)).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
    let mut b = Board::new();
    add_stress(&mut b, stress);
    b.meta.name = "Demo".into();
    // Palette slots follow the active theme (0 = ink, 6 = accent).
    let ink = Paint::Palette(0);
    let purple = Paint::Palette(6);
    let at = |x: f64, y: f64| Transform::at(DVec2::new(x, y));

    let wave: Vec<Vec2> = (0..120).map(|i| Vec2::new(i as f32 * 3.0, (i as f32 * 0.12).sin() * 30.0)).collect();
    let (stroke, off) = Stroke::finish(&wave, 0.3);
    b.push(Element::new(ElementKind::Stroke(stroke), Transform::at(DVec2::new(-300.0, -150.0) + off.as_dvec2()), Style { stroke: purple, fill: None, width: 4.0, opacity: 1.0 }));

    let shapes = [ShapeKind::Rect, ShapeKind::Ellipse, ShapeKind::Diamond, ShapeKind::Triangle];
    for (i, s) in shapes.into_iter().enumerate() {
        let fill = if i % 2 == 0 { Some(Paint::Rgba(Rgba([0x7c, 0x4d, 0xff, 60]))) } else { None };
        let t = Transform { rotation: i as f64 * 0.2, ..at(-300.0 + i as f64 * 160.0, 50.0) };
        b.push(Element::new(ElementKind::Shape(Shape { shape: s, size: Vec2::new(120.0, 80.0) }), t, Style { stroke: ink, fill, width: 2.5, opacity: 1.0 }));
    }

    // A flipped + scaled triangle shows mirrored geometry.
    let t = Transform { pos: DVec2::new(380.0, 50.0), rotation: 0.3, scale: DVec2::new(-1.5, 1.0) };
    b.push(Element::new(ElementKind::Shape(Shape { shape: ShapeKind::Triangle, size: Vec2::new(80.0, 80.0) }), t, Style { stroke: purple, fill: None, width: 2.5, opacity: 1.0 }));

    b.push(Element::new(
        ElementKind::Line(Line { a: Vec2::new(-150.0, 0.0), b: Vec2::new(150.0, 0.0), arrow_start: false, arrow_end: true }),
        Transform { rotation: -0.15, ..at(-100.0, 220.0) },
        Style { stroke: ink, fill: None, width: 3.0, opacity: 1.0 },
    ));

    b.push(Element::new(
        ElementKind::Text(Text { content: "Hello, Plum!\nRotated & flipped text".into(), font_size: 28.0, align: TextAlign::Left, size: Vec2::new(300.0, 70.0) }),
        Transform { rotation: 0.1, ..at(150.0, -200.0) },
        Style { stroke: ink, fill: None, width: 0.0, opacity: 1.0 },
    ));
    b.push(Element::new(
        ElementKind::Text(Text { content: "mirrored".into(), font_size: 28.0, align: TextAlign::Left, size: Vec2::new(110.0, 34.0) }),
        Transform { pos: DVec2::new(330.0, 200.0), rotation: 0.0, scale: DVec2::new(-1.0, 1.0) },
        Style { stroke: purple, fill: None, width: 0.0, opacity: 1.0 },
    ));

    let t = std::time::Instant::now();
    plum_format::save_to_path(&b, std::path::Path::new(&out)).expect("save");
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    println!("wrote {out} ({} elements, {} KiB) in {:?}", b.len(), size / 1024, t.elapsed());
    let t = std::time::Instant::now();
    let loaded = plum_format::load_from_path(std::path::Path::new(&out)).expect("load");
    println!("reloaded {} elements in {:?}", loaded.board.len(), t.elapsed());
}

fn add_stress(b: &mut Board, n: usize) {
    if n == 0 {
        return;
    }
    let mut seed = 0x2545f4914f6cdd1du64;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed >> 11) as f64 / (1u64 << 53) as f64
    };
    for _ in 0..n {
        let center = DVec2::new(rnd() - 0.5, rnd() - 0.5) * 100_000.0;
        let mut p = Vec2::ZERO;
        let pts: Vec<Vec2> = (0..40)
            .map(|_| {
                p += Vec2::new((rnd() as f32 - 0.5) * 20.0, (rnd() as f32 - 0.5) * 20.0);
                p
            })
            .collect();
        let (stroke, off) = Stroke::finish(&pts, 0.0);
        let color = Rgba([(rnd() * 200.0) as u8, (rnd() * 200.0) as u8, (rnd() * 200.0) as u8, 255]);
        b.push(Element::new(ElementKind::Stroke(stroke), Transform::at(center + off.as_dvec2()), Style { stroke: Paint::Rgba(color), fill: None, width: 2.0, opacity: 1.0 }));
    }
    b.meta.view_zoom = 0.05;
}
