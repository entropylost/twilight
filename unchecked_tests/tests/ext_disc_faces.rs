// Circle light (issue 2): is the error in the per-face weights (visibility) or in the lossy carriers?
//
// `Faces` is a lossless carrier: a handle to a list of (face, weight) kept in a thread-local arena, so every emitting
// face keeps its own position through extensions and merges. With it the only things left are the weight
// `get_extension` + the merge give each face and the kernel cos / distance at the face centre.
//
//   disc_carriers: scores circle-light (and the two shadow scenes above the wall) like ext_metrics for the carrier
//                  pairs dp/inv2 (renderer), faces/inv2, faces/faces, dp/dp. Also prints the mean ext / pt.
//   disc_faces:    rotation 0 only. For some probes left of the disc, per size-1 cone: the weight every face gets
//                  (faces/faces) against the fraction of the face that is really visible from the probe inside the
//                  cone's wedge (ray cast in the test only), and the totals.
//   cargo test --release -p unchecked_tests --test ext_disc_faces -- --nocapture --test-threads 1

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
use std::ops::{Add, Mul};

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::{Extension, Fluence, PixelScene};
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{Constant, DirectionalPosition, Inverse2, Radiance, ResampleRadiance, ViewDir};
use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

#[derive(Default)]
pub struct FaceTable {
    ids: HashMap<(i32, i32, u8), u32>,
    // position, 0 = seen horizontally / 1 = vertically
    list: Vec<(Vec2, u8)>,
}
thread_local! {
    static ARENA: RefCell<Vec<Vec<(u32, f32)>>> = RefCell::new(vec![Vec::new()]);
    static FACES: RefCell<FaceTable> = RefCell::new(FaceTable::default());
}
fn reset_faces() {
    ARENA.with(|a| *a.borrow_mut() = vec![Vec::new()]);
    FACES.with(|f| *f.borrow_mut() = FaceTable::default());
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Faces(u32);
impl Faces {
    fn list(self) -> Vec<(Vec2, u8, f32)> {
        ARENA.with(|a| {
            FACES.with(|f| {
                let f = f.borrow();
                a.borrow()[self.0 as usize].iter().map(|&(id, w)| (f.list[id as usize].0, f.list[id as usize].1, w)).collect()
            })
        })
    }
    fn push(list: Vec<(u32, f32)>) -> Self {
        if list.is_empty() {
            return Faces(0);
        }
        ARENA.with(|a| {
            let mut a = a.borrow_mut();
            a.push(list);
            Faces(a.len() as u32 - 1)
        })
    }
}
impl Radiance for Faces {
    fn read(self, _source: Frustum, target: Vec2) -> f32 {
        self.list()
            .into_iter()
            .map(|(pos, axis, w)| {
                let v = pos - target;
                w * v[axis as usize].abs() / v.length_squared()
            })
            .sum()
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        unimplemented!()
    }
    fn new_directional(_frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        let axis = view_dir.is_vertical() as u8;
        let key = ((emitter.x * 2.0).round() as i32, (emitter.y * 2.0).round() as i32, axis);
        let id = FACES.with(|f| {
            let mut f = f.borrow_mut();
            let next = f.list.len() as u32;
            let id = *f.ids.entry(key).or_insert(next);
            if id == next {
                f.list.push((emitter, axis));
            }
            id
        });
        Self::push(vec![(id, 1.0)])
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl ResampleRadiance<Faces> for Faces {
    fn resample(original: Faces, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl Add for Faces {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        if self.0 == 0 {
            return o;
        }
        if o.0 == 0 {
            return self;
        }
        let merged = ARENA.with(|a| {
            let a = a.borrow();
            let (x, y) = (&a[self.0 as usize], &a[o.0 as usize]);
            let mut out = Vec::with_capacity(x.len() + y.len());
            let (mut i, mut j) = (0, 0);
            while i < x.len() || j < y.len() {
                if j == y.len() || (i < x.len() && x[i].0 < y[j].0) {
                    out.push(x[i]);
                    i += 1;
                } else if i == x.len() || y[j].0 < x[i].0 {
                    out.push(y[j]);
                    j += 1;
                } else {
                    out.push((x[i].0, x[i].1 + y[j].1));
                    i += 1;
                    j += 1;
                }
            }
            out
        });
        Self::push(merged)
    }
}
impl Mul<f32> for Faces {
    type Output = Self;
    fn mul(self, r: f32) -> Self {
        if self.0 == 0 || r == 0.0 {
            return Faces(0);
        }
        if r == 1.0 {
            return self;
        }
        let scaled = ARENA.with(|a| a.borrow()[self.0 as usize].iter().map(|&(id, w)| (id, w * r)).collect());
        Self::push(scaled)
    }
}

type ExtCache<L> = HashMap<Extension, Fluence<L>>;
type Image = HashMap<IVec2, f32>;

// copy of the private `probe_pixel` in src/render/ext.rs
fn probe_pixel(cone: ConeFrustum, scene: &PixelScene) -> Fluence<Constant> {
    let pixel = cone.focus + IVec2::new(0, cone.index);
    if let Some(&value) = scene.get(&pixel) {
        return Fluence::new(0.0, Constant { value: value * 0.5 * FRAC_PI_4 });
    }
    // the next pixel's face is emitted by the cone's first extension (emission by arrival)
    Fluence::new(1.0, Constant { value: 0.0 })
}

fn rotations(scene: &PixelScene) -> [(Affine2, PixelScene); 4] {
    let rot = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    [Affine2::IDENTITY, rot, rot * rot, rot * rot * rot].map(|t| {
        (t, scene.iter().map(|(k, v)| (t.transform_point2(k.as_vec2() + 0.5).floor().as_ivec2(), *v)).collect())
    })
}

// `render_ext` with any carriers, one (rotation, cone) at a time
fn render<L: Radiance, I: Radiance + ResampleRadiance<L>>(scene: &PixelScene) -> Image {
    let settings = RenderExtSettings { simplify_occlusion: false };
    let mut image = Image::new();
    for (t, scene) in &rotations(scene) {
        for i in [-1, 0] {
            reset_faces();
            let mut ext_cache = ExtCache::<L>::new();
            let mut cache = HashMap::<Frustum, I>::new();
            for x in 1..128 {
                for y in 1..128 {
                    let cell = IVec2::new(x, y);
                    let cone =
                        ConeFrustum { focus: t.transform_point2(cell.as_vec2()).round().as_ivec2(), size: 1, index: i };
                    let frustum = Frustum::Cone(cone);
                    let near = probe_pixel(cone, scene);
                    let value = if near.transmittance == 0.0 {
                        near.radiance.value
                    } else {
                        let far = get_radiance_recursive_ext(&mut ext_cache, &mut cache, frustum, scene, settings);
                        near.over(Fluence::new(1.0, Constant { value: far.at_focus(frustum) })).radiance.value
                    };
                    *image.entry(cell).or_default() += value;
                }
            }
        }
    }
    reset_faces();
    image
}

fn score(tag: &str, scene: &PixelScene, ext: &Image, pt: &Image, light: Vec2, rings: std::ops::Range<usize>, wall: bool) {
    let mut errors = Vec::new();
    let mut ratio = (0.0, 0);
    let mut values = vec![Vec::new(); rings.end];
    for (&cell, &p) in pt {
        if p > 1e-4 && !scene.contains_key(&cell) {
            errors.push((ext[&cell] - p).abs() / p);
        }
        if wall && cell.y >= 59 {
            continue;
        }
        let r = (cell.as_vec2() - light).length().round() as usize;
        if rings.contains(&r) && p > 1e-4 {
            values[r].push(ext[&cell] / p);
            ratio = (ratio.0 + ext[&cell] / p, ratio.1 + 1);
        }
    }
    errors.sort_by(f32::total_cmp);
    let spreads: Vec<f32> = values[rings.start..]
        .iter()
        .filter(|ring| !ring.is_empty())
        .map(|ring| {
            let mean = ring.iter().sum::<f32>() / ring.len() as f32;
            let var = ring.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / ring.len() as f32;
            var.sqrt() / mean
        })
        .collect();
    println!(
        "  {tag:12} rel err mean {:.4} p99 {:.4} | ring spread mean {:.4} worst {:.4} | mean ext/pt on rings {:.4}",
        errors.iter().sum::<f32>() / errors.len() as f32,
        errors[((errors.len() - 1) as f32 * 0.99).round() as usize],
        spreads.iter().sum::<f32>() / spreads.len() as f32,
        spreads.iter().copied().fold(0.0, f32::max),
        ratio.0 / ratio.1 as f32,
    );
}

fn circle_light() -> PixelScene {
    let mut scene = PixelScene::new();
    for i in -10..=10 {
        for j in -10..=10 {
            if IVec2::new(i, j).length_squared() < 95 {
                scene.insert(IVec2::new(64 + i, 64 + j), 0.3);
            }
        }
    }
    scene
}
fn block(size: i32, emittance: f32) -> PixelScene {
    let mut scene = PixelScene::new();
    for i in 0..size {
        for j in 0..size {
            scene.insert(IVec2::new(36 + i, 32 + j), emittance);
        }
    }
    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    scene
}

#[test]
fn disc_carriers() {
    let only = std::env::var("SCENES").ok();
    let cases = [
        ("circle-light", circle_light(), Vec2::new(64.5, 64.5), 13..60, false),
        ("block-shadow", block(3, 2.5), Vec2::new(37.5, 33.5), 5..25, true),
        ("pixel-shadow", block(1, 5.0), Vec2::new(36.5, 32.5), 4..25, true),
    ];
    for (name, scene, light, rings, wall) in cases {
        if let Some(only) = &only
            && !only.split(',').any(|n| n == name)
        {
            continue;
        }
        println!("== {name} ==");
        let pt = render_pt(&scene, RenderPtSettings { directions: 4096, seed: 0 });
        score("dp/inv2", &scene, &render::<DirectionalPosition, Inverse2>(&scene), &pt, light, rings.clone(), wall);
        score("faces/inv2", &scene, &render::<Faces, Inverse2>(&scene), &pt, light, rings.clone(), wall);
        score("faces/faces", &scene, &render::<Faces, Faces>(&scene), &pt, light, rings.clone(), wall);
        score("dp/dp", &scene, &render::<DirectionalPosition, DirectionalPosition>(&scene), &pt, light, rings.clone(), wall);
    }
}

// The angle (radians) under which the part of the segment a..b that is visible from `probe` and lies in the wedge of
// slopes [lo, hi] about +x is seen; (visible, total within the wedge ignoring occlusion, total).
fn visible_angle(scene: &PixelScene, probe: Vec2, a: Vec2, b: Vec2, lo: f32, hi: f32) -> (f32, f32, f32) {
    const N: usize = 256;
    let (mut visible, mut wedge, mut total) = (0.0, 0.0, 0.0);
    for k in 0..N {
        let p0 = a.lerp(b, k as f32 / N as f32);
        let p1 = a.lerp(b, (k + 1) as f32 / N as f32);
        let mid = (p0 + p1) * 0.5;
        let angle = ((p0 - probe).perp_dot(p1 - probe) / ((p0 - probe).length() * (p1 - probe).length())).abs().asin();
        total += angle;
        let d = mid - probe;
        if d.x <= 0.0 || d.y < lo * d.x || d.y > hi * d.x {
            continue;
        }
        wedge += angle;
        // blocked if the segment probe..mid passes through the inside of a solid pixel
        let blocked = scene.keys().any(|q| {
            let (min, max) = (q.as_vec2() + 1e-3, q.as_vec2() + 1.0 - 1e-3);
            let inv = d.recip();
            let (t0, t1) = ((min - probe) * inv, (max - probe) * inv);
            let near = t0.min(t1).max_element().max(0.0);
            let far = t0.max(t1).min_element().min(1.0);
            near < far
        });
        if !blocked {
            visible += angle;
        }
    }
    (visible, wedge, total)
}

#[test]
fn disc_faces() {
    let scene = circle_light();
    let settings = RenderExtSettings { simplify_occlusion: false };
    let verbose = std::env::var("VERBOSE").is_ok();
    // offsets of the probe to the left of / below the disc centre pixel corner (64, 64)
    let probes = [(16, 0), (24, 0), (44, 0), (60, 0), (15, 6), (37, 15), (55, 23), (14, 10), (40, 30), (50, 45), (20, -8)];
    for i in [-1, 0] {
        reset_faces();
        let mut ext_cache = ExtCache::<Faces>::new();
        let mut cache = HashMap::<Frustum, Faces>::new();
        let (lo, hi) = (i as f32, i as f32 + 1.0);
        for (dx, dy) in probes {
            let focus = IVec2::new(64 - dx, 64 - dy);
            let probe = focus.as_vec2();
            let frustum = Frustum::Cone(ConeFrustum { focus, size: 1, index: i });
            let far = get_radiance_recursive_ext(&mut ext_cache, &mut cache, frustum, &scene, settings);
            let weights: HashMap<(i32, i32, u8), f32> = far
                .list()
                .into_iter()
                .map(|(pos, axis, w)| (((pos.x * 2.0).round() as i32, (pos.y * 2.0).round() as i32, axis), w / 0.15))
                .collect();
            // every face this cone could see: Right faces, and Up (i = 0) or Down (i = -1) faces
            let mut faces = Vec::new();
            for &q in scene.keys() {
                if !scene.contains_key(&(q - IVec2::X)) {
                    faces.push((q.as_vec2(), q.as_vec2() + Vec2::Y, 0u8));
                }
                if i == 0 && !scene.contains_key(&(q - IVec2::Y)) {
                    faces.push((q.as_vec2(), q.as_vec2() + Vec2::X, 1));
                }
                if i == -1 && !scene.contains_key(&(q + IVec2::Y)) {
                    faces.push((q.as_vec2() + Vec2::Y, q.as_vec2() + 1.0, 1));
                }
            }
            faces.sort_by(|a, b| (a.2, a.0.y, a.0.x).partial_cmp(&(b.2, b.0.y, b.0.x)).unwrap());
            // totals: ext with kernel at the centre, ext weights with the exact face angle, truth, and truth without
            // occlusion
            let (mut ext_kernel, mut ext_angle, mut truth, mut unoccluded) = (0.0, 0.0, 0.0, 0.0);
            let mut rows = Vec::new();
            let mut seen = 0;
            for (a, b, axis) in faces {
                let centre = (a + b) * 0.5;
                let key = ((centre.x * 2.0).round() as i32, (centre.y * 2.0).round() as i32, axis);
                let w = weights.get(&key).copied().unwrap_or(0.0);
                seen += weights.contains_key(&key) as usize;
                let v = centre - probe;
                let kernel = v[axis as usize].abs() / v.length_squared();
                let (visible, wedge, total) = visible_angle(&scene, probe, a, b, lo, hi);
                ext_kernel += w * kernel;
                ext_angle += w * total;
                truth += visible;
                unoccluded += wedge;
                if (w - visible / total).abs() > 0.02 {
                    rows.push(format!(
                        "      face {:?} {} rel ({:+.1},{:+.1})  ext weight {:.3}  visible {:.3}  in wedge {:.3}  kernel/angle {:.3}",
                        centre,
                        if axis == 0 { "R" } else { "V" },
                        centre.x - 64.5,
                        centre.y - 64.5,
                        w,
                        visible / total,
                        wedge / total,
                        kernel / total
                    ));
                }
            }
            assert_eq!(seen, weights.len(), "ext saw a face the test does not list");
            println!(
                "cone {i:2} probe -({dx},{dy}): ext {:.5} (exact angles {:.5})  truth {:.5}  unoccluded {:.5}  ext/truth {:.4}  unoccluded/truth {:.4}",
                ext_kernel,
                ext_angle,
                truth,
                unoccluded,
                ext_kernel / truth,
                unoccluded / truth
            );
            if verbose {
                for row in rows {
                    println!("{row}");
                }
            }
        }
    }
}

// Which faces carry the light a probe in the reference's shadow gets ("leak worst" of ext_metrics)? Prints, for the
// probes in PROBES (default: the two worst-leak probes of the shadow scenes), every (rotation, cone) that sees
// anything: the face (in the rotated frame), its weight (1 = whole face) and its contribution.
#[test]
fn leak_faces() {
    let settings = RenderExtSettings { simplify_occlusion: false };
    for (name, scene, probe, emittance) in
        [("pixel-shadow", block(1, 5.0), IVec2::new(56, 73), 5.0), ("block-shadow", block(3, 2.5), IVec2::new(51, 63), 2.5)]
    {
        println!("== {name} probe {probe} ==");
        let mut total = 0.0;
        for (k, (t, scene)) in rotations(&scene).iter().enumerate() {
            for i in [-1, 0] {
                reset_faces();
                let mut ext_cache = ExtCache::<Faces>::new();
                let mut cache = HashMap::<Frustum, Faces>::new();
                let focus = t.transform_point2(probe.as_vec2()).round().as_ivec2();
                let frustum = Frustum::Cone(ConeFrustum { focus, size: 1, index: i });
                let far = get_radiance_recursive_ext(&mut ext_cache, &mut cache, frustum, scene, settings);
                for (pos, axis, w) in far.list() {
                    let v = pos - focus.as_vec2();
                    let value = w * v[axis as usize].abs() / v.length_squared();
                    total += value;
                    println!(
                        "  rotation {k} cone {i:2}: face at {:?} (rel {:?}) {}  weight {:.3}  contributes {:.5}",
                        pos,
                        v,
                        if axis == 0 { "R" } else { "V" },
                        w / (emittance * 0.5),
                        value
                    );
                }
            }
        }
        println!("  total {total:.5}");
    }
    reset_faces();
}

// Unoccluded weights of the two faces of a single emitting pixel at (64, 64) seen by rotation 0, cone 0, for probes
// at (64 - dx, 64 - dy). Under the baseline and candidate C both must be 1 (0.5 on the diagonal for the baseline).
// Under the flux candidate B1 the side face is stored as a horizontal item at (64.5, 64) whose weight is the flux that
// steps sideways into the emitter, which should be the slope towards the face: it is compared with dy / dx and
// dy / (dx + 1).
#[test]
fn pixel_weights() {
    let mut scene = PixelScene::new();
    scene.insert(IVec2::new(64, 64), 2.0);
    let settings = RenderExtSettings { simplify_occlusion: false };
    reset_faces();
    let mut ext_cache = ExtCache::<Faces>::new();
    let mut cache = HashMap::<Frustum, Faces>::new();
    for dx in [3, 4, 5, 8, 13, 21, 34, 55] {
        let mut line = format!("dx {dx:2}:");
        for dy in (1..=dx).step_by((dx as usize / 6).max(1)) {
            let focus = IVec2::new(64 - dx, 64 - dy);
            let frustum = Frustum::Cone(ConeFrustum { focus, size: 1, index: 0 });
            let far = get_radiance_recursive_ext(&mut ext_cache, &mut cache, frustum, &scene, settings);
            let mut near_face = 0.0;
            let mut side = (0.0, 0);
            for (pos, axis, w) in far.list() {
                if pos == Vec2::new(64.0, 64.5) {
                    near_face = w;
                } else {
                    side = (w, axis);
                }
            }
            line += &format!(
                "  dy {dy:2} R {near_face:.3} side {:.4}{} (dy/dx {:.4}, dy/(dx+1) {:.4})",
                side.0,
                if side.1 == 0 { "h" } else { "v" },
                dy as f32 / dx as f32,
                dy as f32 / (dx + 1) as f32
            );
        }
        println!("{line}");
    }
    reset_faces();
}
