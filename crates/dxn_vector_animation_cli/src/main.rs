//! `vector-animation`: inspect, validate, compile and render animated SVG,
//! Lottie, Rive and compiled scenes.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use dxn_vector_animation::{Color, Error, Import, Limits, LoopMode, Point, Scene, compiled};
use dxn_vector_animation_render::{RenderOptions, render_png};

#[derive(Parser)]
#[command(
    name = "vector-animation",
    version,
    about = "Inspect, validate, compile and render animated SVG, Lottie and Rive"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// The artboard of a Rive file to read, by name; its first by default.
    #[arg(long, global = true)]
    artboard: Option<String>,
}

// Parsed once: the render command's many options are no cost.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    /// Prints a file's size, timeline, contents and what it leaves out.
    Inspect {
        /// An animated SVG, a Lottie JSON, a dotLottie, a Rive file or a
        /// compiled scene.
        file: PathBuf,
    },
    /// Reads a file and reports whether it plays; fails on errors.
    Validate {
        /// An animated SVG, a Lottie JSON, a dotLottie, a Rive file or a
        /// compiled scene.
        file: PathBuf,
        /// Fails on warnings too.
        #[arg(long)]
        strict: bool,
    },
    /// Compiles a file into a scene (`.vector-animation.json`).
    Compile {
        /// An animated SVG, a Lottie JSON, a dotLottie, a Rive file or a
        /// compiled scene.
        file: PathBuf,
        /// Where to write (`-` for stdout); next to the file by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Indented JSON.
        #[arg(long)]
        pretty: bool,
    },
    /// Renders frames as PNG images.
    Render {
        /// An animated SVG, a Lottie JSON, a dotLottie, a Rive file or a
        /// compiled scene.
        file: PathBuf,
        /// A PNG file for one frame, or a directory for several; next to
        /// the file by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Times to render, in seconds (repeatable); 0 by default.
        #[arg(short, long = "time")]
        times: Vec<f64>,
        /// Renders the whole timeline at this many frames a second.
        #[arg(long, conflicts_with = "times")]
        fps: Option<f64>,
        /// Renders a marker's span (a Rive animation's) instead: `--fps`
        /// renders all of it, `--time` seconds into it.
        #[arg(long)]
        marker: Option<String>,
        /// Pixels wide (the height keeps the aspect unless given).
        #[arg(long)]
        width: Option<u32>,
        /// Pixels high.
        #[arg(long)]
        height: Option<u32>,
        /// Pixels per user unit when no size is given.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// A background color (`#rrggbb`, `#rrggbbaa`, `white`, `black`,
        /// `transparent`); transparent by default.
        #[arg(long)]
        background: Option<String>,
        /// An event that happened, `name=seconds` (repeatable): what the
        /// file plays on it plays from then (`inspect` lists them).
        #[arg(long = "event")]
        events: Vec<String>,
        /// A change to a Rive state machine's input, `name=value@seconds`
        /// (`true`, `false` or a number), or `name@seconds` to fire a
        /// trigger, a nested artboard's machine's with its path before
        /// (`#12/hover=true@1`; repeatable; `inspect` lists the inputs).
        #[arg(long = "input")]
        inputs: Vec<String>,
        /// A change to a property of the view model instance bound to a
        /// Rive artboard, `path=value@seconds` (`true`, `false`, a number,
        /// `#rrggbb` or `#aarrggbb`, an enum's value, else text), or
        /// `path@seconds` to fire a trigger; nested view models' property
        /// names first in the path, `/` between (`player/score=3@1`;
        /// repeatable; `inspect` lists the properties).
        #[arg(long = "value")]
        values: Vec<String>,
        /// A pointer over a Rive file, `down:x,y@seconds` (or `up:`,
        /// `move:`, `exit:` for leaving, `lift:` for a finger lifted; the
        /// scene's units, repeatable): the state machine's listeners act on
        /// it as Rive's runtime has them act.
        #[arg(long = "pointer")]
        pointers: Vec<String>,
    },
}

/// The file formats read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Svg,
    Lottie,
    Rive,
    Compiled,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Svg => "animated SVG",
            Format::Lottie => "Lottie",
            Format::Rive => "Rive",
            Format::Compiled => "compiled scene",
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command, cli.artboard.as_deref()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn read(
    file: &Path,
    artboard: Option<&str>,
    inputs: &[dxn_vector_animation_rive::InputChange],
    values: &[dxn_vector_animation_rive::ValueChange],
) -> Result<(Format, Import), String> {
    let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let limits = Limits::default();
    let fail = |e: Error| format!("{}: {e}", file.display());
    if dxn_vector_animation_rive::sniff(&data) {
        let options = dxn_vector_animation_rive::Options {
            artboard: artboard.map(str::to_owned),
            inputs: inputs.to_vec(),
            values: values.to_vec(),
            ..Default::default()
        };
        return dxn_vector_animation_rive::read_options(&data, &limits, &options)
            .map(|i| (Format::Rive, i))
            .map_err(fail);
    }
    if compiled::sniff(&data) {
        let scene = compiled::from_json(&data, &limits).map_err(fail)?;
        return Ok((
            Format::Compiled,
            Import {
                scene,
                warnings: Vec::new(),
            },
        ));
    }
    let json = data.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{');
    if dxn_vector_animation_lottie::sniff(&data)
        || data.starts_with(b"PK\x03\x04")
        || json && !dxn_vector_animation_svg::sniff(&data)
    {
        return dxn_vector_animation_lottie::read_with(&data, &limits)
            .map(|i| (Format::Lottie, i))
            .map_err(fail);
    }
    if dxn_vector_animation_svg::sniff(&data) {
        return dxn_vector_animation_svg::read_with(&data, &limits)
            .map(|i| (Format::Svg, i))
            .map_err(fail);
    }
    Err(format!(
        "{}: neither SVG, Lottie, Rive nor a compiled scene",
        file.display()
    ))
}

fn run(command: Command, artboard: Option<&str>) -> Result<ExitCode, String> {
    match command {
        Command::Inspect { file } => {
            let (format, import) = read(&file, artboard, &[], &[])?;
            print!("{}", describe(&file, format, &import));
            if format == Format::Rive {
                print!("{}", describe_machine(&file, artboard, &import.scene)?);
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate { file, strict } => {
            let (format, import) = read(&file, artboard, &[], &[])?;
            for w in &import.warnings {
                println!("warning: {w}");
            }
            if strict && !import.warnings.is_empty() {
                println!("{}: {} warning(s)", file.display(), import.warnings.len());
                return Ok(ExitCode::FAILURE);
            }
            println!("{}: valid {}", file.display(), format.name());
            Ok(ExitCode::SUCCESS)
        }
        Command::Compile {
            file,
            output,
            pretty,
        } => {
            let (_, import) = read(&file, artboard, &[], &[])?;
            for w in &import.warnings {
                eprintln!("warning: {w}");
            }
            let json = compiled::to_json(&import.scene, pretty)
                .map_err(|e| format!("{}: {e}", file.display()))?;
            if output.as_deref() == Some(Path::new("-")) {
                writeln!(std::io::stdout().lock(), "{json}").map_err(|e| format!("stdout: {e}"))?;
                return Ok(ExitCode::SUCCESS);
            }
            let output = output.unwrap_or_else(|| file.with_extension("vector-animation.json"));
            std::fs::write(&output, json).map_err(|e| format!("{}: {e}", output.display()))?;
            println!("{}", output.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Render {
            file,
            output,
            times,
            fps,
            marker,
            width,
            height,
            scale,
            background,
            events,
            inputs,
            values,
            pointers,
        } => {
            let inputs = inputs
                .iter()
                .map(|i| parse_input(i))
                .collect::<Result<Vec<_>, _>>()?;
            let values = values
                .iter()
                .map(|v| parse_value(v))
                .collect::<Result<Vec<_>, _>>()?;
            let (_, import) = if pointers.is_empty() {
                read(&file, artboard, &inputs, &values)?
            } else {
                point(&file, artboard, &inputs, &values, &pointers)?
            };
            let mut happened: Vec<(&str, f64)> = Vec::new();
            for e in &events {
                let parsed = e
                    .rsplit_once('=')
                    .and_then(|(name, t)| Some((name, t.trim().parse::<f64>().ok()?)));
                match parsed {
                    Some(p) => happened.push(p),
                    None => return Err(format!("--event `{e}` is not `name=seconds`")),
                }
            }
            let scene = if happened.is_empty() {
                import.scene
            } else {
                import.scene.with_events(&happened)
            };
            let background = background
                .map(|b| parse_color(&b).ok_or_else(|| format!("unknown color `{b}`")))
                .transpose()?;
            let options = RenderOptions {
                width,
                height,
                scale,
                background,
            };
            let span = match &marker {
                Some(name) => {
                    let m = scene
                        .timeline
                        .markers
                        .iter()
                        .find(|m| &m.name == name)
                        .ok_or_else(|| format!("no marker `{name}` (`inspect` lists them)"))?;
                    (m.start, m.duration.max(0.0))
                }
                None => (0.0, scene.timeline.duration),
            };
            let times = match fps {
                Some(fps) if fps > 0.0 => frame_times(span, fps),
                Some(_) => return Err("--fps must be positive".into()),
                None if times.is_empty() => vec![span.0],
                None => times.iter().map(|t| span.0 + t).collect(),
            };
            let stem = file
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("frame")
                .trim_end_matches(".vector-animation")
                .to_owned();
            let single = times.len() == 1;
            let output = output.unwrap_or_else(|| {
                let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
                if single {
                    dir.join(format!("{stem}.png"))
                } else {
                    dir.join(format!("{stem}-frames"))
                }
            });
            let to_file = single
                && output
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("png"));
            if !to_file {
                std::fs::create_dir_all(&output)
                    .map_err(|e| format!("{}: {e}", output.display()))?;
            }
            for (i, t) in times.iter().enumerate() {
                if let Some(shown) = past_the_loop(&scene, *t).filter(|_| marker.is_none()) {
                    eprintln!(
                        "warning: t={t:.3}s is past the {:.3} s timeline, which loops, and is rendered as \
                         given, not wrapped: the player shows {shown:.3} s there",
                        scene.timeline.duration
                    );
                }
                let png = render_png(&scene.frame_at(*t), &options).map_err(|e| e.to_string())?;
                let path = if to_file {
                    output.clone()
                } else {
                    output.join(format!("{stem}-{i:04}.png"))
                };
                std::fs::write(&path, png).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("{}  t={t:.3}s", path.display());
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Where the player shows `t` seconds of a timeline that repeats, when `t`
/// is past its end (where the scene's tracks hold or run on, not loop).
fn past_the_loop(scene: &Scene, t: f64) -> Option<f64> {
    let timeline = &scene.timeline;
    (t > timeline.duration && timeline.loop_mode != LoopMode::Once).then(|| timeline.time_at(t))
}

/// Rive file `file` played with `inputs` changed and the pointer events
/// `pointers` (`kind:x,y@seconds`) over its listeners.
fn point(
    file: &Path,
    artboard: Option<&str>,
    inputs: &[dxn_vector_animation_rive::InputChange],
    values: &[dxn_vector_animation_rive::ValueChange],
    pointers: &[String],
) -> Result<(Format, Import), String> {
    use dxn_vector_animation_rive::{Interaction, Options, Pointer};
    let mut events = Vec::new();
    for text in pointers {
        let bad = || {
            format!(
                "--pointer `{text}` is not `down:x,y@seconds` (or `up:`, `move:`, `exit:`, `lift:`)"
            )
        };
        let (kind, rest) = text.split_once(':').ok_or_else(bad)?;
        let (xy, time) = rest.rsplit_once('@').ok_or_else(bad)?;
        let (x, y) = xy.split_once(',').ok_or_else(bad)?;
        let kind = match kind {
            "down" => Pointer::Down,
            "up" => Pointer::Up,
            "move" => Pointer::Move,
            "exit" => Pointer::Exit,
            "lift" => Pointer::Lift,
            _ => return Err(bad()),
        };
        let num = |v: &str| v.trim().parse::<f64>().map_err(|_| bad());
        events.push((num(time)?, kind, Point::new(num(x)?, num(y)?)));
    }
    events.sort_by(|a, b| a.0.total_cmp(&b.0));
    let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    if !dxn_vector_animation_rive::sniff(&data) {
        return Err(format!("{}: --pointer needs a Rive file", file.display()));
    }
    let fail = |e: Error| format!("{}: {e}", file.display());
    let options = Options {
        artboard: artboard.map(str::to_owned),
        inputs: inputs.to_vec(),
        values: values.to_vec(),
        ..Options::default()
    };
    let mut play = Interaction::new(data, Limits::default(), options).map_err(fail)?;
    for (time, kind, at) in events {
        play.pointer(time, kind, at).map_err(fail)?;
    }
    // What the pointer did, on stderr.
    for c in play.changes() {
        let path = if c.path.is_empty() {
            String::new()
        } else {
            format!("{}/", c.path)
        };
        let value = match c.value {
            dxn_vector_animation_rive::InputValue::Bool(b) => format!("= {b}"),
            dxn_vector_animation_rive::InputValue::Number(n) => format!("= {n}"),
            dxn_vector_animation_rive::InputValue::Fire => "fired".to_owned(),
        };
        eprintln!("input {path}{} {value} at {:.3} s", c.input, c.time);
    }
    for c in play.take_fired() {
        eprintln!("event {} at {:.3} s", c.name, c.time);
    }
    Ok((Format::Rive, play.import().clone()))
}

/// A state machine input's change: `name=value@seconds`, or `name@seconds`
/// (a nested artboard's machine's with its path before, `/` between)
/// firing a trigger.
fn parse_input(text: &str) -> Result<dxn_vector_animation_rive::InputChange, String> {
    use dxn_vector_animation_rive::{InputChange, InputValue};
    let bad = || format!("--input `{text}` is not `name=value@seconds` or `name@seconds`");
    let (what, time) = text.rsplit_once('@').ok_or_else(bad)?;
    let time = time.trim().parse::<f64>().map_err(|_| bad())?;
    let (input, value) = match what.split_once('=') {
        None => (what, InputValue::Fire),
        Some((name, "true")) => (name, InputValue::Bool(true)),
        Some((name, "false")) => (name, InputValue::Bool(false)),
        Some((name, v)) => (
            name,
            InputValue::Number(v.trim().parse().map_err(|_| bad())?),
        ),
    };
    // A nested artboard's machine's: its path first (`Button/hover`).
    let (path, input) = input.rsplit_once('/').unwrap_or(("", input));
    Ok(InputChange {
        time,
        path: path.to_owned(),
        input: input.to_owned(),
        value,
    })
}

/// A view model property's change: `path=value@seconds`, or `path@seconds`
/// firing a trigger.
fn parse_value(text: &str) -> Result<dxn_vector_animation_rive::ValueChange, String> {
    use dxn_vector_animation_rive::{PropertyValue, ValueChange};
    let bad = || format!("--value `{text}` is not `path=value@seconds` or `path@seconds`");
    let (what, time) = text.rsplit_once('@').ok_or_else(bad)?;
    let time = time.trim().parse::<f64>().map_err(|_| bad())?;
    let (path, value) = match what.split_once('=') {
        None => (what, PropertyValue::Trigger),
        Some((path, "true")) => (path, PropertyValue::Bool(true)),
        Some((path, "false")) => (path, PropertyValue::Bool(false)),
        Some((path, v)) => {
            let hex = v
                .strip_prefix('#')
                .filter(|h| matches!(h.len(), 6 | 8))
                .and_then(|h| u32::from_str_radix(h, 16).ok().map(|c| (h.len(), c)));
            let value = match (hex, v.trim().parse::<f64>()) {
                (Some((6, c)), _) => PropertyValue::Color(0xFF00_0000 | c),
                (Some((_, c)), _) => PropertyValue::Color(c),
                (None, Ok(n)) => PropertyValue::Number(n),
                (None, Err(_)) => PropertyValue::Text(v.to_owned()),
            };
            (path, value)
        }
    };
    Ok(ValueChange {
        time,
        path: path.to_owned(),
        value,
    })
}

/// Every frame time at `fps` of the span from `start` lasting `duration`.
fn frame_times((start, duration): (f64, f64), fps: f64) -> Vec<f64> {
    let count = (duration * fps).floor() as usize;
    (0..=count.min(100_000))
        .map(|i| start + i as f64 / fps)
        .collect()
}

fn describe(file: &Path, format: Format, import: &Import) -> String {
    let scene = &import.scene;
    let t = &scene.timeline;
    let s = scene.stats();
    let mut out = String::new();
    let mut line = |text: String| {
        out.push_str(&text);
        out.push('\n');
    };
    line(format!("{}", file.display()));
    line(format!("  format       {}", format.name()));
    line(format!("  size         {} × {}", scene.width, scene.height));
    line(format!(
        "  timeline     {:.3} s{}, {}",
        t.duration,
        t.frame_rate
            .map(|f| format!(" at {f} fps"))
            .unwrap_or_default(),
        match t.loop_mode {
            LoopMode::Once => "once".to_owned(),
            mode => {
                let name = if mode == LoopMode::Loop {
                    "loops"
                } else {
                    "ping-pong"
                };
                match t.loop_from() {
                    0.0 => name.to_owned(),
                    from => format!("{name} from {from:.3} s"),
                }
            }
        }
    ));
    for m in &t.markers {
        match m.name.strip_prefix(dxn_vector_animation::EVENT_PREFIX) {
            Some(event) => line(format!(
                "  event        {event}: plays for {:.3} s (render --event {event}=<seconds>)",
                m.duration
            )),
            None => line(format!(
                "  marker       {} at {:.3} s for {:.3} s{}",
                m.name,
                m.start,
                m.duration,
                match m.loop_mode {
                    Some(LoopMode::Loop) => ", loops",
                    Some(LoopMode::PingPong) => ", ping-pong",
                    Some(LoopMode::Once) => ", once",
                    None => "",
                }
            )),
        }
    }
    for c in &t.cues {
        let mut what = c.name.clone();
        if let Some((url, target)) = &c.url {
            what.push_str(&format!(", opens {url} ({target})"));
        }
        for (name, value) in &c.properties {
            let value = match value {
                dxn_vector_animation::CueValue::Number(n) => n.to_string(),
                dxn_vector_animation::CueValue::Text(t) => format!("{t:?}"),
                dxn_vector_animation::CueValue::Bool(b) => b.to_string(),
            };
            what.push_str(&format!(", {name} = {value}"));
        }
        line(format!("  cue          {:.3} s: {what}", c.time));
    }
    line(format!(
        "  nodes        {} groups, {} shapes, {} images (depth {})",
        s.groups, s.shapes, s.images, s.depth
    ));
    line(format!(
        "  animation    {} animated properties, {} keyframes",
        s.animated_properties, s.keyframes
    ));
    line(format!("  path points  {}", s.path_points));
    if import.warnings.is_empty() {
        line("  left out     nothing".into());
    }
    for w in &import.warnings {
        line(format!("  left out     {w}"));
    }
    out
}

/// The inputs and listeners of a Rive file's default state machine.
fn describe_machine(file: &Path, artboard: Option<&str>, scene: &Scene) -> Result<String, String> {
    use dxn_vector_animation_rive::{Action, Change, InputKind, InputValue, Options, PointerEvent};
    let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let options = Options {
        artboard: artboard.map(str::to_owned),
        ..Options::default()
    };
    let Some(machine) = dxn_vector_animation_rive::machine(&data, &options)
        .map_err(|e| format!("{}: {e}", file.display()))?
    else {
        return Ok(String::new());
    };
    let mut out = String::new();
    if let Some(vm) = dxn_vector_animation_rive::view_model(&data, &options)
        .map_err(|e| format!("{}: {e}", file.display()))?
    {
        use dxn_vector_animation_rive::PropertyValue;
        let values: Vec<String> = vm
            .properties
            .iter()
            .map(|p| {
                let v = match &p.value {
                    PropertyValue::Number(n) => n.to_string(),
                    PropertyValue::Bool(b) => b.to_string(),
                    PropertyValue::Color(c) => format!("#{:06x}", c & 0xFF_FFFF),
                    PropertyValue::Text(t) => format!("{t:?}"),
                    PropertyValue::Enum(e) => e.map_or("none".to_owned(), |e| format!("value {e}")),
                    PropertyValue::Trigger => "trigger".to_owned(),
                    PropertyValue::Other => "…".to_owned(),
                };
                format!("{} = {v}", p.name)
            })
            .collect();
        out.push_str(&format!(
            "  view model   {} (instance {:?}): {}\n",
            vm.name,
            vm.instance,
            values.join(", ")
        ));
        if !vm.properties.is_empty() {
            out.push_str(
                "               (render --value '<path>=<value>@<seconds>', '<trigger>@<seconds>')\n",
            );
        }
    }
    let all = machine.all();
    for m in &all {
        let at = if m.path.is_empty() {
            String::new()
        } else {
            format!(" (in {})", m.path)
        };
        if m.path.is_empty() || !m.inputs.is_empty() || !m.listeners.is_empty() {
            out.push_str(&format!("  machine      {}{at}\n", m.name));
        }
        let path = if m.path.is_empty() {
            String::new()
        } else {
            format!("{}/", m.path)
        };
        for i in &m.inputs {
            let kind = match i.kind {
                InputKind::Bool(v) => format!("bool, {v}"),
                InputKind::Number(v) => format!("number, {v}"),
                InputKind::Trigger => "trigger".to_owned(),
            };
            out.push_str(&format!("  input        {path}{}: {kind}\n", i.name));
        }
        for l in &m.listeners {
            let mut on: Vec<String> =
                l.on.iter()
                    .map(|e| {
                        match e {
                            PointerEvent::Enter => "enter",
                            PointerEvent::Exit => "exit",
                            PointerEvent::Down => "down",
                            PointerEvent::Up => "up",
                            PointerEvent::Move => "move",
                            PointerEvent::Click => "click",
                            PointerEvent::Drag => "drag",
                        }
                        .to_owned()
                    })
                    .collect();
            on.extend(l.events.iter().map(|e| format!("event {e}")));
            let actions: Vec<String> = l
                .actions
                .iter()
                .map(|a| match a {
                    Action::Input { input, change } => match change {
                        Change::Set(InputValue::Bool(v)) => format!("{path}{input} = {v}"),
                        Change::Set(InputValue::Number(v)) => format!("{path}{input} = {v}"),
                        Change::Set(InputValue::Fire) => format!("fires {path}{input}"),
                        Change::Toggle => format!("toggles {path}{input}"),
                    },
                    Action::Fire { event } => format!("fires event {event}"),
                    Action::Value { property, .. } => format!("sets view model {property}"),
                    Action::Align { node, keep_offset } => format!(
                        "moves node {node} to the pointer{}",
                        if *keep_offset {
                            ", keeping its offset"
                        } else {
                            ""
                        }
                    ),
                })
                .collect();
            let at = l
                .targets
                .iter()
                .filter_map(|id| scene.hit_bounds(id, 0.0))
                .reduce(|a, b| a.union(b))
                .map(|b| {
                    format!(
                        " ({:.0},{:.0} to {:.0},{:.0})",
                        b.x,
                        b.y,
                        b.x + b.width,
                        b.y + b.height
                    )
                })
                .unwrap_or_default();
            out.push_str(&format!(
                "  listener     {} on {}{at}: {}\n",
                on.join(", "),
                match l.targets.as_slice() {
                    [] => format!("{path}node {}", l.node),
                    [one] => format!("shape {one}"),
                    more => format!("{} shapes", more.len()),
                },
                actions.join(", ")
            ));
        }
    }
    if all.iter().any(|m| !m.inputs.is_empty()) {
        out.push_str(
            "               (render --input '<path/><name>=<value>@<seconds>', '<path/><trigger>@<seconds>')\n",
        );
    }
    if all.iter().any(|m| !m.listeners.is_empty()) {
        out.push_str(
            "               (render --pointer <down|up|move|exit|lift>:<x>,<y>@<seconds>)\n",
        );
    }
    Ok(out)
}

fn parse_color(s: &str) -> Option<Color> {
    match s.to_ascii_lowercase().as_str() {
        "white" => return Some(Color::WHITE),
        "black" => return Some(Color::BLACK),
        "transparent" => return Some(Color::TRANSPARENT),
        _ => {}
    }
    let h = s.strip_prefix('#')?;
    let v = u32::from_str_radix(h, 16).ok()?;
    match h.len() {
        6 => Some(Color::rgba8((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)),
        8 => Some(Color::rgba8(
            (v >> 24) as u8,
            (v >> 16) as u8,
            (v >> 8) as u8,
            v as u8,
        )),
        _ => None,
    }
}
