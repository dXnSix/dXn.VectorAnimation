//! `inspect` names where a timeline's loop starts over, when not at 0.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn inspect(name: &str, svg: &str) -> String {
    let dir = std::env::temp_dir().join(format!(
        "vector-animation-cli-loop-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.svg"));
    std::fs::write(&file, svg).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_vector-animation"))
        .arg("inspect")
        .arg(&file)
        .output()
        .unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn a_staggered_loader_loops_from_its_last_delay() {
    let text = inspect(
        "staggered",
        r#"<svg width="20" height="10" xmlns="http://www.w3.org/2000/svg"><style>#a { animation: k 1s linear infinite } #b { animation: k 1s linear 0.4s infinite } @keyframes k { to { opacity: 0 } }</style><g id="a"><rect width="5" height="5"/></g><g id="b"><rect x="10" width="5" height="5"/></g></svg>"#,
    );
    assert!(text.contains("1.400 s, loops from 0.400 s"), "{text}");
}

#[test]
fn a_whole_loop_says_loops() {
    let text = inspect(
        "whole",
        r#"<svg width="10" height="10" xmlns="http://www.w3.org/2000/svg"><style>#a { animation: k 1s linear infinite } @keyframes k { to { opacity: 0 } }</style><g id="a"><rect width="5" height="5"/></g></svg>"#,
    );
    assert!(text.contains("1.000 s, loops\n"), "{text}");
}
