//! A text document of more letters × animators than the importer bakes, or
//! whose keyframes do not fit even at two samples, is refused, and the error
//! names the limit it was checked against.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Error, Limits};

/// A text layer of `letters` letters `a`, moved by `animators` animators.
fn read(letters: usize, animators: usize) -> Result<dxn_vector_animation::Import, Error> {
    read_within(letters, animators, STILL, &Limits::default())
}

/// An animator that moves its letters 10 down.
const STILL: &str = r#"{"s":{"s":{"a":0,"k":0},"e":{"a":0,"k":100},"o":{"a":0,"k":0}},"a":{"p":{"a":0,"k":[0,10]}}}"#;

/// An animator whose range moves across the letters.
const MOVING: &str = r#"{"s":{"s":{"a":1,"k":[{"t":0,"s":[0]},{"t":10,"s":[100]}]},"e":{"a":0,"k":100},"o":{"a":0,"k":0}},"a":{"p":{"a":0,"k":[0,10]}}}"#;

/// The same document, read within `limits`.
fn read_within(
    letters: usize,
    animators: usize,
    animator: &str,
    limits: &Limits,
) -> Result<dxn_vector_animation::Import, Error> {
    let glyph = r#"{"ch":"a","size":100,"style":"Regular","w":60,"fFamily":"Test","data":{"shapes":[{"ty":"gr","it":[{"ty":"sh","ks":{"a":0,"k":{"c":true,"i":[[0,0],[0,0],[0,0]],"o":[[0,0],[0,0],[0,0]],"v":[[0,0],[50,0],[0,-50]]}}}]}]}}"#;
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":10,"w":400,"h":400,
"fonts":{{"list":[{{"fName":"Test-Regular","fFamily":"Test","fStyle":"Regular","ascent":70}}]}},
"chars":[{glyph}],
"layers":[{{"ty":5,"ind":1,"ip":0,"op":10,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "t":{{"d":{{"k":[{{"t":0,"s":{{"s":100,"f":"Test-Regular","t":"{}","j":0,"tr":0,"lh":120,"fc":[1,0,0]}}}}]}},
  "p":{{}},"m":{{"g":1,"a":{{"a":0,"k":[0,0]}}}},"a":[{}]}}}}]}}"#,
        "a".repeat(letters),
        vec![animator; animators].join(",")
    );
    dxn_vector_animation_lottie::read_with(json.as_bytes(), limits)
}

#[test]
fn the_error_names_the_limit_it_checks() {
    // 2,000 letters × 1,001 animators: past 2,000,000.
    let error = read(2000, 1001).unwrap_err();
    assert_eq!(
        error,
        Error::Limit {
            what: "text animator samples",
            limit: 2_000_000.0,
        }
    );
    assert_eq!(
        error.to_string(),
        "the file exceeds the limit of 2000000 text animator samples"
    );
}

#[test]
fn a_document_within_it_reads() {
    let import = read(20, 3).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
}

#[test]
fn the_keyframes_error_names_the_text_budget() {
    // Text keyframes get half of `max_keyframes`: 50 of 100, while 40 letters
    // need at least two keyframes each.
    let limits = Limits {
        max_keyframes: 100,
        ..Limits::default()
    };
    let Err(error) = read_within(40, 1, MOVING, &limits) else {
        panic!("read within 100 keyframes");
    };
    assert_eq!(
        error,
        Error::Limit {
            what: "text keyframes",
            limit: 50.0,
        }
    );
    assert_eq!(
        error.to_string(),
        "the file exceeds the limit of 50 text keyframes"
    );
}
