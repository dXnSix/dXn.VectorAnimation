//! Each canvas restores everything it saves: platforms keep a drawing
//! context's state from one drawing to the next (the web's does), so a
//! clip left saved would cut the next frame.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

fn canvases(units: &[Unit], out: &mut Vec<Vec<Op>>) {
    for u in units {
        match u {
            Unit::Canvas(ops) | Unit::Wide { ops, .. } | Unit::Area { ops, .. } => {
                out.push(ops.clone())
            }
            Unit::Layer { units, .. } => canvases(units, out),
            Unit::Sampled(_) | Unit::Raster(_) => {}
        }
    }
}

fn assert_balanced(scene: &Scene) {
    let p = plan::compile(scene, Matrix::IDENTITY).unwrap();
    let mut list = Vec::new();
    canvases(&p.units, &mut list);
    assert!(list.len() > 1, "the scene splits into canvases");
    for (k, ops) in list.iter().enumerate() {
        let mut depth = 0i64;
        for op in ops {
            match op {
                Op::Save => depth += 1,
                Op::Restore => depth -= 1,
                _ => {}
            }
            assert!(depth >= 0, "canvas {k} restores more than it saves");
        }
        assert_eq!(depth, 0, "canvas {k} leaves {depth} saves open");
    }
}

#[test]
fn canvases_split_inside_groups_restore_their_setup() {
    // The Lottie fixture fades groups over overlapping content: layers
    // that split the canvases inside groups.
    let lottie =
        dxn_vector_animation_lottie::read(include_bytes!("../../../fixtures/lottie-motion.json"))
            .unwrap()
            .scene;
    assert_balanced(&lottie);
}
