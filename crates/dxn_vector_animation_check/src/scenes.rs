//! The check set's own scenes: one or more for each kind of animation the
//! player draws on the graph, so a platform's run covers them without the
//! corpus (whose files are not ours to commit).

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Fill, Geometry, GradientStop,
    GradientUnits, Group, Keyframe, LineCap, LineJoin, LinearGradient, LoopMode, Matrix, Node,
    Paint, RadialGradient, RectStart, Scene, Shape, ShapePath, Spread, Stroke, Timeline, Track,
    Trim, TrimMode,
};

/// A ring spinner as svg-spinners draws one: a dash growing while its
/// offset runs on, SMIL throughout.
pub const RING: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24">
  <g>
    <circle cx="12" cy="12" r="9.5" fill="none" stroke="#000" stroke-width="3" stroke-linecap="round">
      <animate attributeName="stroke-dasharray" calcMode="spline" dur="1.5s" keySplines="0.42,0,0.58,1;0.42,0,0.58,1;0.42,0,0.58,1" keyTimes="0;0.475;0.95;1" repeatCount="indefinite" values="0 150;42 150;42 150;42 150"/>
      <animate attributeName="stroke-dashoffset" calcMode="spline" dur="1.5s" keySplines="0.42,0,0.58,1;0.42,0,0.58,1;0.42,0,0.58,1" keyTimes="0;0.475;0.95;1" repeatCount="indefinite" values="0;-16;-59;-59"/>
    </circle>
    <animateTransform attributeName="transform" dur="2s" repeatCount="indefinite" type="rotate" values="0 12 12;360 12 12"/>
  </g>
</svg>"##;

/// A path drawn on as Figma Motion exports it: a dash pattern `0 1` to
/// `1 1` along a path of length 1.
pub const DRAWN_ON: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes kf_line_dash { 0% { stroke-dasharray: 0 1; animation-timing-function: cubic-bezier(0.4, 0, 0.2, 1); } 100% { stroke-dasharray: 1 1; } }
    #line { animation: kf_line_dash 2s linear 0s 1 both; }
  </style>
  <path id="line" pathLength="1" d="M10 80 C 30 10, 70 10, 90 80 L 50 90" fill="none" stroke="#1060e0" stroke-width="6" stroke-linejoin="round"/>
</svg>"##;

/// Drop shadows as Figma exports them: a card's still one, and a disc's
/// whose blur and opacity move (CSS `drop-shadow()` keyframes).
pub const SHADOW: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes lift { 0% { filter: drop-shadow(0px 3px 2px rgba(20, 0, 60, 0.2)); } 100% { filter: drop-shadow(0px 3px 10px rgba(20, 0, 60, 0.6)); } }
    #disc { animation: lift 2s ease-in-out infinite alternate; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g filter="url(#card)"><rect x="12" y="10" width="54" height="40" rx="6" fill="#F24822"/></g>
  <g id="disc"><circle cx="56" cy="66" r="20" fill="#1E5AE6"/></g>
  <defs>
    <filter id="card" x="0" y="0" width="100" height="100" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
      <feFlood flood-opacity="0" result="BackgroundImageFix"/>
      <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
      <feOffset dx="2" dy="5"/>
      <feGaussianBlur stdDeviation="3"/>
      <feComposite in2="hardAlpha" operator="out"/>
      <feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.35 0"/>
      <feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow"/>
      <feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow" result="shape"/>
    </filter>
  </defs>
</svg>"##;

/// Layer blurs: a still glow (drawn once) and a disc coming into focus.
pub const BLUR: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes focus { 0% { filter: blur(6px); } 100% { filter: blur(0px); } }
    #disc { animation: focus 2s ease-in-out infinite alternate; }
  </style>
  <g filter="url(#glow)"><circle cx="34" cy="34" r="20" fill="#F5B800"/></g>
  <g id="disc"><circle cx="62" cy="64" r="22" fill="#1E5AE6"/><rect x="50" y="56" width="24" height="16" fill="#FFFFFF"/></g>
  <defs>
    <filter id="glow" x="0" y="0" width="100" height="100" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
      <feFlood flood-opacity="0" result="BackgroundImageFix"/>
      <feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
      <feGaussianBlur stdDeviation="5" result="effect1_foregroundBlur"/>
    </filter>
  </defs>
</svg>"##;

/// Soft masks: a luminance mask of gray and white, and an alpha mask of a
/// half-transparent disc, over moving content.
pub const SOFT_MASK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes slide { 0% { transform: translateX(-20px); } 100% { transform: translateX(20px); } }
    .slide { animation: slide 2s ease-in-out infinite alternate; }
  </style>
  <mask id="luma" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
    <rect x="5" y="5" width="90" height="40" fill="#808080"/>
    <circle cx="50" cy="25" r="14" fill="#FFFFFF"/>
  </mask>
  <mask id="alpha" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100" style="mask-type: alpha">
    <circle cx="50" cy="75" r="22" fill="#000000" fill-opacity="0.5"/>
  </mask>
  <g mask="url(#luma)"><g class="slide"><rect x="20" y="0" width="60" height="50" fill="#F24822"/><rect x="45" y="10" width="10" height="30" fill="#1E5AE6"/></g></g>
  <g mask="url(#alpha)"><g class="slide"><rect x="25" y="50" width="50" height="50" fill="#1E5AE6"/></g></g>
</svg>"##;

const GOLD: Color = Color::rgba(0.96, 0.72, 0.0, 1.0);
const LIGHT: Color = Color::rgba(1.0, 0.91, 0.64, 1.0);
const RED: Color = Color::rgba(0.9, 0.1, 0.1, 1.0);
const BLUE: Color = Color::rgba(0.1, 0.3, 0.9, 1.0);
const WHITE: Color = Color::rgba(1.0, 1.0, 1.0, 1.0);

fn track(keys: &[(f64, f64)]) -> Animated<f64> {
    Animated::Track(Track::new(
        keys.iter()
            .map(|(t, v)| Keyframe::new(*t, *v, Easing::EASE_IN_OUT))
            .collect(),
    ))
}

fn point(keys: &[(f64, f64, f64)]) -> AnimatedPoint {
    AnimatedPoint {
        x: track(&keys.iter().map(|k| (k.0, k.1)).collect::<Vec<_>>()),
        y: track(&keys.iter().map(|k| (k.0, k.2)).collect::<Vec<_>>()),
    }
}

fn stops(list: &[(f64, Color)]) -> Vec<GradientStop> {
    list.iter()
        .map(|&(offset, color)| GradientStop { offset, color })
        .collect()
}

fn linear(start: AnimatedPoint, end: AnimatedPoint, stops: Animated<Vec<GradientStop>>) -> Paint {
    Paint::LinearGradient(LinearGradient {
        start,
        end,
        stops,
        units: GradientUnits::UserSpace,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    })
}

fn scene(children: Vec<Node>) -> Scene {
    Scene {
        width: 100.0,
        height: 100.0,
        timeline: Timeline {
            duration: 2.0,
            frame_rate: None,
            loop_mode: LoopMode::Loop,
            markers: vec![],
        },
        root: Group {
            children,
            ..Group::new()
        },
    }
}

fn ellipse(x: f64, y: f64, r: f64) -> ShapePath {
    ShapePath::new(Geometry::Ellipse {
        center: AnimatedPoint::fixed(x, y),
        radius: AnimatedPoint::fixed(r, r),
        direction: Direction::Clockwise,
        start: EllipseStart::Top,
    })
}

fn square(x: f64, y: f64, size: f64) -> ShapePath {
    ShapePath::new(Geometry::Rect {
        center: AnimatedPoint::fixed(x, y),
        size: AnimatedPoint::fixed(size, size),
        radius: AnimatedPoint::fixed(0.0, 0.0),
        direction: Direction::Clockwise,
        start: RectStart::TopLeft,
    })
}

fn stroke(paint: Paint, width: f64, cap: LineCap) -> Stroke {
    Stroke {
        paint,
        opacity: Animated::Static(1.0),
        width: Animated::Static(width),
        cap,
        join: LineJoin::Miter,
        miter_limit: 4.0,
        dash: None,
    }
}

fn filled(path: ShapePath, paint: Paint, opacity: f64) -> Node {
    Node::shape(Shape {
        paths: vec![path],
        fill: Some(Fill {
            paint,
            opacity: Animated::Static(opacity),
            rule: Default::default(),
        }),
        ..Default::default()
    })
}

fn stroked(path: ShapePath, stroke: Stroke, trim: Option<Trim>) -> Node {
    Node::shape(Shape {
        paths: vec![path],
        stroke: Some(stroke),
        trim,
        ..Default::default()
    })
}

/// Trims moving at both ends over a circle's start and a square's corner.
fn trims() -> Scene {
    let trim = Trim {
        start: track(&[(0.0, 0.0), (1.0, 0.1), (2.0, 0.7)]),
        end: track(&[(0.0, 0.05), (1.0, 0.8), (2.0, 0.9)]),
        offset: track(&[(0.0, 0.6), (2.0, 0.95)]),
        mode: TrimMode::EachPath,
    };
    scene(vec![
        stroked(
            ellipse(30.0, 30.0, 18.0),
            stroke(Paint::solid(RED), 5.0, LineCap::Round),
            Some(trim.clone()),
        ),
        stroked(
            square(68.0, 68.0, 36.0),
            stroke(Paint::solid(BLUE), 5.0, LineCap::Butt),
            Some(trim),
        ),
    ])
}

/// A loader: a fixed arc carried round more than once by its offset.
fn loader() -> Scene {
    let trim = Trim {
        start: Animated::Static(0.4),
        end: Animated::Static(0.1),
        offset: Animated::Track(Track::new(vec![
            Keyframe::new(0.0, -0.3, Easing::Linear),
            Keyframe::new(2.0, 1.9, Easing::Linear),
        ])),
        mode: TrimMode::EachPath,
    };
    scene(vec![stroked(
        ellipse(50.0, 50.0, 30.0),
        stroke(
            Paint::solid(Color::rgba(0.0, 0.6, 0.3, 1.0)),
            8.0,
            LineCap::Round,
        ),
        Some(trim),
    )])
}

/// As Figma's coin: a light band running along the gradient, narrowing,
/// on a fill and on its rim.
fn shine() -> Scene {
    let sweep = Animated::Track(Track::new(vec![
        Keyframe::new(
            0.0,
            stops(&[(0.96, GOLD), (0.985, LIGHT), (1.0, GOLD)]),
            Easing::cubic_bezier(0.535, -0.005, 0.455, 0.985),
        ),
        Keyframe::new(
            1.0,
            stops(&[(0.26, GOLD), (0.49, LIGHT), (0.72, GOLD)]),
            Easing::cubic_bezier(0.535, -0.005, 0.455, 0.985),
        ),
        Keyframe::new(
            2.0,
            stops(&[(0.0, GOLD), (0.015, LIGHT), (0.028, GOLD)]),
            Easing::Linear,
        ),
    ]));
    scene(vec![
        filled(
            ellipse(50.0, 50.0, 40.0),
            linear(
                AnimatedPoint::fixed(20.0, 80.0),
                AnimatedPoint::fixed(80.0, 20.0),
                sweep.clone(),
            ),
            0.85,
        ),
        stroked(
            ellipse(50.0, 50.0, 44.0),
            stroke(
                linear(
                    AnimatedPoint::fixed(10.0, 90.0),
                    AnimatedPoint::fixed(90.0, 10.0),
                    sweep,
                ),
                4.0,
                LineCap::Butt,
            ),
            None,
        ),
    ])
}

/// Gradients whose ends move: a linear one on a square, a radial one
/// whose circle grows, on a circle.
fn sweep() -> Scene {
    let linear_paint = linear(
        point(&[(0.0, 10.0, 10.0), (2.0, 60.0, 30.0)]),
        point(&[(0.0, 90.0, 20.0), (1.0, 30.0, 90.0), (2.0, 70.0, 80.0)]),
        Animated::Static(stops(&[(0.0, RED), (0.4, GOLD), (1.0, BLUE)])),
    );
    let center = point(&[(0.0, 70.0, 30.0), (2.0, 76.0, 24.0)]);
    let radial = Paint::RadialGradient(RadialGradient {
        center: center.clone(),
        radius: track(&[(0.0, 8.0), (2.0, 22.0)]),
        focal: center,
        stops: Animated::Static(stops(&[(0.0, WHITE), (0.5, GOLD), (1.0, RED)])),
        units: GradientUnits::UserSpace,
        transform: Matrix::IDENTITY,
        spread: Spread::Pad,
    });
    scene(vec![
        filled(
            ShapePath::new(Geometry::Rect {
                center: AnimatedPoint::fixed(35.0, 65.0),
                size: AnimatedPoint::fixed(60.0, 60.0),
                radius: AnimatedPoint::fixed(6.0, 6.0),
                direction: Direction::Clockwise,
                start: RectStart::TopLeft,
            }),
            linear_paint,
            0.9,
        ),
        filled(ellipse(73.0, 27.0, 22.0), radial, 1.0),
    ])
}

/// Gradient colors that change, fading to transparent, on a fill and on a
/// stroke whose gradient moves too.
fn fade() -> Scene {
    let colors = Animated::Track(Track::new(vec![
        Keyframe::new(
            0.0,
            stops(&[(0.1, BLUE), (1.0, Color::rgba(1.0, 1.0, 1.0, 0.0))]),
            Easing::EASE_IN_OUT,
        ),
        Keyframe::new(
            1.0,
            stops(&[(0.1, RED), (1.0, Color::rgba(1.0, 1.0, 1.0, 0.0))]),
            Easing::EASE_IN_OUT,
        ),
        Keyframe::new(
            2.0,
            stops(&[(0.1, GOLD), (1.0, Color::rgba(1.0, 1.0, 1.0, 0.0))]),
            Easing::EASE_IN_OUT,
        ),
    ]));
    scene(vec![
        filled(
            ShapePath::new(Geometry::Rect {
                center: AnimatedPoint::fixed(50.0, 27.5),
                size: AnimatedPoint::fixed(80.0, 35.0),
                radius: AnimatedPoint::fixed(6.0, 6.0),
                direction: Direction::Clockwise,
                start: RectStart::TopLeft,
            }),
            linear(
                AnimatedPoint::fixed(10.0, 0.0),
                AnimatedPoint::fixed(90.0, 0.0),
                colors.clone(),
            ),
            0.9,
        ),
        stroked(
            ellipse(50.0, 72.0, 18.0),
            stroke(
                linear(
                    point(&[(0.0, 30.0, 60.0), (2.0, 40.0, 90.0)]),
                    point(&[(0.0, 70.0, 80.0), (2.0, 75.0, 55.0)]),
                    colors,
                ),
                6.0,
                LineCap::Round,
            ),
            None,
        ),
    ])
}

/// The check set's own scenes, by name.
pub fn built_in(fixtures: &std::path::Path) -> Result<Vec<(String, Scene)>, String> {
    let read = |name: &str| -> Result<Vec<u8>, String> {
        std::fs::read(fixtures.join(name)).map_err(|e| format!("{name}: {e}"))
    };
    let svg = |data: &[u8]| {
        dxn_vector_animation_svg::read(data)
            .map(|i| i.scene)
            .map_err(|e| e.to_string())
    };
    Ok(vec![
        ("figma".into(), svg(&read("figma-motion.svg")?)?),
        (
            "lottie".into(),
            dxn_vector_animation_lottie::read(&read("lottie-motion.json")?)
                .map(|i| i.scene)
                .map_err(|e| e.to_string())?,
        ),
        ("ring".into(), svg(RING.as_bytes())?),
        ("drawn-on".into(), svg(DRAWN_ON.as_bytes())?),
        ("trims".into(), trims()),
        ("loader".into(), loader()),
        ("shine".into(), shine()),
        ("sweep".into(), sweep()),
        ("fade".into(), fade()),
        ("shadow".into(), svg(SHADOW.as_bytes())?),
        ("blur".into(), svg(BLUR.as_bytes())?),
        ("soft-mask".into(), svg(SOFT_MASK.as_bytes())?),
    ])
}
