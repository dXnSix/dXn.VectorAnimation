//! `compile -o -` writes the scene to stdout, and the help names every
//! input and value the commands take.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{path::PathBuf, process::Command};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vector-animation"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn help(command: &str) -> String {
    let out = bin().args([command, "--help"]).output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn a_dash_writes_the_scene_to_stdout() {
    let dir = std::env::temp_dir().join(format!(
        "vector-animation-cli-stdout-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = bin()
        .current_dir(&dir)
        .arg("compile")
        .arg(fixture("figma-motion.svg"))
        .args(["-o", "-"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let scene =
        dxn_vector_animation::compiled::from_json(&out.stdout, &Default::default()).unwrap();
    let direct =
        dxn_vector_animation_svg::read(&std::fs::read(fixture("figma-motion.svg")).unwrap())
            .unwrap()
            .scene;
    assert_eq!(scene, direct);
    assert!(!dir.join("-").exists(), "no file named `-`");
}

#[test]
fn compile_help_names_every_input_and_stdout() {
    let text = help("compile");
    assert!(
        text.contains("a Lottie JSON, a dotLottie, a Rive file or a compiled scene"),
        "{text}"
    );
    assert!(text.contains("`-` for stdout"), "{text}");
}

#[test]
fn render_help_names_a_transparent_background() {
    let text = help("render");
    assert!(text.contains("`transparent`"), "{text}");
}
