//! The `.riv` format: its header, table of contents and objects, and what a
//! reader refuses.

#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

mod common;

use common::{Riv, V, color_at, key, near, scene, ty};
use dxn_vector_animation::{Error, Limits};
use dxn_vector_animation_rive::{Options, artboards, read, read_options, read_with, sniff};

const RED: u32 = 0xFFFF_0000;

#[test]
fn rive_files_are_told_by_their_fingerprint() {
    assert!(sniff(&Riv::new().bytes()));
    assert!(!sniff(b"<svg/>"));
    assert!(!sniff(b"{\"v\":\"5.7\"}"));
}

#[test]
fn other_major_versions_are_refused() {
    let mut bytes = Riv::new().artboard(10.0, 10.0).bytes();
    bytes[4] = 6;
    assert!(matches!(read(&bytes), Err(Error::Unsupported(_))));
}

#[test]
fn properties_the_runtime_does_not_know_are_skipped_by_the_table_of_contents() {
    // Property 4000 is a double the file says it writes.
    let riv = Riv::with_toc(&[(4000, 2)])
        .artboard(20.0, 20.0)
        .obj(
            ty::SHAPE,
            &[
                (key::PARENT_ID, V::U(0)),
                (4000, V::D(7.5)),
                (key::X, V::D(10.0)),
                (key::Y, V::D(10.0)),
            ],
        )
        .obj(
            ty::RECTANGLE,
            &[
                (key::PARENT_ID, V::U(1)),
                (key::PATH_WIDTH, V::D(20.0)),
                (key::PATH_HEIGHT, V::D(20.0)),
            ],
        )
        .fill(1, RED);
    assert!(near(color_at(&scene(&riv), 0.0, 10, 10), [255, 0, 0], 2));
}

#[test]
fn properties_in_neither_the_runtime_nor_the_table_are_malformed() {
    let riv = Riv::new()
        .artboard(20.0, 20.0)
        .obj(ty::NODE, &[(key::PARENT_ID, V::U(0)), (4000, V::D(7.5))]);
    assert!(matches!(read(&riv.bytes()), Err(Error::Invalid(_))));
}

#[test]
fn objects_the_runtime_does_not_know_keep_their_place() {
    // A `ForcedEdge` (editor only, its properties in the table) is object
    // 1: the shape after it is 2, and its rectangle and fill say so. The
    // runtime skips it too: nothing is left out.
    let riv = Riv::with_toc(&[(219, 0), (220, 0)])
        .artboard(20.0, 20.0)
        .obj(ty::FORCED_EDGE, &[(219, V::U(0)), (220, V::U(1))])
        .rect_shape(0, 10.0, 10.0, 20.0, 20.0, RED);
    let import = read(&riv.bytes()).unwrap();
    assert!(near(color_at(&import.scene, 0.0, 10, 10), [255, 0, 0], 2));
    assert!(
        !import.warnings.iter().any(|w| w.contains("types newer")),
        "{:?}",
        import.warnings
    );
}

#[test]
fn objects_newer_than_the_runtime_are_left_out_with_a_warning() {
    // Type 4000 (in no table), its property in the table of contents,
    // after the shape.
    let riv = Riv::with_toc(&[(4001, 0)])
        .artboard(20.0, 20.0)
        .rect_shape(0, 10.0, 10.0, 20.0, 20.0, RED)
        .obj(4000, &[(4001, V::U(3))]);
    let import = read(&riv.bytes()).unwrap();
    assert!(near(color_at(&import.scene, 0.0, 10, 10), [255, 0, 0], 2));
    assert!(
        import
            .warnings
            .iter()
            .any(|w| w.contains("1 object(s) of types newer")),
        "{:?}",
        import.warnings
    );
}

#[test]
fn a_file_that_ends_early_is_refused() {
    let riv = Riv::new()
        .artboard(20.0, 20.0)
        .rect_shape(0, 10.0, 10.0, 20.0, 20.0, RED)
        .bytes();
    assert!(matches!(
        read(&riv[..riv.len() - 3]),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn files_over_the_byte_limit_are_refused() {
    let riv = Riv::new().artboard(20.0, 20.0).bytes();
    let limits = Limits {
        max_bytes: 8,
        ..Limits::default()
    };
    assert!(matches!(
        read_with(&riv, &limits),
        Err(Error::Limit { what: "bytes", .. })
    ));
}

#[test]
fn an_artboard_is_read_by_name() {
    let riv = Riv::new()
        .artboard(20.0, 20.0)
        .obj(
            ty::ARTBOARD,
            &[
                (key::NAME, V::S("Second")),
                (key::WIDTH, V::D(40.0)),
                (key::HEIGHT, V::D(30.0)),
            ],
        )
        .bytes();
    assert_eq!(artboards(&riv).unwrap(), ["Main", "Second"]);
    let second = read_options(
        &riv,
        &Limits::default(),
        &Options {
            artboard: Some("Second".into()),
        },
    )
    .unwrap();
    assert_eq!((second.scene.width, second.scene.height), (40.0, 30.0));
    assert_eq!(read(&riv).unwrap().scene.width, 20.0);
    let missing = Options {
        artboard: Some("Third".into()),
    };
    assert!(read_options(&riv, &Limits::default(), &missing).is_err());
}
