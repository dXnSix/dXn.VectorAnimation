//! `vector-animation`: inspect, validate, compile and render animated SVG,
//! Lottie and compiled scenes.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use dxn_vector_animation::{Color, Error, Import, Limits, LoopMode, Scene, compiled};
use dxn_vector_animation_render::{RenderOptions, render_png};

#[derive(Parser)]
#[command(
    name = "vector-animation",
    version,
    about = "Inspect, validate, compile and render animated SVG and Lottie"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Prints a file's size, timeline, contents and what it leaves out.
    Inspect {
        /// An animated SVG, a Lottie JSON or a compiled scene.
        file: PathBuf,
    },
    /// Reads a file and reports whether it plays; fails on errors.
    Validate {
        /// An animated SVG, a Lottie JSON or a compiled scene.
        file: PathBuf,
        /// Fails on warnings too.
        #[arg(long)]
        strict: bool,
    },
    /// Compiles a file into a scene (`.vector-animation.json`).
    Compile {
        /// An animated SVG or a Lottie JSON.
        file: PathBuf,
        /// Where to write; next to the file by default.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Indented JSON.
        #[arg(long)]
        pretty: bool,
    },
    /// Renders frames as PNG images.
    Render {
        /// An animated SVG, a Lottie JSON or a compiled scene.
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
        /// Pixels wide (the height keeps the aspect unless given).
        #[arg(long)]
        width: Option<u32>,
        /// Pixels high.
        #[arg(long)]
        height: Option<u32>,
        /// Pixels per user unit when no size is given.
        #[arg(long, default_value_t = 1.0)]
        scale: f64,
        /// A background color (`#rrggbb`, `#rrggbbaa`, `white`, `black`).
        #[arg(long)]
        background: Option<String>,
        /// An event that happened, `name=seconds` (repeatable): what the
        /// file plays on it plays from then (`inspect` lists them).
        #[arg(long = "event")]
        events: Vec<String>,
    },
}

/// The file formats read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Svg,
    Lottie,
    Compiled,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Svg => "animated SVG",
            Format::Lottie => "Lottie",
            Format::Compiled => "compiled scene",
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn read(file: &Path) -> Result<(Format, Import), String> {
    let data = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let limits = Limits::default();
    let fail = |e: Error| format!("{}: {e}", file.display());
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
        "{}: neither SVG, Lottie nor a compiled scene",
        file.display()
    ))
}

fn run(command: Command) -> Result<ExitCode, String> {
    match command {
        Command::Inspect { file } => {
            let (format, import) = read(&file)?;
            print!("{}", describe(&file, format, &import));
            Ok(ExitCode::SUCCESS)
        }
        Command::Validate { file, strict } => {
            let (format, import) = read(&file)?;
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
            let (_, import) = read(&file)?;
            for w in &import.warnings {
                eprintln!("warning: {w}");
            }
            let json = compiled::to_json(&import.scene, pretty)
                .map_err(|e| format!("{}: {e}", file.display()))?;
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
            width,
            height,
            scale,
            background,
            events,
        } => {
            let (_, import) = read(&file)?;
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
            let times = match fps {
                Some(fps) if fps > 0.0 => frame_times(&scene, fps),
                Some(_) => return Err("--fps must be positive".into()),
                None if times.is_empty() => vec![0.0],
                None => times,
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

/// Every frame time of the timeline at `fps`.
fn frame_times(scene: &Scene, fps: f64) -> Vec<f64> {
    let count = (scene.timeline.duration * fps).floor() as usize;
    (0..=count.min(100_000)).map(|i| i as f64 / fps).collect()
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
            LoopMode::Once => "once",
            LoopMode::Loop => "loops",
            LoopMode::PingPong => "ping-pong",
        }
    ));
    for m in &t.markers {
        match m.name.strip_prefix(dxn_vector_animation::EVENT_PREFIX) {
            Some(event) => line(format!(
                "  event        {event}: plays for {:.3} s (render --event {event}=<seconds>)",
                m.duration
            )),
            None => line(format!(
                "  marker       {} at {:.3} s for {:.3} s",
                m.name, m.start, m.duration
            )),
        }
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
