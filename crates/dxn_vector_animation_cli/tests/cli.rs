//! The `vector-animation` command.

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

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "vector-animation-cli-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn inspect_describes_both_formats() {
    let svg = bin()
        .arg("inspect")
        .arg(fixture("figma-motion.svg"))
        .output()
        .unwrap();
    let text = String::from_utf8(svg.stdout).unwrap();
    assert!(svg.status.success());
    assert!(
        text.contains("animated SVG") && text.contains("2.000 s, loops"),
        "{text}"
    );
    let lottie = bin()
        .arg("inspect")
        .arg(fixture("lottie-motion.json"))
        .output()
        .unwrap();
    let text = String::from_utf8(lottie.stdout).unwrap();
    assert!(
        text.contains("Lottie") && text.contains("marker       half"),
        "{text}"
    );
}

#[test]
fn validate_fails_strictly_on_warnings() {
    let dir = temp_dir("validate");
    let file = dir.join("foreign.svg");
    std::fs::write(
        &file,
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg"><foreignObject/></svg>"#,
    )
    .unwrap();
    assert!(bin().arg("validate").arg(&file).status().unwrap().success());
    assert!(
        !bin()
            .args(["validate", "--strict"])
            .arg(&file)
            .status()
            .unwrap()
            .success()
    );
    let broken = dir.join("broken.json");
    std::fs::write(&broken, "{\"layers\": [], \"fr\": 0}").unwrap();
    assert!(
        !bin()
            .arg("validate")
            .arg(&broken)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn compiled_scenes_read_back_the_same() {
    let dir = temp_dir("compile");
    let out = dir.join("motion.vector-animation.json");
    let status = bin()
        .arg("compile")
        .arg(fixture("lottie-motion.json"))
        .arg("-o")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success());
    let compiled = dxn_vector_animation::compiled::from_json(
        &std::fs::read(&out).unwrap(),
        &Default::default(),
    )
    .unwrap();
    let direct =
        dxn_vector_animation_lottie::read(&std::fs::read(fixture("lottie-motion.json")).unwrap())
            .unwrap()
            .scene;
    assert_eq!(compiled, direct);
}

#[test]
fn render_writes_frames() {
    let dir = temp_dir("render");
    let one = dir.join("one.png");
    let status = bin()
        .arg("render")
        .arg(fixture("figma-motion.svg"))
        .args(["-t", "1", "--width", "100", "-o"])
        .arg(&one)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(std::fs::read(&one).unwrap().starts_with(b"\x89PNG"));
    let frames = dir.join("frames");
    let status = bin()
        .arg("render")
        .arg(fixture("lottie-motion.json"))
        .args(["--fps", "2", "-o"])
        .arg(&frames)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        std::fs::read_dir(&frames).unwrap().count(),
        5,
        "0, 0.5, 1, 1.5 and 2 s"
    );
}
