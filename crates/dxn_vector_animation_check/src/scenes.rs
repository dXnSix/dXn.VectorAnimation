//! The check set's own scenes: one or more for each kind of animation the
//! player draws on the graph, so a platform's run covers them without the
//! corpus (whose files are not ours to commit).

use dxn_vector_animation::{
    Animated, AnimatedPoint, Color, Direction, Easing, EllipseStart, Fill, Geometry, GradientStop,
    GradientUnits, Group, Keyframe, LineCap, LineJoin, LinearGradient, LoopMode, Matrix, Node,
    Paint, RadialGradient, RectStart, Scene, Shape, ShapePath, Spread, Star, StarKind, Stroke,
    Timeline, Track, Trim, TrimMode,
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

/// Masks whose shapes turn: a clip (opaque), a soft mask of one shape
/// (half transparent) and one of two overlapping shapes turning apart.
pub const TURNING_MASKS: [&str; 3] = [
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>@keyframes turn { to { transform: rotate(360deg); } } .t { transform-origin: 50px 50px; animation: turn 2s linear infinite; }</style>
  <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100" style="mask-type: alpha">
    <rect class="t" x="20" y="35" width="60" height="30" fill="#000"/>
  </mask>
  <g mask="url(#m)"><rect width="100" height="100" fill="#F24822"/><circle cx="50" cy="50" r="18" fill="#1E5AE6"/></g>
</svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>@keyframes turn { to { transform: rotate(360deg); } } .t { transform-origin: 50px 50px; animation: turn 2s linear infinite; }</style>
  <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100" style="mask-type: alpha">
    <rect class="t" x="20" y="35" width="60" height="30" fill="#000" fill-opacity="0.5"/>
  </mask>
  <g mask="url(#m)"><rect width="100" height="100" fill="#F24822"/><circle cx="50" cy="50" r="18" fill="#1E5AE6"/></g>
</svg>"##,
    r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>@keyframes turn { to { transform: rotate(360deg); } } @keyframes back { to { transform: rotate(-360deg); } }
    .t { transform-origin: 50px 50px; animation: turn 2s linear infinite; } .b { transform-origin: 50px 50px; animation: back 2s linear infinite; }</style>
  <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100" style="mask-type: alpha">
    <rect class="t" x="20" y="35" width="60" height="30" fill="#000" fill-opacity="0.6"/>
    <rect class="b" x="30" y="20" width="25" height="60" fill="#000" fill-opacity="0.6"/>
  </mask>
  <g mask="url(#m)"><rect width="100" height="100" fill="#F24822"/><circle cx="50" cy="50" r="18" fill="#1E5AE6"/></g>
</svg>"##,
];

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
            loop_start: 0.0,
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

/// Shadows that move and show through: a card whose shadow falls further
/// as it rises, a half-clear disc whose shadow shows under it (CSS
/// `drop-shadow()`, cast by coverage, not knocked out), a ring whose
/// shadow changes hue.
pub const MOVING_SHADOWS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes rise { 0% { filter: drop-shadow(0px 1px 2px rgba(20, 0, 60, 0.5)); } 100% { filter: drop-shadow(6px 9px 3px rgba(20, 0, 60, 0.5)); } }
    @keyframes glow { 0% { filter: drop-shadow(2px 3px 2px rgba(230, 30, 20, 0.7)); } 100% { filter: drop-shadow(2px 3px 2px rgba(20, 120, 230, 0.7)); } }
    #card { animation: rise 2s ease-in-out infinite alternate; }
    #ring { animation: glow 2s linear infinite alternate; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g id="card"><rect x="10" y="10" width="44" height="32" rx="5" fill="#F24822"/></g>
  <g style="filter: drop-shadow(4px 5px 2px rgba(20, 0, 60, 0.8))"><circle cx="68" cy="34" r="20" fill="#1E5AE6" fill-opacity="0.45"/></g>
  <g id="ring"><circle cx="40" cy="72" r="16" fill="none" stroke="#F5B800" stroke-width="7"/></g>
</svg>"##;

/// Figma's shadows (hard silhouettes, knocked out) of a card clipping its
/// content, turning and shrinking, and of a half-clear one.
pub const CARD_SHADOWS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes turn { 0% { transform: rotate(-15deg) scale(1); } 100% { transform: rotate(25deg) scale(0.8); } }
    #turning { transform-origin: 38px 36px; animation: turn 2s ease-in-out infinite alternate; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g id="turning"><g filter="url(#figma)"><g clip-path="url(#card)">
    <rect x="14" y="14" width="48" height="44" rx="8" fill="#FFFFFF"/>
    <circle cx="56" cy="50" r="18" fill="#1E5AE6"/>
  </g></g></g>
  <g filter="url(#figma)"><rect x="56" y="58" width="34" height="30" rx="6" fill="#F24822" fill-opacity="0.3"/></g>
  <defs>
    <clipPath id="card"><rect x="14" y="14" width="48" height="44" rx="8" fill="white"/></clipPath>
    <filter id="figma" x="-50" y="-50" width="200" height="200" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
      <feFlood flood-opacity="0" result="BackgroundImageFix"/>
      <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
      <feOffset dx="3" dy="6"/>
      <feGaussianBlur stdDeviation="3"/>
      <feComposite in2="hardAlpha" operator="out"/>
      <feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.45 0"/>
      <feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow"/>
      <feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow" result="shape"/>
    </filter>
  </defs>
</svg>"##;

/// Masks drawn as images of what they hide: a gradient fading content
/// out, six half-clear discs overlapping, and a mask sliding and turning
/// as a whole over still content.
pub const MASK_IMAGES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes slide { 0% { transform: translateX(-12px); } 100% { transform: translateX(12px); } }
    @keyframes turn { 0% { transform: translate(-10px, 0) rotate(-20deg); } 100% { transform: translate(10px, 0) rotate(30deg); } }
    .slide { animation: slide 2s ease-in-out infinite alternate; }
    #moving { transform-origin: 50px 78px; animation: turn 2s ease-in-out infinite alternate; }
  </style>
  <linearGradient id="fade" x1="10" y1="0" x2="90" y2="0" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="#000" stop-opacity="1"/><stop offset="1" stop-color="#000" stop-opacity="0"/>
  </linearGradient>
  <mask id="gradient" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100" style="mask-type: alpha">
    <rect x="5" y="4" width="90" height="26" fill="url(#fade)"/>
  </mask>
  <mask id="discs" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100" style="mask-type: alpha">
    <circle cx="30" cy="46" r="9" fill="#000" fill-opacity="0.5"/><circle cx="40" cy="42" r="9" fill="#000" fill-opacity="0.5"/>
    <circle cx="50" cy="46" r="9" fill="#000" fill-opacity="0.5"/><circle cx="60" cy="42" r="9" fill="#000" fill-opacity="0.5"/>
    <circle cx="70" cy="46" r="9" fill="#000" fill-opacity="0.5"/><circle cx="50" cy="52" r="9" fill="#000" fill-opacity="0.5"/>
  </mask>
  <mask id="mover" maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100" style="mask-type: alpha">
    <g id="moving"><rect x="30" y="66" width="40" height="22" rx="6" fill="#000" fill-opacity="0.8"/><circle cx="50" cy="77" r="6" fill="#000" fill-opacity="0.4"/></g>
  </mask>
  <g mask="url(#gradient)"><g class="slide"><rect x="10" y="4" width="80" height="26" fill="#F24822"/><rect x="40" y="8" width="20" height="18" fill="#1E5AE6"/></g></g>
  <g mask="url(#discs)"><rect x="15" y="30" width="70" height="34" fill="#1E5AE6"/></g>
  <g mask="url(#mover)"><rect x="10" y="62" width="80" height="34" fill="#F5B800"/><rect x="44" y="62" width="12" height="34" fill="#F24822"/></g>
</svg>"##;

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
        ("turning-clip".into(), svg(TURNING_MASKS[0].as_bytes())?),
        ("turning-mask".into(), svg(TURNING_MASKS[1].as_bytes())?),
        ("turning-masks".into(), svg(TURNING_MASKS[2].as_bytes())?),
        ("moving-shadows".into(), svg(MOVING_SHADOWS.as_bytes())?),
        ("card-shadows".into(), svg(CARD_SHADOWS.as_bytes())?),
        ("mask-images".into(), svg(MASK_IMAGES.as_bytes())?),
        ("trims-moving".into(), trims_moving()),
        ("stop-counts".into(), stop_counts()),
        ("dash-slots".into(), dash_slots()),
        ("switching-star".into(), switching_star()),
        ("live-text".into(), svg(LIVE_TEXT.as_bytes())?),
        ("inner-shadows".into(), svg(INNER_SHADOWS.as_bytes())?),
        ("color-effects".into(), svg(COLOR_EFFECTS.as_bytes())?),
        ("patterns".into(), svg(PATTERNS.as_bytes())?),
        ("shadow-images".into(), svg(SHADOW_IMAGES.as_bytes())?),
        ("moving-layers".into(), svg(MOVING_LAYERS.as_bytes())?),
        ("clear-stops".into(), clear_stops()),
    ])
}

/// A gradient whose middle stop fades in from clear as it moves, then out
/// again.
fn clear_stops() -> Scene {
    let red = Color::rgba(0.95, 0.28, 0.13, 1.0);
    let gold = Color::rgba(0.96, 0.72, 0.0, 1.0);
    let clear = Color::rgba(0.12, 0.35, 0.9, 0.0);
    let track = Animated::Track(Track::new(vec![
        Keyframe::new(
            0.0,
            stops(&[(0.0, red), (0.3, clear), (1.0, red)]),
            Easing::EASE_IN_OUT,
        ),
        Keyframe::new(
            1.0,
            stops(&[(0.0, red), (0.7, gold), (1.0, red)]),
            Easing::EASE_IN_OUT,
        ),
        Keyframe::new(
            2.0,
            stops(&[(0.0, red), (0.3, clear), (1.0, red)]),
            Easing::Linear,
        ),
    ]));
    scene(vec![filled(
        square(10.0, 10.0, 80.0),
        linear(
            point(&[(0.0, 10.0, 0.0)]),
            point(&[(0.0, 90.0, 0.0)]),
            track,
        ),
        1.0,
    )])
}

/// Live text: a centered title growing and changing color, a line sliding
/// in, a stroked word.
pub const LIVE_TEXT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes hue { 0% { fill: #1E5AE6; } 100% { fill: #F24822; } }
    @keyframes slide { 0% { transform: translateX(-30px); opacity: 0; } 100% { transform: translateX(0px); opacity: 1; } }
    #title { animation: hue 2s ease-in-out infinite alternate; }
    #line { animation: slide 2s ease-out infinite alternate; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <text id="title" x="50" y="34" font-family="sans-serif" font-size="18" font-weight="bold" text-anchor="middle">Motion<animate attributeName="font-size" values="14;20;14" dur="2s" repeatCount="indefinite"/></text>
  <text id="line" x="10" y="60" font-family="sans-serif" font-size="11" fill="#333">sliding in</text>
  <text x="90" y="88" font-family="sans-serif" font-size="16" font-weight="bold" fill="#F5B800" stroke="#222" stroke-width="1" text-anchor="end" paint-order="stroke">Bold</text>
</svg>"##;

/// Inner shadows as Figma exports them: a card's, under a drop shadow, and
/// a moving disc's.
pub const INNER_SHADOWS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes bob { 0% { transform: translateY(-8px); } 100% { transform: translateY(8px); } }
    #disc { animation: bob 2s ease-in-out infinite alternate; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g filter="url(#card)"><rect x="10" y="12" width="54" height="40" rx="8" fill="#5B8DEF"/></g>
  <g id="disc"><g filter="url(#well)"><circle cx="66" cy="66" r="22" fill="#F5B800"/></g></g>
  <defs>
    <filter id="card" x="0" y="0" width="100" height="100" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
      <feFlood flood-opacity="0" result="BackgroundImageFix"/>
      <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
      <feOffset dy="4"/><feGaussianBlur stdDeviation="3"/><feComposite in2="hardAlpha" operator="out"/>
      <feColorMatrix type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.3 0"/>
      <feBlend mode="normal" in2="BackgroundImageFix" result="effect1_dropShadow"/>
      <feBlend mode="normal" in="SourceGraphic" in2="effect1_dropShadow" result="shape"/>
      <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
      <feOffset dx="3" dy="3"/><feGaussianBlur stdDeviation="2"/>
      <feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
      <feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.6 0"/>
      <feBlend mode="normal" in2="shape" result="effect2_innerShadow"/>
    </filter>
    <filter id="well" x="0" y="0" width="100" height="100" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB">
      <feFlood flood-opacity="0" result="BackgroundImageFix"/>
      <feBlend mode="normal" in="SourceGraphic" in2="BackgroundImageFix" result="shape"/>
      <feColorMatrix in="SourceAlpha" type="matrix" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" result="hardAlpha"/>
      <feOffset dx="-2" dy="5"/><feGaussianBlur stdDeviation="3"/>
      <feComposite in2="hardAlpha" operator="arithmetic" k2="-1" k3="1"/>
      <feColorMatrix type="matrix" values="0 0 0 0 0.4 0 0 0 0 0.2 0 0 0 0 0 0 0 0 0.5 0"/>
      <feBlend mode="normal" in2="shape" result="effect1_innerShadow"/>
    </filter>
  </defs>
</svg>"##;

/// Color effects: shapes turning gray and back, a hue going round.
pub const COLOR_EFFECTS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes gray { 0% { filter: grayscale(0) brightness(1); } 100% { filter: grayscale(1) brightness(1.2); } }
    @keyframes hue { 0% { filter: hue-rotate(0deg); } 100% { filter: hue-rotate(300deg); } }
    #a { animation: gray 2s ease-in-out infinite alternate; }
    #b { animation: hue 2s linear infinite; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g id="a"><rect x="8" y="8" width="44" height="44" rx="6" fill="#F24822"/><circle cx="40" cy="40" r="16" fill="#1E5AE6" fill-opacity="0.8"/></g>
  <g id="b"><rect x="50" y="52" width="42" height="40" rx="20" fill="url(#g)"/></g>
  <defs><linearGradient id="g"><stop offset="0" stop-color="#F5B800"/><stop offset="1" stop-color="#E5007A"/></linearGradient></defs>
</svg>"##;

/// Pattern fills: a tiling turned in a moving circle, and Figma's image
/// fill (one tile the size of the box).
pub const PATTERNS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes drift { 0% { transform: translateX(-10px); } 100% { transform: translateX(10px); } }
    #disc { animation: drift 2s ease-in-out infinite alternate; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g id="disc"><circle cx="40" cy="40" r="30" fill="url(#checks)"/></g>
  <rect x="55" y="62" width="40" height="30" rx="4" fill="url(#pattern0)"/>
  <defs>
    <pattern id="checks" x="2" y="2" width="12" height="12" patternUnits="userSpaceOnUse" patternTransform="rotate(20)">
      <rect width="6" height="6" fill="#1E5AE6"/><circle cx="9" cy="9" r="3" fill="#F24822"/>
    </pattern>
    <pattern id="pattern0" patternContentUnits="objectBoundingBox" width="1" height="1">
      <use xlink:href="#image0" transform="scale(0.25 0.333)"/>
    </pattern>
    <g id="image0"><rect width="4" height="3" fill="#F5B800"/><rect width="2" height="1.5" fill="#2EAD5A"/><rect x="2" y="1.5" width="2" height="1.5" fill="#2EAD5A"/></g>
  </defs>
</svg>"##;

/// Shadows of what the canvas cannot cast as shapes: a card whose content
/// is clipped by a circle, and a group whose disc casts a shadow itself,
/// both turning.
pub const SHADOW_IMAGES: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <style>
    @keyframes turn { 0% { transform: rotate(-12deg); } 100% { transform: rotate(12deg); } }
    #a, #b { animation: turn 2s ease-in-out infinite alternate; transform-box: fill-box; transform-origin: center; }
  </style>
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g id="a" filter="url(#s)"><g clip-path="url(#c)"><rect x="8" y="8" width="44" height="44" fill="#5B8DEF"/><rect x="8" y="8" width="22" height="22" fill="#F24822"/></g></g>
  <g id="b" filter="url(#s)"><rect x="52" y="50" width="36" height="36" rx="5" fill="#2EAD5A"/><g filter="url(#i)"><circle cx="70" cy="68" r="10" fill="#F5B800"/></g></g>
  <defs>
    <clipPath id="c"><circle cx="30" cy="30" r="20"/></clipPath>
    <filter id="s" x="-50" y="-50" width="200" height="200" filterUnits="userSpaceOnUse"><feDropShadow dx="3" dy="5" stdDeviation="3" flood-color="#000" flood-opacity="0.4"/></filter>
    <filter id="i" x="-50" y="-50" width="200" height="200" filterUnits="userSpaceOnUse"><feDropShadow dx="-2" dy="2" stdDeviation="1.5" flood-color="#A00" flood-opacity="0.6"/></filter>
  </defs>
</svg>"##;

/// Groups that only move or fade (layers the platform moves): a badge
/// turning and growing along a path, a card fading.
pub const MOVING_LAYERS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100" viewBox="0 0 100 100">
  <rect width="100" height="100" fill="#F4F1EA"/>
  <g><animateTransform attributeName="transform" type="translate" values="25 30; 70 45; 40 70" dur="2s" repeatCount="indefinite"/>
    <g><animateTransform attributeName="transform" type="rotate" from="0" to="360" dur="2s" repeatCount="indefinite"/>
      <g><animateTransform attributeName="transform" type="scale" values="0.6; 1.2; 0.6" dur="2s" repeatCount="indefinite"/>
        <rect x="-14" y="-10" width="28" height="20" rx="5" fill="#1E5AE6"/><path d="M-8 0 L-2 6 L9 -6" fill="none" stroke="#fff" stroke-width="3" stroke-linecap="round"/>
      </g>
    </g>
  </g>
  <g><animate attributeName="opacity" values="1; 0.15; 1" dur="2s" repeatCount="indefinite"/>
    <rect x="58" y="8" width="34" height="24" rx="4" fill="#F24822"/><rect x="62" y="14" width="20" height="3" fill="#fff"/>
  </g>
</svg>"##;

/// A star turning and deepening, drawn on and off, filled and stroked
/// through a trim; a disc's fill trimmed as it runs round.
fn trims_moving() -> Scene {
    let star = ShapePath::new(Geometry::Star(Star {
        kind: StarKind::Star,
        center: AnimatedPoint::fixed(36.0, 40.0),
        points: Animated::Static(5.0),
        outer_radius: Animated::Static(28.0),
        inner_radius: track(&[(0.0, 10.0), (2.0, 18.0)]),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: track(&[(0.0, 0.0), (2.0, 72.0)]),
        direction: Direction::Clockwise,
    }));
    let trim = |start: Animated<f64>, end: Animated<f64>, offset: Animated<f64>| {
        Some(Trim {
            start,
            end,
            offset,
            mode: TrimMode::Combined,
        })
    };
    scene(vec![
        Node::shape(Shape {
            paths: vec![star],
            fill: Some(Fill {
                paint: Paint::solid(GOLD),
                opacity: Animated::Static(1.0),
                rule: Default::default(),
            }),
            stroke: Some(stroke(Paint::solid(BLUE), 3.0, LineCap::Round)),
            trim: trim(
                track(&[(0.0, 0.0), (2.0, 0.4)]),
                track(&[(0.0, 0.3), (1.0, 1.0)]),
                Animated::Static(0.0),
            ),
            ..Default::default()
        }),
        Node::shape(Shape {
            paths: vec![ellipse(68.0, 70.0, 22.0)],
            fill: Some(Fill {
                paint: Paint::solid(RED),
                opacity: Animated::Static(1.0),
                rule: Default::default(),
            }),
            trim: trim(
                Animated::Static(0.0),
                track(&[(0.0, 0.3), (2.0, 0.8)]),
                track(&[(0.0, 0.0), (2.0, 0.9)]),
            ),
            ..Default::default()
        }),
    ])
}

/// A gradient gaining and losing stops as they move.
fn stop_counts() -> Scene {
    let keys = vec![
        (0.0, stops(&[(0.0, RED), (1.0, BLUE)]), Easing::EASE_IN_OUT),
        (
            0.8,
            stops(&[(0.0, RED), (0.5, GOLD), (1.0, BLUE)]),
            Easing::Linear,
        ),
        (1.3, stops(&[(0.2, GOLD), (0.9, BLUE)]), Easing::Linear),
        (2.0, stops(&[(0.5, RED), (1.0, GOLD)]), Easing::Linear),
    ];
    let paint = linear(
        AnimatedPoint::fixed(8.0, 0.0),
        AnimatedPoint::fixed(92.0, 0.0),
        Animated::Track(Track::new(
            keys.into_iter()
                .map(|(t, v, e)| Keyframe::new(t, v, e))
                .collect(),
        )),
    );
    scene(vec![filled(square(8.0, 20.0, 84.0), paint, 1.0)])
}

/// A hundred dashes round a circle growing, their offset running on.
fn dash_slots() -> Scene {
    let mut s = stroke(Paint::solid(BLUE), 4.0, LineCap::Round);
    s.dash = Some(dxn_vector_animation::Dash {
        array: vec![track(&[(0.0, 0.5), (2.0, 3.0)]), Animated::Static(2.5)],
        offset: track(&[(0.0, 0.0), (2.0, -60.0)]),
    });
    scene(vec![stroked(ellipse(50.0, 50.0, 40.0), s, None)])
}

/// A star of five points and six, forty times over, turning.
fn switching_star() -> Scene {
    let star = ShapePath::new(Geometry::Star(Star {
        kind: StarKind::Star,
        center: AnimatedPoint::fixed(50.0, 50.0),
        points: Animated::Track(Track::new(
            (0..=40)
                .map(|k| {
                    let points = if k % 2 == 0 { 5.0 } else { 6.0 };
                    Keyframe::new(f64::from(k) * 0.05, points, Easing::Hold)
                })
                .collect(),
        )),
        outer_radius: Animated::Static(40.0),
        inner_radius: Animated::Static(18.0),
        outer_roundness: Animated::Static(0.0),
        inner_roundness: Animated::Static(0.0),
        rotation: track(&[(0.0, 0.0), (2.0, 144.0)]),
        direction: Direction::Clockwise,
    }));
    scene(vec![Node::shape(Shape {
        paths: vec![star],
        fill: Some(Fill {
            paint: Paint::solid(RED),
            opacity: Animated::Static(1.0),
            rule: Default::default(),
        }),
        stroke: Some(stroke(Paint::solid(BLUE), 3.0, LineCap::Round)),
        ..Default::default()
    })])
}
