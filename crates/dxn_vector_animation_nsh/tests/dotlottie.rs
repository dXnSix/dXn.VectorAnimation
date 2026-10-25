//! The player reads dotLottie archives as it reads Lottie JSON.

#![allow(clippy::unwrap_used, clippy::expect_used)]

/// A ZIP of `files` (name, bytes), stored.
fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
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
    for (name, data) in files {
        let offset = out.len() as u32;
        let header = |sig: &[u8], central: bool| {
            let mut h = sig.to_vec();
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes());
            h.extend([0u8; 2]);
            h.extend(0u16.to_le_bytes());
            h.extend([0u8; 4]);
            h.extend(crc32(data).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central {
                h.extend([0u8; 10]);
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(b"PK\x03\x04", false));
        out.extend(*data);
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

#[test]
fn a_dotlottie_archive_loads() {
    let lottie = include_bytes!("../../../fixtures/lottie-motion.json");
    let data = zip(&[
        (
            "manifest.json",
            br#"{"version":"1.0","animations":[{"id":"a"}]}"#,
        ),
        ("animations/a.json", lottie),
    ]);
    let loaded = dxn_vector_animation_nsh::source::read(&data).unwrap();
    let direct = dxn_vector_animation_nsh::source::read(lottie).unwrap();
    assert_eq!(loaded.scene, direct.scene);
}
