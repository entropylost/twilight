// Can the side faces of `get_extension` get a transmittance that belongs to the rays that really see them, without
// giving up the falloff? Scores alternatives to "emission by arrival" for SIDE faces with the numbers of ext_metrics
// (the scenes and `score` are copies from there), without touching src/.
//
// Background: a side face (the face of a solid pixel beside an empty start pixel) is only seen by the rays that leave
// the start pixel sideways, a strip about one ray slope high. Emission by arrival lets every size-1 piece that starts
// in the pixel emit the face (weights sum to exactly 1, ext_start_tiling), so the face gets the mean transmittance of
// the whole pixel. The alternative is flux: only the pieces whose rays leave sideways emit it. A size-1 piece does not
// know the ray slope, so a flux piece can only emit its own angular size, i.e. the horizontal kernel (`ViewDir::Right`)
// at the face. ext_side_flux shows what that adds up to: per pixel, diagonal lines + cones have about weight
// (y + 1) / x (exact for some probe parities, off by 0.012 on average and up to 0.05 for others), and the cones alone
// about 1 / x.
//
// The variants are built on the test side: `SideVariant` wraps DirectionalPosition and, for a vertical face, looks at
// the frustum it is created in (size-1 cone, diagonal line or straight line = the kind of piece) and emits
// `right * horizontal kernel + up * vertical kernel` with per-kind weights. Transmittances are the renderer's.
//   arrival    every kind (0, 1): what src/ does
//   flux-half  diagonal (1, 0), cone (0.5, 0): the share of the piece's rays that leave sideways ("B1")
//   flux       diagonal (1, 0), cone (1, 0): the weight those rays have at the next column
//   flux-fixed flux, and cones also emit -0.5 * vertical kernel: (y + 1) / x * horizontal kernel overshoots the face's
//              angle by the factor 1 + 0.5 / x, and the cones' weight is 1 / x
//   blend-NN   NN% arrival + the rest flux-fixed
//   cargo test --release -p unchecked_tests --test ext_side_variants -- --nocapture
// VARIANTS=flux-fixed,blend-50 restricts the variants.

use std::collections::HashMap;
use std::ops::{Add, Mul};
use std::sync::RwLock;

use glam::{IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::frustum::{Frustum, FrustumType};
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance, ViewDir};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

// (weight of the horizontal kernel, weight of the vertical kernel) of a side face, for [straight line, diagonal line,
// cone] pieces
static WEIGHTS: RwLock<[(f32, f32); 3]> = RwLock::new([(0.0, 1.0); 3]);

#[derive(Debug, Clone, Copy, Default)]
struct SideVariant(DirectionalPosition);
impl Radiance for SideVariant {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        self.0.read(source, target)
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self(DirectionalPosition::new(frustum, emitter))
    }
    fn new_directional(frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        if view_dir.is_horizontal() {
            return Self(DirectionalPosition::new_directional(frustum, emitter, view_dir));
        }
        // side faces are only created by size-1 pieces
        assert_eq!(frustum.size(), 1);
        let kind = match frustum {
            Frustum::Cone(_) => 2,
            Frustum::Linear(line) if line.index != 0.0 => 1,
            Frustum::Linear(_) => 0,
        };
        let (right, up) = WEIGHTS.read().unwrap()[kind];
        Self(
            DirectionalPosition::new_directional(frustum, emitter, ViewDir::Right) * right
                + DirectionalPosition::new_directional(frustum, emitter, view_dir) * up,
        )
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        Self(self.0.reproject(source, target))
    }
}
impl Add for SideVariant {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}
impl Mul<f32> for SideVariant {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self(self.0 * rhs)
    }
}

const PT_DIRECTIONS: u32 = 4096;
const WALL_Y: i32 = 60;

// Copies of the scenes in src/bin/offline.rs.
fn block_shadow() -> PixelScene {
    let mut scene = PixelScene::new();
    let pos = IVec2::new(36, 32);
    for i in 0..3 {
        for j in 0..3 {
            scene.insert(pos + IVec2::new(i, j), 2.5);
        }
    }
    for i in 0..50 {
        scene.insert(IVec2::new(i, WALL_Y), 0.0);
    }
    scene
}
fn pixel_shadow() -> PixelScene {
    let mut scene = PixelScene::new();
    scene.insert(IVec2::new(36, 32), 5.0);
    for i in 0..50 {
        scene.insert(IVec2::new(i, WALL_Y), 0.0);
    }
    scene
}
fn circle_light() -> PixelScene {
    let mut scene = PixelScene::new();
    let pos = IVec2::new(64, 64);
    for i in -10..=10 {
        for j in -10..=10 {
            if IVec2::new(i, j).length_squared() < 95 {
                scene.insert(pos + IVec2::new(i, j), 0.3);
            }
        }
    }
    scene
}

struct Case {
    name: &'static str,
    scene: PixelScene,
    // centre of the light, in probe coordinates
    light: Vec2,
    // rings of whole radius in this range, around the light, are scored for rotational artifacts
    rings: std::ops::Range<usize>,
    shadow: bool,
}

type Image = HashMap<IVec2, f32>;

fn percentile(values: &mut [f32], p: f32) -> f32 {
    values.sort_by(f32::total_cmp);
    values[((values.len() - 1) as f32 * p).round() as usize]
}

fn score(case: &Case, ext: &Image, pt: &Image) {
    println!("== {} ==", case.name);

    let mut errors: Vec<f32> = pt
        .iter()
        .filter(|&(cell, &p)| p > 1e-4 && !case.scene.contains_key(cell))
        .map(|(cell, &p)| (ext[cell] - p).abs() / p)
        .collect();
    let mean = errors.iter().sum::<f32>() / errors.len() as f32;
    let max = errors.iter().copied().fold(0.0, f32::max);
    println!("  rel err: mean {mean:.4}  p99 {:.4}  max {max:.4}", percentile(&mut errors, 0.99));

    let mut rings = vec![Vec::new(); case.rings.end];
    for (&cell, &p) in pt {
        // the shadow scenes are only scored above the wall, where nothing is occluded
        if case.shadow && cell.y >= WALL_Y - 1 {
            continue;
        }
        let r = (cell.as_vec2() - case.light).length().round() as usize;
        if case.rings.contains(&r) && p > 1e-4 {
            rings[r].push(ext[&cell] / p);
        }
    }
    let spreads: Vec<f32> = rings[case.rings.start..]
        .iter()
        .filter(|ring| !ring.is_empty())
        .map(|ring| {
            let mean = ring.iter().sum::<f32>() / ring.len() as f32;
            let var = ring.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / ring.len() as f32;
            var.sqrt() / mean
        })
        .collect();
    let mean = spreads.iter().sum::<f32>() / spreads.len() as f32;
    let worst = spreads.iter().copied().fold(0.0, f32::max);
    println!("  rotation: mean ring spread {mean:.4}  worst {worst:.4}");

    if !case.shadow {
        return;
    }
    let (mut dips, mut leaks, mut band_errors) = (Vec::new(), Vec::new(), Vec::new());
    for y in WALL_Y + 2..128 {
        let row = |image: &Image| (1..128).map(|x| image[&IVec2::new(x, y)]).collect::<Vec<f32>>();
        let (e, p) = (row(ext), row(pt));
        let peak_at = (0..p.len()).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
        let peak = p[peak_at];
        let dip: f32 = (0..peak_at).map(|x| (e[x] - e[x + 1]).max(0.0)).sum();
        dips.push((dip / peak, y));
        let leak = (0..p.len()).filter(|&x| p[x] == 0.0).map(|x| e[x]).fold(0.0, f32::max);
        leaks.push((leak / peak, y));
        let in_band = |x: usize| p[x] > 0.05 * peak && p[x] < 0.95 * peak && x <= peak_at;
        let band: Vec<f32> = (0..p.len())
            .filter(|&x| (x.saturating_sub(4)..=(x + 4).min(p.len() - 1)).any(in_band))
            .map(|x| (e[x] - p[x]).abs() / peak)
            .collect();
        if !band.is_empty() {
            band_errors.push((band.iter().sum::<f32>() / band.len() as f32, y));
        }
    }
    for (name, values) in [("dip", &dips), ("leak", &leaks), ("band err", &band_errors)] {
        let mean = values.iter().map(|v| v.0).sum::<f32>() / values.len() as f32;
        let (worst, y) = values.iter().copied().max_by(|a, b| a.0.total_cmp(&b.0)).unwrap();
        println!("  {name}: mean {mean:.4}  worst {worst:.4} (row {y})");
    }
}

#[test]
fn side_variants() {
    let blend = |a: f32| [(0.0, a), (1.0 - a, a), (1.0 - a, a - 0.5 * (1.0 - a))];
    let variants: Vec<(&str, [(f32, f32); 3])> = vec![
        ("arrival", [(0.0, 1.0); 3]),
        ("flux-half", [(0.0, 0.0), (1.0, 0.0), (0.5, 0.0)]),
        ("flux", [(0.0, 0.0), (1.0, 0.0), (1.0, 0.0)]),
        ("flux-fixed", blend(0.0)),
        ("blend-50", blend(0.5)),
        ("blend-75", blend(0.75)),
    ];
    let only = std::env::var("VARIANTS").ok();
    let cases = [
        Case { name: "block-shadow", scene: block_shadow(), light: Vec2::new(37.5, 33.5), rings: 5..25, shadow: true },
        Case { name: "pixel-shadow", scene: pixel_shadow(), light: Vec2::new(36.5, 32.5), rings: 4..25, shadow: true },
        Case { name: "circle-light", scene: circle_light(), light: Vec2::new(64.5, 64.5), rings: 13..60, shadow: false },
    ];
    let pts: Vec<Image> =
        cases.iter().map(|case| render_pt(&case.scene, RenderPtSettings { directions: PT_DIRECTIONS, seed: 0 })).collect();
    for (name, weights) in variants {
        if only.as_ref().is_some_and(|only| !only.split(',').any(|v| v.trim() == name)) {
            continue;
        }
        *WEIGHTS.write().unwrap() = weights;
        println!("######## {name}: (horizontal, vertical) kernel weights for straight / diagonal / cone {weights:?}");
        for (case, pt) in cases.iter().zip(&pts) {
            let ext = render_ext::<SideVariant, Inverse2>(&case.scene, RenderExtSettings { simplify_occlusion: false });
            score(case, &ext, pt);
        }
    }
}
