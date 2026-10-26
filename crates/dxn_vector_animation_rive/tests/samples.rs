//! The Rive files of the gallery (`examples/gallery/animations-rive`, when
//! they are there) read to their ends, each drawing something.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

#[test]
fn the_gallery_files_read() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/gallery/animations-rive"
    );
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "riv") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let import = dxn_vector_animation_rive::read(&bytes)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let stats = import.scene.stats();
        assert!(
            stats.shapes + stats.images > 0,
            "{}: draws nothing",
            path.display()
        );
    }
}
