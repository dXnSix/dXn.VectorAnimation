//! `render -t` takes scene times as given: past the end of a timeline that
//! repeats it does not wrap them (the player does), and says so.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{path::PathBuf, process::Command};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn render(file: &std::path::Path, times: &[&str]) -> String {
    let dir = std::env::temp_dir().join(format!(
        "vector-animation-past-{}-{}",
        std::process::id(),
        times.join("-")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_vector-animation"));
    cmd.arg("render").arg(file).args(["--width", "20", "-o"]);
    cmd.arg(&dir);
    for t in times {
        cmd.args(["-t", t]);
    }
    let out = cmd.output().unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), times.len());
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8(out.stderr).unwrap()
}

#[test]
fn a_time_past_a_loop_is_rendered_as_given_and_the_players_time_is_named() {
    // 2 s, loops: 2.5 s is 0.5 s into the second round.
    let said = render(&fixture("lottie-motion.json"), &["1.5", "2.5"]);
    assert!(
        said.contains("t=2.500s is past the 2.000 s timeline")
            && said.contains("the player shows 0.500 s there"),
        "{said}"
    );
    assert_eq!(said.matches("warning").count(), 1, "{said}");
}

#[test]
fn times_within_the_timeline_and_timelines_that_hold_say_nothing() {
    let said = render(&fixture("lottie-motion.json"), &["0", "2"]);
    assert!(said.is_empty(), "{said}");
    // One play-through that holds its last frame: the end is the end.
    let dir = std::env::temp_dir().join(format!("vector-animation-once-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("once.svg");
    std::fs::write(
        &file,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20"><rect width="5" height="5"><animate attributeName="x" from="0" to="10" dur="1s" fill="freeze"/></rect></svg>"#,
    )
    .unwrap();
    let said = render(&file, &["0.5", "3"]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(said.is_empty(), "{said}");
}
