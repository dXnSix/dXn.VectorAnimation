//! Where a frame draws: its outlines' bounds, strokes as wide as they
//! reach, spread by blurs and shadows, within clips.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{
    BlendMode, ClipPath, Color, FillRule, Frame, FrameEffect, FramePaint, Item, Layer, LineCap,
    LineJoin, Matrix, Path, Point, Rect, StrokeStyle,
};

fn square(x: f64, y: f64, size: f64) -> Path {
    let mut p = Path::new();
    p.move_to(Point::new(x, y));
    p.line_to(Point::new(x + size, y));
    p.line_to(Point::new(x + size, y + size));
    p.line_to(Point::new(x, y + size));
    p.close();
    p
}

fn fill(path: Path) -> Item {
    Item::Fill {
        path,
        transform: Matrix::IDENTITY,
        paint: FramePaint::Solid(Color::rgba(0.0, 0.0, 0.0, 1.0)),
        rule: FillRule::NonZero,
    }
}

fn layer(effects: Vec<FrameEffect>, clip: Option<Vec<ClipPath>>, items: Vec<Item>) -> Vec<Item> {
    let mut out = vec![Item::PushLayer(Layer {
        opacity: 1.0,
        blend: BlendMode::Normal,
        clip,
        mask: None,
        effects,
    })];
    out.extend(items);
    out.push(Item::PopLayer);
    out
}

fn bounds(items: Vec<Item>) -> Rect {
    Frame {
        width: 100.0,
        height: 100.0,
        items,
    }
    .bounds()
    .unwrap()
}

#[test]
fn fills_and_strokes() {
    let stroke = Item::Stroke {
        path: square(50.0, 50.0, 10.0),
        transform: Matrix::scale(2.0, 2.0),
        paint: FramePaint::Solid(Color::rgba(0.0, 0.0, 0.0, 1.0)),
        style: StrokeStyle {
            width: 4.0,
            cap: LineCap::Butt,
            join: LineJoin::Round,
            miter_limit: 4.0,
            dash: None,
        },
    };
    // The stroke 4 wide, doubled: 4 out each side of 100..120.
    assert_eq!(
        bounds(vec![fill(square(0.0, 0.0, 10.0)), stroke]),
        Rect::new(0.0, 0.0, 124.0, 124.0)
    );
}

#[test]
fn shadows_and_blurs_spread() {
    let b = bounds(layer(
        vec![FrameEffect::DropShadow {
            color: Color::rgba(0.0, 0.0, 0.0, 1.0),
            offset: Point::new(10.0, 0.0),
            sigma: 2.0,
            knockout: false,
            hard: false,
        }],
        None,
        vec![fill(square(20.0, 20.0, 10.0))],
    ));
    assert_eq!(b, Rect::new(20.0, 14.0, 26.0, 22.0));
    let b = bounds(layer(
        vec![FrameEffect::Blur { sigma: 1.0 }],
        None,
        vec![fill(square(20.0, 20.0, 10.0))],
    ));
    assert_eq!(b, Rect::new(17.0, 17.0, 16.0, 16.0));
}

#[test]
fn clips_cut_what_spreads() {
    let clip = vec![ClipPath {
        path: square(0.0, 0.0, 25.0),
        transform: Matrix::IDENTITY,
        rule: FillRule::NonZero,
    }];
    let b = bounds(layer(
        vec![FrameEffect::Blur { sigma: 2.0 }],
        Some(clip),
        vec![fill(square(20.0, 20.0, 10.0))],
    ));
    assert_eq!(b, Rect::new(14.0, 14.0, 11.0, 11.0));
}
