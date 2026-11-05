// Splits one probe's radiance in the offline --ext scene into the near-extension terms it is built from, mirroring
// get_radiance_recursive_ext without its cache, so a leak can be traced to the frustum chain and transmittances that
// let it through. Each term is reprojected to the root on its own, so the terms only approximately sum to the total.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::{
    ConeExtension, Extension, ExtensionType, Fluence, LineExtension, PixelScene, ViewDirection, get_extension,
};
use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance, ResampleRadiance};

type L = DirectionalPosition;
type I = Inverse2;
type Cache = HashMap<Extension, Fluence<L>>;

struct Term {
    path: String,
    ext: Extension,
    transmittance: f32,
    value: I,
}

fn cone(f: Frustum) -> ConeFrustum {
    let Frustum::Cone(c) = f else { unreachable!() };
    c
}

fn walk(cache: &mut Cache, frustum: Frustum, scene: &PixelScene) -> Vec<Term> {
    let c = cone(frustum);
    if frustum.size() > 128 {
        return vec![];
    }
    let view_dir = if c.index >= 0 { ViewDirection::Up } else { ViewDirection::Down };
    let start = c.focus + c.lower_dir();
    let tb = |cache: &mut Cache, f: Frustum| {
        let cf = cone(f);
        get_extension(cache, LineExtension { start, end: cf.focus + cf.lower_dir() }.into(), scene, view_dir)
            .transmittance
    };
    let leaf = |cache: &mut Cache, ext: Extension, w: f32, t: f32, tag: &str| {
        let r = get_extension(cache, ext, scene, view_dir).radiance;
        Term { path: tag.to_string(), ext, transmittance: t, value: I::resample(r, ext.as_frustum(), frustum) * w * t }
    };
    let mut out = vec![];
    let children = |cache: &mut Cache, merged: Vec<(Frustum, f32)>, w: f32, with_t: bool, tag: &str| {
        let mut v = vec![];
        for (n, (f, _)) in merged.into_iter().enumerate() {
            let t = if with_t { tb(cache, f) } else { 1.0 };
            for term in walk(cache, f, scene) {
                v.push(Term {
                    path: format!("{tag}{n}(t{t:.2}) > {}", term.path),
                    ext: term.ext,
                    transmittance: term.transmittance * t,
                    value: term.value.reproject(f, frustum) * t * w,
                });
            }
        }
        v
    };
    if frustum.parity() == 0 {
        out.extend(children(cache, merge_even(frustum), 1.0, true, "E"));
        out.push(leaf(cache, ConeExtension { start, end_bottom: start + c.lower_dir() }.into(), 1.0, 1.0, "Enear"));
    } else {
        out.extend(children(cache, merge_odd(frustum, false, true), 0.5, false, "S"));
        out.push(leaf(
            cache,
            LineExtension { start, end: start + c.lower_dir() + c.upper_dir() }.into(),
            -0.5,
            1.0,
            "Snear(-)",
        ));
        out.extend(children(cache, merge_odd(frustum, true, true), 0.5, true, "L"));
        // calculate_long_near
        let lower_start = start + c.lower_dir();
        let upper_start = start + c.upper_dir();
        out.push(leaf(cache, ConeExtension { start, end_bottom: start + c.lower_dir() }.into(), 0.5, 1.0, "Lnear"));
        let ts = [lower_start, upper_start]
            .map(|end| get_extension(cache, LineExtension { start, end }.into(), scene, view_dir).transmittance);
        let far: [(f32, Extension, &str); 4] = [
            (ts[0], LineExtension { start: lower_start, end: lower_start + c.lower_dir() }.into(), "Lfar-line-lo"),
            (ts[1], ConeExtension { start: upper_start, end_bottom: upper_start + c.lower_dir() }.into(), "Lfar-cone-up"),
            (ts[0], ConeExtension { start: lower_start, end_bottom: lower_start + c.lower_dir() }.into(), "Lfar-cone-lo"),
            (ts[1], LineExtension { start: upper_start, end: upper_start + c.upper_dir() }.into(), "Lfar-line-up"),
        ];
        for (t, ext, tag) in far {
            out.push(leaf(cache, ext, 0.25, t, tag));
        }
    }
    for t in &mut out {
        t.path = format!("[s{} j{} @{},{}] {}", c.size, c.index, c.focus.x, c.focus.y, t.path);
    }
    out
}

fn rotation(k: usize) -> Affine2 {
    let rotate = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    (0..k).fold(Affine2::IDENTITY, |a, _| rotate * a)
}

fn rotated_scene(k: usize, wall: bool) -> PixelScene {
    let mut scene = PixelScene::new();
    for i in 0..8 {
        for j in 0..8 {
            scene.insert(IVec2::new(36 + i, 32 + j), 1.0);
        }
    }
    if wall {
        for i in 0..50 {
            scene.insert(IVec2::new(i, 60), 0.0);
        }
    }
    let t = rotation(k);
    scene.iter().map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v)).collect()
}

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// frame-1 (k = 1) rectangles of the emitter and the wall, as [x0, x1, y0, y1]
const EMITTER: [f32; 4] = [88.0, 96.0, 36.0, 44.0];
const WALL: [f32; 4] = [67.0, 68.0, 0.0, 50.0];

// ray parameter where origin + t * dir enters rect, if it does for t >= 0
fn enter(o: Vec2, d: Vec2, r: [f32; 4]) -> Option<f32> {
    let (mut t0, mut t1) = (0.0f32, f32::INFINITY);
    for (p, d, lo, hi) in [(o.x, d.x, r[0], r[1]), (o.y, d.y, r[2], r[3])] {
        if d == 0.0 {
            if p < lo || p > hi {
                return None;
            }
        } else {
            let (a, b) = ((lo - p) / d, (hi - p) / d);
            t0 = t0.max(a.min(b));
            t1 = t1.min(a.max(b));
        }
    }
    (t0 <= t1).then_some(t0)
}

// Of the rays an extension's frustum stands for that reach the emitter, the fraction the wall doesn't block, traced
// exactly from the frustum's focus (None when no ray reaches the emitter).
fn exact_fraction(ext: Extension) -> Option<f32> {
    let n = 1000;
    let rays: Vec<(Vec2, Vec2)> = match ext.as_frustum() {
        Frustum::Cone(c) => (0..n)
            .map(|a| (c.focus.as_vec2(), Vec2::new(c.size as f32, c.index as f32 + (a as f32 + 0.5) / n as f32)))
            .collect(),
        f => (0..n).map(|a| (f.focus() + Vec2::Y * ((a as f32 + 0.5) / n as f32 - 0.5), f.dir())).collect(),
    };
    let (mut reach, mut lit) = (0, 0);
    for (o, d) in rays {
        if let Some(te) = enter(o, d, EMITTER) {
            reach += 1;
            if enter(o, d, WALL).is_none_or(|tw| tw > te) {
                lit += 1;
            }
        }
    }
    (reach > 0).then(|| lit as f32 / reach as f32)
}

#[test]
fn ext_decompose() {
    // per cell along a row: rendered (binary extensions), with each no-wall term scaled by its exact cone fraction,
    // and no-wall total, for k = 1, i = -1 only
    let y: i32 = env("Y", 126);
    let xs: Vec<i32> = (env("X0", 50)..env("X1", 100)).step_by(2).collect();
    let (scene_w, scene_n) = (rotated_scene(1, true), rotated_scene(1, false));
    let (mut cw, mut cn) = (Cache::new(), Cache::new());
    println!("cell      render  cone-exact  nowall   (ratios: render, cone-exact)");
    for x in xs {
        let focus = rotation(1).transform_point2(IVec2::new(x, y).as_vec2()).round().as_ivec2();
        let root = Frustum::Cone(ConeFrustum { focus, size: 1, index: -1 });
        let render: f32 = walk(&mut cw, root, &scene_w).iter().map(|t| t.value.at_focus(root)).sum();
        let nowall_terms = walk(&mut cn, root, &scene_n);
        let nowall: f32 = nowall_terms.iter().map(|t| t.value.at_focus(root)).sum();
        let cone: f32 = nowall_terms
            .iter()
            .map(|t| t.value.at_focus(root) * exact_fraction(t.ext).unwrap_or(1.0))
            .sum();
        println!(
            "({x:3},{y}) {render:.4} {cone:.4} {nowall:.4}   {:.3} {:.3}",
            render / nowall,
            cone / nowall
        );
    }
}

#[test]
fn ext_attribution() {
    let (k, i, x, y): (usize, i32, i32, i32) = (env("K", 1), env("I", -1), env("X", 64), env("Y", 126));
    let wall = env("WALL", 1) != 0;
    let top = env("TOP", 25);
    let scene = rotated_scene(k, wall);
    let focus = rotation(k).transform_point2(IVec2::new(x, y).as_vec2()).round().as_ivec2();
    let root = Frustum::Cone(ConeFrustum { focus, size: 1, index: i });
    let mut cache = Cache::new();
    let mut terms = walk(&mut cache, root, &scene);
    let total: f32 = terms.iter().map(|t| t.value.at_focus(root)).sum();
    println!("probe {focus:?} (cell {x},{y}) k{k} i{i}: sum of terms {total:.4}, {} terms", terms.len());
    terms.sort_by(|a, b| b.value.at_focus(root).abs().total_cmp(&a.value.at_focus(root).abs()));
    for t in terms.iter().take(top) {
        println!("{:+.4} T={:.2} {:?}\n    {}", t.value.at_focus(root), t.transmittance, t.ext, t.path);
    }
}

// Counts how many times a probe sees each emitting pixel: ideally once (with transmittance as the fraction of the
// pixel the wall leaves visible).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Count {
    value: f32,
}
impl Radiance for Count {
    fn read(self, _source: Frustum, _target: Vec2) -> f32 {
        self.value
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        Self { value: 1.0 }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl ResampleRadiance<Count> for Count {
    fn resample(original: Count, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl std::ops::Add for Count {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value }
    }
}
impl std::ops::Mul<f32> for Count {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs }
    }
}

#[test]
fn ext_pixel_weights() {
    use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext};
    let (x, y): (i32, i32) = (env("X", 88), env("Y", 126));
    let mut grid = [[(0.0f32, 0.0f32, 0.0f32); 8]; 8];
    for (gi, row) in grid.iter_mut().enumerate() {
        for (gj, slot) in row.iter_mut().enumerate() {
            let pixel = IVec2::new(36 + gj as i32, 32 + gi as i32);
            let mut seen = [0.0; 2];
            for (n, wall) in [false, true].into_iter().enumerate() {
                let mut scene = PixelScene::new();
                scene.insert(pixel, 1.0);
                if wall {
                    for i in 0..50 {
                        scene.insert(IVec2::new(i, 60), 0.0);
                    }
                }
                // every direction, as render_ext sums them
                for k in 0..4 {
                    let t = rotation(k);
                    let scene: PixelScene = scene
                        .iter()
                        .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                        .collect();
                    let focus = t.transform_point2(IVec2::new(x, y).as_vec2()).round().as_ivec2();
                    for i in [-1, 0] {
                        let f = Frustum::Cone(ConeFrustum { focus, size: 1, index: i });
                        let r: Count = get_radiance_recursive_ext::<Count, Count>(
                            &mut HashMap::new(),
                            &mut HashMap::new(),
                            f,
                            &scene,
                            RenderExtSettings { simplify_occlusion: false },
                        );
                        seen[n] += r.value;
                    }
                }
            }
            // exact fraction of the pixel visible from the probe, in original coordinates
            let probe = IVec2::new(x, y).as_vec2();
            let m = 16;
            let mut vis = 0;
            for a in 0..m {
                for b in 0..m {
                    let e = pixel.as_vec2() + Vec2::new((a as f32 + 0.5) / m as f32, (b as f32 + 0.5) / m as f32);
                    let blocked = enter(probe, e - probe, [0.0, 50.0, 60.0, 61.0]).is_some_and(|t| t < 1.0);
                    vis += !blocked as i32;
                }
            }
            *slot = (seen[0], seen[1], vis as f32 / (m * m) as f32);
        }
    }
    for (label, pick) in [("no wall", 0), ("wall", 1), ("exact visible", 2)] {
        println!("{label} (rows y 32..40, cols x 36..44):");
        for row in &grid {
            let s: Vec<String> = row.iter().map(|c| format!("{:.2}", [c.0, c.1, c.2][pick])).collect();
            println!("  {}", s.join(" "));
        }
    }
    let total = |pick: usize| grid.iter().flatten().map(|c| [c.0, c.1, c.2][pick]).sum::<f32>();
    println!("sums: no wall {:.2}, wall {:.2}, exact {:.2}", total(0), total(1), total(2));
}

#[test]
fn ext_leak_by_level() {
    // for cells just inside the umbra edge: (wall term - no-wall term * exact cone fraction) summed per extension size,
    // relative to the cell's no-wall total
    let cells: Vec<(i32, i32)> = vec![(52, 70), (55, 86), (57, 98), (60, 110), (63, 122), (64, 126)];
    let (scene_w, scene_n) = (rotated_scene(1, true), rotated_scene(1, false));
    let (mut cw, mut cn) = (Cache::new(), Cache::new());
    for (x, y) in cells {
        let focus = rotation(1).transform_point2(IVec2::new(x, y).as_vec2()).round().as_ivec2();
        let root = Frustum::Cone(ConeFrustum { focus, size: 1, index: -1 });
        let tw = walk(&mut cw, root, &scene_w);
        let tn = walk(&mut cn, root, &scene_n);
        let nowall: f32 = tn.iter().map(|t| t.value.at_focus(root)).sum();
        let mut by_size = std::collections::BTreeMap::<i32, f32>::new();
        // walk visits terms in the same order with or without the wall
        for (a, b) in tw.iter().zip(&tn) {
            assert_eq!(a.ext, b.ext);
            let leak = a.value.at_focus(root) - b.value.at_focus(root) * exact_fraction(b.ext).unwrap_or(1.0);
            *by_size.entry(a.ext.size()).or_default() += leak / nowall;
        }
        let s: Vec<String> = by_size.iter().map(|(k, v)| format!("s{k}:{v:+.3}")).collect();
        println!("cell ({x},{y}) probe' {focus}: {}", s.join(" "));
    }
}
