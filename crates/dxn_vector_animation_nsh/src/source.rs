//! What the player plays (`src`) and how it is read into a scene, and the
//! props that take more than one kind of value (`repeat`, `segment`,
//! `posterFrame`).

use dxn_vector_animation::{Limits, Scene, compiled};
use dxn_vector_animation_rive::Interaction;
use nsh_core::element::{JsHost, JsIn, JsKind, JsOwned, JsValue, PropError, TsTypes};

/// What a `VectorAnimation` plays.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// A URL (`https://…`, or an imported file's `asset:/…`), fetched.
    Url(String),
    /// The text of an SVG, a Lottie JSON or a compiled scene.
    Text(String),
    /// The bytes of one (`Uint8Array`).
    Bytes(Vec<u8>),
    /// A Lottie or a compiled scene as a JSON value (a `.json` import).
    Json(serde_json::Value),
}

impl Source {
    /// The source of `url`.
    pub fn url(url: impl Into<String>) -> Self {
        Source::Url(url.into())
    }

    /// The source of a file's bytes (`include_bytes!`).
    pub fn bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Source::Bytes(bytes.into())
    }
}

fn looks_like_url(s: &str) -> bool {
    let s = s.trim_start();
    ["http://", "https://", "asset:", "file:", "data:"]
        .iter()
        .any(|p| s.starts_with(p))
        || (!s.starts_with('<') && !s.starts_with('{') && !s.contains('\n') && s.len() < 2048)
}

impl JsValue for Source {
    fn ts_type(_types: &mut TsTypes) -> String {
        "string | { url: string } | Uint8Array | object".into()
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if let Some(s) = value.as_string() {
            return Ok(if looks_like_url(&s) {
                Source::Url(s)
            } else {
                Source::Text(s)
            });
        }
        if let Some(bytes) = value.as_bytes() {
            return Ok(Source::Bytes(bytes));
        }
        if value.kind() == JsKind::Object {
            // An imported file: `{ url, size }`.
            if let Some(url) = value.field("url")?.get().as_string()
                && value.field("layers")?.get().is_nullish()
            {
                return Ok(Source::Url(url));
            }
            return Ok(Source::Json(value.to_json()?));
        }
        Err(value.expected("a URL, the text or bytes of an animation, or its JSON"))
    }

    fn to_js<'a>(&self, _host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        Err(PropError(
            "a source is JS's to give, not to get back".to_owned(),
        ))
    }
}

/// A scene read from a file, with what it leaves out.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The scene.
    pub scene: Scene,
    /// What the file holds that the scene leaves out.
    pub warnings: Vec<String>,
    /// A Rive file's default state machine as the user plays it: its
    /// inputs set and its listeners' pointer events, each read into the
    /// scene again.
    pub interaction: Option<Interaction>,
}

/// Reads `bytes` (an SVG, a Lottie JSON, a dotLottie archive, a Rive file
/// or a compiled scene).
pub fn read(bytes: &[u8]) -> Result<Loaded, String> {
    let limits = Limits::default();
    if dxn_vector_animation_rive::sniff(bytes) {
        let play = Interaction::new(bytes.to_vec(), limits, Default::default())
            .map_err(|e| e.to_string())?
            .forward();
        let import = play.import();
        return Ok(Loaded {
            scene: import.scene.clone(),
            warnings: import.warnings.clone(),
            interaction: Some(play),
        });
    }
    if compiled::sniff(bytes) {
        return compiled::from_json(bytes, &limits)
            .map(|scene| Loaded {
                scene,
                warnings: Vec::new(),
                interaction: None,
            })
            .map_err(|e| e.to_string());
    }
    let json = bytes.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{');
    let import =
        if dxn_vector_animation_lottie::sniff(bytes) || bytes.starts_with(b"PK\x03\x04") || json {
            dxn_vector_animation_lottie::read_with(bytes, &limits)
        } else if dxn_vector_animation_svg::sniff(bytes) {
            dxn_vector_animation_svg::read_with(bytes, &limits)
        } else {
            return Err("neither SVG, Lottie, Rive nor a compiled scene".into());
        };
    import
        .map(|i| Loaded {
            scene: i.scene,
            warnings: i.warnings,
            interaction: None,
        })
        .map_err(|e| e.to_string())
}

/// Reads a JSON value (a Lottie or a compiled scene).
pub fn read_json(value: &serde_json::Value) -> Result<Loaded, String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    read(&bytes)
}

/// What a state machine's input is set to (`setInput`): a bool or a number.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputSetting {
    /// A bool input.
    Bool(bool),
    /// A number input.
    Number(f64),
}

impl JsValue for InputSetting {
    fn ts_type(_types: &mut TsTypes) -> String {
        "boolean | number".into()
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if let Some(b) = value.as_bool() {
            return Ok(InputSetting::Bool(b));
        }
        match value.as_number() {
            Some(n) => Ok(InputSetting::Number(n)),
            None => Err(value.expected("a boolean or a number")),
        }
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let raw = match self {
            InputSetting::Bool(b) => host.new_bool(*b),
            InputSetting::Number(n) => host.new_number(*n),
        };
        Ok(JsOwned::new(host, raw))
    }
}

/// What a view model property is set to (`setValue`): a bool, a number
/// (a number's, an enum's index or a color's ARGB) or a string (a
/// string's, an enum's value or a color's `#rrggbb` or `#aarrggbb`).
#[derive(Debug, Clone, PartialEq)]
pub enum ValueSetting {
    /// A bool.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    Text(String),
}

impl JsValue for ValueSetting {
    fn ts_type(_types: &mut TsTypes) -> String {
        "boolean | number | string".into()
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if let Some(b) = value.as_bool() {
            return Ok(ValueSetting::Bool(b));
        }
        if let Some(n) = value.as_number() {
            return Ok(ValueSetting::Number(n));
        }
        match value.as_string() {
            Some(s) => Ok(ValueSetting::Text(s)),
            None => Err(value.expected("a boolean, a number or a string")),
        }
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let raw = match self {
            ValueSetting::Bool(b) => host.new_bool(*b),
            ValueSetting::Number(n) => host.new_number(*n),
            ValueSetting::Text(s) => host.new_string(s)?,
        };
        Ok(JsOwned::new(host, raw))
    }
}

/// How many times an animation plays (`repeat`): `true` forever, a count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Repeat {
    /// Once (`false`).
    Once,
    /// Forever (`true`).
    Forever,
    /// This many times in all.
    Times(u32),
}

impl JsValue for Repeat {
    fn ts_type(_types: &mut TsTypes) -> String {
        "boolean | number".into()
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if let Some(b) = value.as_bool() {
            return Ok(if b { Repeat::Forever } else { Repeat::Once });
        }
        match value.as_number() {
            Some(n) if n.is_infinite() && n > 0.0 => Ok(Repeat::Forever),
            Some(n) if n >= 1.0 => Ok(Repeat::Times(n.min(f64::from(u32::MAX)) as u32)),
            Some(_) => Ok(Repeat::Once),
            None => Err(value.expected("a boolean or a number of times")),
        }
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let raw = match self {
            Repeat::Once => host.new_bool(false),
            Repeat::Forever => host.new_bool(true),
            Repeat::Times(n) => host.new_number(f64::from(*n)),
        };
        Ok(JsOwned::new(host, raw))
    }
}

/// A time on the timeline: seconds, or a marker's name.
#[derive(Debug, Clone, PartialEq)]
pub enum At {
    /// Seconds.
    Seconds(f64),
    /// A marker's start.
    Marker(String),
}

/// What part of the timeline plays (`segment`): a marker's span, or
/// `[start, end]` in seconds.
#[derive(Debug, Clone, PartialEq)]
pub enum Segment {
    /// A marker's span.
    Marker(String),
    /// From, to (seconds).
    Span(f64, f64),
}

impl JsValue for Segment {
    fn ts_type(_types: &mut TsTypes) -> String {
        "string | [number, number]".into()
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if let Some(s) = value.as_string() {
            return Ok(Segment::Marker(s));
        }
        if value.array_len()? == Some(2) {
            let a = value.index(0)?.get().as_number();
            let b = value.index(1)?.get().as_number();
            if let (Some(a), Some(b)) = (a, b) {
                return Ok(Segment::Span(a, b));
            }
        }
        Err(value.expected("a marker's name or [start, end] in seconds"))
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let raw = match self {
            Segment::Marker(m) => host.new_string(m)?,
            Segment::Span(a, b) => {
                host.new_array(vec![host.new_number(*a), host.new_number(*b)])?
            }
        };
        Ok(JsOwned::new(host, raw))
    }
}

/// The frame shown when motion is reduced (`posterFrame`): `"first"`,
/// `"last"`, seconds, or a marker's name.
#[derive(Debug, Clone, PartialEq)]
pub enum PosterFrame {
    /// The segment's first frame.
    First,
    /// Its last frame.
    Last,
    /// A time.
    At(At),
}

impl JsValue for PosterFrame {
    fn ts_type(_types: &mut TsTypes) -> String {
        "\"first\" | \"last\" | number | string".into()
    }

    fn from_js(value: JsIn<'_>) -> Result<Self, PropError> {
        if let Some(n) = value.as_number() {
            return Ok(PosterFrame::At(At::Seconds(n)));
        }
        match value.as_string().as_deref() {
            Some("first") => Ok(PosterFrame::First),
            Some("last") => Ok(PosterFrame::Last),
            Some(m) => Ok(PosterFrame::At(At::Marker(m.to_owned()))),
            None => Err(value.expected("\"first\", \"last\", seconds or a marker's name")),
        }
    }

    fn to_js<'a>(&self, host: &'a dyn JsHost) -> Result<JsOwned<'a>, PropError> {
        let raw = match self {
            PosterFrame::First => host.new_string("first")?,
            PosterFrame::Last => host.new_string("last")?,
            PosterFrame::At(At::Seconds(s)) => host.new_number(*s),
            PosterFrame::At(At::Marker(m)) => host.new_string(m)?,
        };
        Ok(JsOwned::new(host, raw))
    }
}
