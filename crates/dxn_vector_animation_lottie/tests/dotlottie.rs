//! dotLottie archives: the first animation of the ZIP, its images
//! embedded, within the limits.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Error, Limits, Node};

/// A ZIP of `files` (name, bytes, deflated or stored).
fn zip(files: &[(&str, &[u8], bool)]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }
    let (mut out, mut central) = (Vec::new(), Vec::new());
    for (name, data, deflate) in files {
        let body = if *deflate {
            miniz_oxide::deflate::compress_to_vec(data, 6)
        } else {
            data.to_vec()
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let offset = out.len() as u32;
        let header = |sig: &[u8], central: bool| {
            let mut h = sig.to_vec();
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend(0u16.to_le_bytes());
            h.extend(method.to_le_bytes());
            h.extend([0u8; 4]);
            h.extend(crc32(data).to_le_bytes());
            h.extend((body.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central {
                // Comment length, disk, internal and external attributes.
                h.extend([0u8; 10]);
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(b"PK\x03\x04", false));
        out.extend(&body);
        central.extend(header(b"PK\x01\x02", true));
    }
    let at = out.len() as u32;
    out.extend(&central);
    out.extend(b"PK\x05\x06");
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((central.len() as u32).to_le_bytes());
    out.extend(at.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

/// A Lottie with a solid layer, and an image layer of asset `img`.
const ANIMATION: &str = r##"{"v":"5.7.0","fr":30,"ip":0,"op":30,"w":64,"h":64,
"assets":[{"id":"img","w":1,"h":1,"u":"/images/","p":"dot.png","e":0}],
"layers":[{"ty":2,"ind":1,"refId":"img","ip":0,"op":30,"st":0,
"ks":{"o":{"a":0,"k":100},"p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0}}},
{"ty":1,"ind":2,"sc":"#ff0000","sw":64,"sh":64,"ip":0,"op":30,"st":0,
"ks":{"o":{"a":0,"k":100},"p":{"a":0,"k":[0,0]},"a":{"a":0,"k":[0,0]},"s":{"a":0,"k":[100,100]},"r":{"a":0,"k":0}}}]}"##;

/// A 1×1 PNG.
const DOT: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8, 0xCF, 0xC0, 0xF0,
    0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn images(scene: &dxn_vector_animation::Scene) -> usize {
    fn walk(g: &dxn_vector_animation::Group) -> usize {
        g.children
            .iter()
            .map(|n| match n {
                Node::Image(_) => 1,
                Node::Group(g) => walk(g),
                Node::Shape(_) | Node::Text(_) => 0,
            })
            .sum()
    }
    walk(&scene.root)
}

#[test]
fn a_version_one_archive_with_an_image() {
    let manifest = br#"{"version":"1.0","animations":[{"id":"hello"}]}"#;
    let data = zip(&[
        ("manifest.json", manifest, false),
        ("animations/hello.json", ANIMATION.as_bytes(), true),
        ("images/dot.png", DOT, false),
    ]);
    assert!(dxn_vector_animation_lottie::sniff(&data));
    let import = dxn_vector_animation_lottie::read(&data).unwrap();
    assert!(import.warnings.is_empty(), "{:?}", import.warnings);
    assert_eq!((import.scene.width, import.scene.height), (64.0, 64.0));
    assert_eq!(images(&import.scene), 1);
}

#[test]
fn a_version_two_archive_reads_its_first_animation() {
    let manifest = br#"{"version":"2","animations":[{"id":"a"},{"id":"b"}]}"#;
    let v2 = ANIMATION.replace("/images/", "/i/");
    let data = zip(&[
        ("manifest.json", manifest, true),
        ("animations/a.json", v2.as_bytes(), true),
        ("animations/b.json", b"{}", true),
        ("i/dot.png", DOT, true),
    ]);
    let import = dxn_vector_animation_lottie::read(&data).unwrap();
    assert_eq!(images(&import.scene), 1);
    assert!(
        import.warnings.iter().any(|w| w.contains("2 animations")),
        "{:?}",
        import.warnings
    );
}

#[test]
fn an_archive_unpacking_past_the_limit_is_refused() {
    let big = vec![b' '; 2_000_000];
    let data = zip(&[("animations/x.json", &big, true)]);
    assert!(data.len() < 100_000, "it packs small");
    let limits = Limits {
        max_bytes: 1_000_000,
        ..Limits::default()
    };
    let err = dxn_vector_animation_lottie::read_with(&data, &limits).unwrap_err();
    assert!(
        matches!(err, Error::Limit { .. } | Error::Invalid(_)),
        "{err:?}"
    );
}

#[test]
fn a_broken_archive_is_an_error() {
    let mut data = zip(&[("animations/x.json", ANIMATION.as_bytes(), true)]);
    data.truncate(data.len() - 10);
    assert!(dxn_vector_animation_lottie::read(&data).is_err());
}
