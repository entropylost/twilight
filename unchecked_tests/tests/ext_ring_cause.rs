// Where does the ring spread (rotational consistency of ext / pt around a light) in the unoccluded field come from?
// Renders unoccluded lights and the two shadow scenes (scored above the wall only) with two extension carriers:
//   dp:    DirectionalPosition (the renderer's): one emission-weighted mean position shared by both face orientations
//   split: SplitPosition (local to this test): a mean position per face orientation
// and prints rel err mean, ring spread mean / worst and the spread of the first rings. If `split` removes a ring
// spread difference between two extension decompositions, that difference is the carrier mixing orientations, not the
// decomposition's weights.
//   cargo test --release -p unchecked_tests --test ext_ring_cause -- --nocapture
//   exact: ExactFaces (local to this test, ring_exact only): every face kept apart, so the extension carrier loses
//          nothing; what is left is the far carrier (Inverse2) and the weights themselves
// SCENES=name,name restricts the scenes.

use std::collections::HashMap;
use std::ops::{Add, Mul};

use glam::{IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::frustum::{Frustum, FrustumType};
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance, ResampleRadiance, ViewDir};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct SplitPosition {
    // (faces seen horizontally, faces seen vertically)
    emission: Vec2,
    offsets: [Vec2; 2],
}
impl Radiance for SplitPosition {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        let mut total = 0.0;
        for (k, e) in [self.emission.x, self.emission.y].into_iter().enumerate() {
            if e == 0.0 {
                continue;
            }
            let v = source.focus() + self.offsets[k] / e - target;
            total += e * v[k].abs() / v.length_squared();
        }
        total
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        unimplemented!()
    }
    fn new_directional(frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        let offset = emitter - frustum.focus();
        if view_dir.is_horizontal() {
            Self { emission: Vec2::X, offsets: [offset, Vec2::ZERO] }
        } else {
            Self { emission: Vec2::Y, offsets: [Vec2::ZERO, offset] }
        }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let delta = target.focus() - source.focus();
        Self {
            emission: self.emission,
            offsets: [self.offsets[0] - delta * self.emission.x, self.offsets[1] - delta * self.emission.y],
        }
    }
}
impl Add for SplitPosition {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self { emission: self.emission + o.emission, offsets: [self.offsets[0] + o.offsets[0], self.offsets[1] + o.offsets[1]] }
    }
}
impl Mul<f32> for SplitPosition {
    type Output = Self;
    fn mul(self, r: f32) -> Self {
        Self { emission: self.emission * r, offsets: self.offsets.map(|o| o * r) }
    }
}

// Up to FACES faces, each with its own position (absolute, so reprojection is the identity) and summed weight.
const FACES: usize = 12;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct ExactFaces {
    // (position, weight, 0 = seen horizontally / 1 = vertically); weight 0 marks a free slot
    faces: [(Vec2, f32, u8); FACES],
}
impl Radiance for ExactFaces {
    fn read(self, _source: Frustum, target: Vec2) -> f32 {
        self.faces
            .iter()
            .filter(|f| f.1 != 0.0)
            .map(|&(pos, e, axis)| {
                let v = pos - target;
                e * v[axis as usize].abs() / v.length_squared()
            })
            .sum()
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        unimplemented!()
    }
    fn new_directional(_frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        let mut faces = [(Vec2::ZERO, 0.0, 0); FACES];
        faces[0] = (emitter, 1.0, view_dir.is_vertical() as u8);
        Self { faces }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl ResampleRadiance<ExactFaces> for ExactFaces {
    fn resample(original: ExactFaces, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl Add for ExactFaces {
    type Output = Self;
    fn add(mut self, o: Self) -> Self {
        for &(pos, e, axis) in o.faces.iter().filter(|f| f.1 != 0.0) {
            let slot = self
                .faces
                .iter_mut()
                .find(|f| f.1 == 0.0 || (f.0 == pos && f.2 == axis))
                .expect("more faces in one extension than ExactFaces holds");
            *slot = (pos, slot.1 + e, axis);
        }
        self
    }
}
impl Mul<f32> for ExactFaces {
    type Output = Self;
    fn mul(mut self, r: f32) -> Self {
        for f in &mut self.faces {
            f.1 *= r;
        }
        self
    }
}

type Image = HashMap<IVec2, f32>;

fn rect(scene: &mut PixelScene, x: std::ops::Range<i32>, y: std::ops::Range<i32>, e: f32) {
    for i in x {
        for j in y.clone() {
            scene.insert(IVec2::new(i, j), e);
        }
    }
}

struct Case {
    name: &'static str,
    scene: PixelScene,
    rings: std::ops::Range<usize>,
    // only probes with y below this are scored
    max_y: i32,
}

fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    let mut add = |name, rings, max_y, build: &dyn Fn(&mut PixelScene)| {
        let mut scene = PixelScene::new();
        build(&mut scene);
        v.push(Case { name, scene, rings, max_y });
    };
    add("pixel-shadow", 4..25, 59, &|s| {
        rect(s, 36..37, 32..33, 5.0);
        rect(s, 0..50, 60..61, 0.0);
    });
    add("block-shadow", 5..25, 59, &|s| {
        rect(s, 36..39, 32..35, 2.5);
        rect(s, 0..50, 60..61, 0.0);
    });
    add("pix-odd", 4..40, 128, &|s| rect(s, 71..72, 53..54, 5.0));
    add("block2", 4..40, 128, &|s| rect(s, 50..52, 77..79, 3.0));
    add("block5", 6..40, 128, &|s| rect(s, 61..66, 59..64, 1.0));
    add("bar", 7..40, 128, &|s| rect(s, 60..67, 70..71, 2.0));
    v
}

fn score(case: &Case, tag: &str, ext: &Image, pt: &Image) {
    let lit = |c: &IVec2| c.y < case.max_y && !case.scene.contains_key(c);
    let errors: Vec<f32> =
        pt.iter().filter(|&(c, &p)| p > 1e-4 && lit(c)).map(|(c, &p)| (ext[c] - p).abs() / p).collect();
    let rel = errors.iter().sum::<f32>() / errors.len() as f32;

    let emitters: Vec<Vec2> = case.scene.iter().filter(|e| *e.1 > 0.0).map(|e| e.0.as_vec2() + 0.5).collect();
    let light = emitters.iter().sum::<Vec2>() / emitters.len() as f32;
    let mut rings = vec![Vec::new(); case.rings.end];
    for (&c, &p) in pt {
        let r = (c.as_vec2() - light).length().round() as usize;
        if case.rings.contains(&r) && p > 1e-4 && lit(&c) {
            rings[r].push(ext[&c] / p);
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
    println!(
        "{:13} {tag:6} rel {rel:.4}  ring {mean:.4}/{worst:.4}  first rings {:?}",
        case.name,
        spreads.iter().take(8).map(|s| (s * 1000.0).round() / 1000.0).collect::<Vec<_>>()
    );
}

#[test]
fn ring_cause() {
    let only = std::env::var("SCENES").ok();
    println!("EXT_MODE = {:?}", std::env::var("EXT_MODE").ok());
    let settings = RenderExtSettings { simplify_occlusion: false };
    for case in cases() {
        if let Some(only) = &only
            && !only.split(',').any(|n| n == case.name)
        {
            continue;
        }
        let pt = render_pt(&case.scene, RenderPtSettings { directions: 4096, seed: 0 });
        score(&case, "dp", &render_ext::<DirectionalPosition, Inverse2>(&case.scene, settings), &pt);
        score(&case, "split", &render_ext::<SplitPosition, Inverse2>(&case.scene, settings), &pt);
    }
}

// The same score with the lossless extension carrier. Needs a few GB of memory.
#[test]
fn ring_exact() {
    let only = std::env::var("SCENES").ok();
    println!("EXT_MODE = {:?}", std::env::var("EXT_MODE").ok());
    let settings = RenderExtSettings { simplify_occlusion: false };
    for case in cases() {
        if let Some(only) = &only
            && !only.split(',').any(|n| n == case.name)
        {
            continue;
        }
        let pt = render_pt(&case.scene, RenderPtSettings { directions: 4096, seed: 0 });
        score(&case, "exact", &render_ext::<ExactFaces, Inverse2>(&case.scene, settings), &pt);
        // lossless far carrier as well: only the weights (and cos / distance per face) are left
        score(&case, "exact2", &render_ext::<ExactFaces, ExactFaces>(&case.scene, settings), &pt);
    }
}

// ext / pt - 1 in parts per thousand around the light of one scene (SCENE, default pixel-shadow), half window R
// (default 8), carrier CARRIER=dp|split (default dp).
#[test]
fn ring_map() {
    let name = std::env::var("SCENE").unwrap_or("pixel-shadow".into());
    let case = cases().into_iter().find(|c| c.name == name).unwrap();
    let r: i32 = std::env::var("R").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let settings = RenderExtSettings { simplify_occlusion: false };
    let ext = if std::env::var("CARRIER").as_deref() == Ok("split") {
        render_ext::<SplitPosition, Inverse2>(&case.scene, settings)
    } else {
        render_ext::<DirectionalPosition, Inverse2>(&case.scene, settings)
    };
    let pt = render_pt(&case.scene, RenderPtSettings { directions: 8192, seed: 0 });
    let (min, max) = case
        .scene
        .iter()
        .filter(|e| *e.1 > 0.0)
        .fold((IVec2::MAX, IVec2::MIN), |(lo, hi), (&p, _)| (lo.min(p), hi.max(p)));
    for y in min.y - r..=max.y + 1 + r {
        let mut line = String::new();
        for x in min.x - r..=max.x + 1 + r {
            let c = IVec2::new(x, y);
            if pt.get(&c).copied().unwrap_or(0.0) > 1e-4 && !case.scene.contains_key(&c) {
                line += &format!("{:5.0}", (ext[&c] / pt[&c] - 1.0) * 1000.0);
            } else {
                line += "    .";
            }
        }
        println!("{line}");
    }
}
