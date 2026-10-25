//! A repeater copy takes at most 10,000 steps of its transform; one
//! further away stops there, with a warning.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::Import;

/// A shape layer of a square in red, repeated twice with offset `offset`
/// (a property, JSON), each copy 20 to the right of the last.
fn read(offset: &str) -> Import {
    let json = format!(
        r#"{{"v":"5.7.0","fr":10,"ip":0,"op":20,"w":200,"h":100,"layers":[
{{"ty":4,"ind":1,"ip":0,"op":20,"st":0,
 "ks":{{"o":{{"a":0,"k":100}},"p":{{"a":0,"k":[0,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}}}},
 "shapes":[
  {{"ty":"rc","p":{{"a":0,"k":[5,5]}},"s":{{"a":0,"k":[10,10]}},"r":{{"a":0,"k":0}}}},
  {{"ty":"fl","c":{{"a":0,"k":[1,0,0,1]}},"o":{{"a":0,"k":100}}}},
  {{"ty":"rp","c":{{"a":0,"k":2}},"o":{offset},"m":1,"tr":{{"ty":"tr","p":{{"a":0,"k":[20,0]}},"a":{{"a":0,"k":[0,0]}},"s":{{"a":0,"k":[100,100]}},"r":{{"a":0,"k":0}},"so":{{"a":0,"k":100}},"eo":{{"a":0,"k":100}}}}}}
 ]}}]}}"#
    );
    dxn_vector_animation_lottie::read(json.as_bytes()).unwrap()
}

const WARNING: &str = "repeater copies more than 10000 steps of the transform away stop at 10000";

#[test]
fn an_offset_past_the_cap_warns() {
    let import = read(r#"{"a":0,"k":20000}"#);
    assert!(
        import.warnings.iter().any(|w| w == WARNING),
        "{:?}",
        import.warnings
    );
}

#[test]
fn a_moving_offset_past_the_cap_warns() {
    let import = read(r#"{"a":1,"k":[{"t":0,"s":[0]},{"t":20,"s":[-20000]}]}"#);
    assert!(
        import.warnings.iter().any(|w| w == WARNING),
        "{:?}",
        import.warnings
    );
}

#[test]
fn an_offset_within_the_cap_reads_without_warnings() {
    let import = read(r#"{"a":0,"k":9998}"#);
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
}
