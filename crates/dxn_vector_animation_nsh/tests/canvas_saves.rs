//! A canvas saves only what it keeps: no save restored at once, none right
//! inside another and restored right before it; what it draws is the same.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use dxn_vector_animation::{Matrix, Scene};
use dxn_vector_animation_nsh::plan::{self, Op, Unit};

fn canvases(units: &[Unit], out: &mut Vec<Vec<Op>>) {
    for u in units {
        match u {
            Unit::Canvas(ops) | Unit::Wide { ops, .. } | Unit::Area { ops, .. } => {
                out.push(ops.clone())
            }
            Unit::Layer { units, .. } | Unit::Clip { units, .. } => canvases(units, out),
            Unit::Sampled(_) | Unit::Raster(_) => {}
        }
    }
}

/// The saves of `ops` that keep nothing.
fn idle_saves(ops: &[Op]) -> usize {
    let mut restore = vec![usize::MAX; ops.len()];
    let mut open = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        match op {
            Op::Save => open.push(i),
            Op::Restore => {
                if let Some(s) = open.pop() {
                    restore[s] = i;
                }
            }
            _ => {}
        }
    }
    (0..ops.len())
        .filter(|&i| {
            restore[i] != usize::MAX
                && (restore[i] == i + 1 || (i > 0 && restore[i - 1] == restore[i] + 1))
        })
        .count()
}

fn scene() -> Scene {
    // Groups in groups, some of nothing but another group, some moving.
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
<style>@keyframes m { from { transform: translate(0, 0) } to { transform: translate(30px, 10px) } }
#a { animation: m 2s linear infinite }</style>
<g><g><g id="a"><g><rect x="5" y="5" width="30" height="30" fill="teal"/></g></g></g></g>
<g transform="translate(10 50)"><g><g><circle cx="20" cy="20" r="15" fill="orange" opacity="0.5"/></g></g></g>
<g><g/></g>
</svg>"#;
    dxn_vector_animation_svg::read(svg.as_bytes())
        .unwrap()
        .scene
}

#[test]
fn canvases_keep_no_idle_saves() {
    let scene = scene();
    let p = plan::compile(&scene, Matrix::IDENTITY).unwrap();
    let mut list = Vec::new();
    canvases(&p.units, &mut list);
    for ops in &list {
        assert_eq!(idle_saves(ops), 0, "{ops:?}");
    }
    let frame = plan::from_frame(&scene.frame_at(1.0), Matrix::IDENTITY);
    let mut list = Vec::new();
    canvases(&frame.units, &mut list);
    assert_eq!(idle_saves(&list[0]), 0);
}

#[test]
fn canvases_without_idle_saves_draw_as_the_reference() {
    // At whole pixels of its motion: a layer moved a fraction of one is
    // resampled.
    common::assert_plan_matches(&scene(), &[0.0, 0.2, 1.0, 1.6], 100, 100);
}
