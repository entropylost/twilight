// Measures the cross through the emitter in the offline `--ext` render: the radiance each probe gets from the single
// emitting pixel, over what an unoccluded pixel subtends, (|dx| + |dy|) / 2 / distance² (both faces at half weight).

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance};
use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext, render_ext};

const EMITTER: IVec2 = IVec2::new(36, 32);

fn expected(cell: IVec2) -> f32 {
    let d = EMITTER.as_vec2() + 0.5 - cell.as_vec2();
    (d.x.abs() + d.y.abs()) * 0.5 / d.length_squared()
}

fn ratio_map(image: &HashMap<IVec2, f32>, range: i32, step: i32) {
    for dy in (-range..=range).step_by(step as usize) {
        let row: Vec<String> = (-range..=range)
            .step_by(step as usize)
            .map(|dx| {
                let cell = EMITTER + IVec2::new(dx, dy);
                if dx.abs().max(dy.abs()) < 2 || !image.contains_key(&cell) {
                    "   . ".to_string()
                } else {
                    format!("{:5.2}", image[&cell] / expected(cell))
                }
            })
            .collect();
        println!("dy={dy:3} {}", row.join(""));
    }
}

#[test]
fn cross_ratio_single() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    for simplify_occlusion in [false, true] {
        println!("simplify_occlusion={simplify_occlusion}");
        let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion });
        ratio_map(&image, 12, 1);
        ratio_map(&image, 30, 3);
    }
}

fn transforms() -> [Affine2; 4] {
    let rotate = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    [Affine2::IDENTITY, rotate, rotate * rotate, rotate * rotate * rotate]
}

// Per root cone (rotation k, index i), what each probe near an axis gets, over the expected total.
#[test]
fn cross_root_cones() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let settings = RenderExtSettings { simplify_occlusion: false };
    for offset in [
        IVec2::new(-8, -1),
        IVec2::new(-8, 0),
        IVec2::new(-8, 1),
        IVec2::new(-8, 2),
        IVec2::new(9, 0),
        IVec2::new(9, 1),
        IVec2::new(0, -8),
        IVec2::new(1, -8),
        IVec2::new(-8, -8),
        IVec2::new(-8, -7),
    ] {
        let cell = EMITTER + offset;
        let mut line = format!("offset {offset:>8}:");
        let mut total = 0.0;
        for (k, t) in transforms().iter().enumerate() {
            let scene = scene
                .iter()
                .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                .collect::<PixelScene>();
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let r = get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(
                    &mut HashMap::new(),
                    &mut HashMap::new(),
                    frustum,
                    &scene,
                    settings,
                )
                .at_focus(frustum)
                    / expected(cell);
                total += r;
                if r.abs() > 1e-4 {
                    line += &format!(" k{k}i{i:2}={r:.3}");
                }
            }
        }
        println!("{line}  total={total:.3}");
    }
}

// The offline scene: the emitter at brightness 5 and the wall along y = 60.
#[test]
fn cross_ratio_offline() {
    let mut scene = PixelScene::from([(EMITTER, 5.0)]);
    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    let image = image.into_iter().map(|(k, v)| (k, v / 5.0)).collect::<HashMap<_, _>>();
    ratio_map(&image, 12, 1);
    ratio_map(&image, 33, 3);
}

// Writes the offline scene's ratio to the unoccluded single pixel as a PNG to $CROSS_RATIO_PNG: grey is 1, each step of
// 0.1 is a quarter of the range. Also prints the raw displayed values of a row and column through the emitter's halo.
#[test]
fn cross_ratio_png() {
    let Ok(path) = std::env::var("CROSS_RATIO_PNG") else {
        return;
    };
    let mut scene = PixelScene::from([(EMITTER, 5.0)]);
    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    let display = |v: f32| (v.powf(1.0 / 2.2).clamp(0.0, 1.0) * 255.0).round() as u8;
    let row: Vec<String> = (10..70).map(|x| display(image[&IVec2::new(x, 12)]).to_string()).collect();
    println!("row y=12, x=10..70: {}", row.join(" "));
    let col: Vec<String> = (10..56).map(|y| display(image[&IVec2::new(16, y)]).to_string()).collect();
    println!("col x=16, y=10..56: {}", col.join(" "));
    let png = image::GrayImage::from_fn(128 * 4, 128 * 4, |x, y| {
        let cell = IVec2::new(x as i32 / 4, y as i32 / 4);
        let ratio = image.get(&cell).map_or(1.0, |v| v / 5.0 / expected(cell));
        image::Luma([((0.5 + (ratio - 1.0) * 2.5).clamp(0.0, 1.0) * 255.0) as u8])
    });
    png.save(path).unwrap();
}

// Counts each face the pixel is seen through, without falloff: x for the faces seen moving horizontally, y vertically.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct FaceCount {
    faces: Vec2,
}
impl Radiance for FaceCount {
    fn read(self, _source: Frustum, _target: Vec2) -> f32 {
        self.faces.element_sum()
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        Self { faces: Vec2::splat(0.5) }
    }
    fn new_directional(_frustum: Frustum, _emitter: Vec2, view_dir: twilight::radiance::ViewDir) -> Self {
        Self { faces: if view_dir.is_horizontal() { Vec2::X } else { Vec2::Y } }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl twilight::radiance::ResampleRadiance<FaceCount> for FaceCount {
    fn resample(original: FaceCount, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl std::ops::Add for FaceCount {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { faces: self.faces + other.faces }
    }
}
impl std::ops::Mul<f32> for FaceCount {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { faces: self.faces * rhs }
    }
}

// Per root cone, the faces of the pixel a probe on a diagonal through it sees. Each face should be seen at 0.5 in total.
#[test]
fn cross_diagonal_faces() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let settings = RenderExtSettings { simplify_occlusion: false };
    for (name, dir) in
        [("up-left", IVec2::new(-1, -1)), ("down-right", IVec2::new(1, 1)), ("up-right", IVec2::new(1, -1))]
    {
        for n in 2..=20 {
            // the pixel's nearest corner to the probe
            let corner = EMITTER + IVec2::new((dir.x > 0) as i32, (dir.y > 0) as i32);
            let cell = corner + dir * n;
            let mut line = format!("{name} n={n:2}:");
            for (k, t) in transforms().iter().enumerate() {
                let scene = scene
                    .iter()
                    .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                    .collect::<PixelScene>();
                let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
                let swap = (t.matrix2.x_axis.x).abs() < 0.5;
                for i in [-1, 0] {
                    let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                    let r = get_radiance_recursive_ext::<FaceCount, FaceCount>(
                        &mut HashMap::new(),
                        &mut HashMap::new(),
                        frustum,
                        &scene,
                        settings,
                    );
                    let faces = if swap { Vec2::new(r.faces.y, r.faces.x) } else { r.faces };
                    if faces != Vec2::ZERO {
                        line += &format!(" k{k}i{i:2} h={:.3} v={:.3}", faces.x, faces.y);
                    }
                }
            }
            println!("{line}");
        }
    }
}

// With the probe fixed, moves a single emitting pixel around it and prints how much of each of its faces all root cones
// together see. Each face the probe can see should be seen at 0.5 (h: seen moving horizontally, v: vertically).
#[test]
fn cross_face_maps() {
    let probe = IVec2::new(64, 64);
    let settings = RenderExtSettings { simplify_occlusion: false };
    let range: i32 = 14;
    let width = (2 * range + 1) as usize;
    let mut maps = vec![vec![[0.0f32; 2]; width]; width];
    for dy in -range..=range {
        for dx in -range..=range {
            if dx.abs().max(dy.abs()) < 2 {
                continue;
            }
            let pixel = probe + IVec2::new(dx, dy);
            let mut faces = Vec2::ZERO;
            for t in transforms() {
                let scene = PixelScene::from([(t.transform_point2(pixel.as_vec2() + 0.5).floor().as_ivec2(), 1.0)]);
                let focus = t.transform_point2(probe.as_vec2()).round().as_ivec2();
                let swap = (t.matrix2.x_axis.x).abs() < 0.5;
                for i in [-1, 0] {
                    let frustum = Frustum::Cone(ConeFrustum { focus, size: 1, index: i });
                    let r = get_radiance_recursive_ext::<FaceCount, FaceCount>(
                        &mut HashMap::new(),
                        &mut HashMap::new(),
                        frustum,
                        &scene,
                        settings,
                    );
                    faces += if swap { Vec2::new(r.faces.y, r.faces.x) } else { r.faces };
                }
            }
            maps[(dy + range) as usize][(dx + range) as usize] = faces.to_array();
        }
    }
    for (f, name) in ["h", "v"].iter().enumerate() {
        println!("{name} faces, pixel offset from probe:");
        for (row, dy) in maps.iter().zip(-range..) {
            let row: String = row.iter().map(|v| format!("{:5.2}", v[f])).collect();
            println!("  dy={dy:3}{row}");
        }
    }
}

// The exact light a root cone gets from the pixel in its frame: half the emittance times the angle of each face facing
// the probe, clipped to the cone's slopes [index, index + 1].
fn exact_cone(probe: IVec2, pixel: IVec2, index: i32) -> f32 {
    let d = (pixel - probe).as_vec2();
    let mut faces = vec![];
    if d.x > 0.0 {
        faces.push((Vec2::new(d.x, d.y), Vec2::new(d.x, d.y + 1.0)));
    }
    if d.y > 0.0 {
        faces.push((Vec2::new(d.x, d.y), Vec2::new(d.x + 1.0, d.y)));
    } else if d.y + 1.0 < 0.0 {
        faces.push((Vec2::new(d.x, d.y + 1.0), Vec2::new(d.x + 1.0, d.y + 1.0)));
    }
    let (lo, hi) = ((index as f32).atan(), ((index + 1) as f32).atan());
    faces
        .into_iter()
        .map(|(a, b)| {
            let (a, b) = (a.y.atan2(a.x), b.y.atan2(b.x));
            let (a, b) = (a.min(b).max(lo), a.max(b).min(hi));
            0.5 * (b - a).max(0.0)
        })
        .sum()
}

// Per root cone, the light from the pixel over the exact light, for probes near the diagonals through it.
#[test]
fn cross_cone_errors() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let settings = RenderExtSettings { simplify_occlusion: false };
    for offset in [
        (-12, -12),
        (-11, -12),
        (-10, -12),
        (-12, -11),
        (-12, -10),
        (-7, -6),
        (-6, -6),
        (-5, -6),
        (-6, -5),
        (-4, -3),
        (-3, -3),
        (-3, -4),
        (10, 11),
        (11, 11),
        (11, 10),
        (12, 11),
        (-11, 13),
        (-10, 12),
    ] {
        let offset = IVec2::from(offset);
        let cell = EMITTER + offset;
        let mut line = format!("offset {offset:>9}:");
        let (mut total, mut total_exact) = (0.0, 0.0);
        for (k, t) in transforms().iter().enumerate() {
            let pixel = t.transform_point2(EMITTER.as_vec2() + 0.5).floor().as_ivec2();
            let scene = scene
                .iter()
                .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                .collect::<PixelScene>();
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let r = get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(
                    &mut HashMap::new(),
                    &mut HashMap::new(),
                    frustum,
                    &scene,
                    settings,
                )
                .at_focus(frustum);
                let exact = exact_cone(probe, pixel, i);
                total += r;
                total_exact += exact;
                if r.abs() > 1e-5 || exact > 1e-5 {
                    line += &format!(" k{k}i{i:2}: {:.4}/{:.4}", r, exact);
                }
            }
        }
        println!("{line}  total {:.3}", total / total_exact);
    }
}

// The same per-cone comparison with the far carrier swapped for DirectionalPosition, which reads a single pixel exactly
// from anywhere: separates the extensions' split of the pixel from Inverse2's reading of it.
#[test]
fn cross_cone_errors_exact_carrier() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let settings = RenderExtSettings { simplify_occlusion: false };
    for offset in [
        (-11, -12),
        (-10, -12),
        (-12, -11),
        (-7, -6),
        (-6, -6),
        (-5, -6),
        (-6, -5),
        (10, 11),
        (11, 11),
        (11, 10),
        (-10, 12),
    ] {
        let offset = IVec2::from(offset);
        let cell = EMITTER + offset;
        let mut line = format!("offset {offset:>9}:");
        let (mut total, mut total_exact) = (0.0, 0.0);
        for (k, t) in transforms().iter().enumerate() {
            let pixel = t.transform_point2(EMITTER.as_vec2() + 0.5).floor().as_ivec2();
            let scene = scene
                .iter()
                .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                .collect::<PixelScene>();
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let r = get_radiance_recursive_ext::<DirectionalPosition, DirectionalPosition>(
                    &mut HashMap::new(),
                    &mut HashMap::new(),
                    frustum,
                    &scene,
                    settings,
                )
                .at_focus(frustum);
                let exact = exact_cone(probe, pixel, i);
                total += r;
                total_exact += exact;
                if r.abs() > 1e-5 || exact > 1e-5 {
                    line += &format!(" k{k}i{i:2}: {:.4}/{:.4}", r, exact);
                }
            }
        }
        println!("{line}  total {:.3}", total / total_exact);
    }
}

// Face coverage per root cone near the diagonals through the offline emitter.
#[test]
fn cross_cone_faces() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let settings = RenderExtSettings { simplify_occlusion: false };
    for offset in
        [(-11, -12), (-10, -12), (-12, -11), (-12, -10), (-7, -6), (-6, -6), (-5, -6), (-6, -5), (-4, -6), (-4, -5)]
    {
        let offset = IVec2::from(offset);
        let cell = EMITTER + offset;
        let mut line = format!("offset {offset:>9}:");
        for (k, t) in transforms().iter().enumerate() {
            let scene = scene
                .iter()
                .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                .collect::<PixelScene>();
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let r = get_radiance_recursive_ext::<FaceCount, FaceCount>(
                    &mut HashMap::new(),
                    &mut HashMap::new(),
                    frustum,
                    &scene,
                    settings,
                );
                if r.faces != Vec2::ZERO {
                    line += &format!(" k{k}i{i:2} probe={probe} faces={:.3?}", r.faces.to_array());
                }
            }
        }
        println!("{line}");
    }
}

// How much of pixel q a root cone covers, from the horizontal face count of an emitter just right of q (only rays that
// cross q see that face, at half weight).
fn coverage(focus: IVec2, index: i32, q: IVec2) -> f32 {
    let scene = PixelScene::from([(q + IVec2::X, 1.0)]);
    let frustum = Frustum::Cone(ConeFrustum { focus, size: 1, index });
    let settings = RenderExtSettings { simplify_occlusion: false };
    let r = get_radiance_recursive_ext::<FaceCount, FaceCount>(
        &mut HashMap::new(),
        &mut HashMap::new(),
        frustum,
        &scene,
        settings,
    );
    r.faces.x * 2.0
}

// The root cones' coverage of the pixels along their sloped edge: the pixel the edge halves (should be 0.5), the one
// inside it (1) and the one outside (0), by focus column (rows) and distance (columns).
#[test]
fn cross_edge_coverage() {
    for index in [0, -1] {
        for (name, rel) in [("halved", IVec2::new(0, 0)), ("inside", IVec2::new(0, -1)), ("outside", IVec2::new(-1, 0))]
        {
            println!("index {index}, {name} pixel, n = 2..24:");
            for fx in 32..40 {
                let focus = IVec2::new(fx, 64);
                let row: String = (2..=24)
                    .map(|n| {
                        // index 0's edge has slope 1 and halves rel (n, n); index -1's has slope -1 and halves (n, -n - 1)
                        let q = if index == 0 {
                            focus + IVec2::new(n, n) + rel
                        } else {
                            focus + IVec2::new(n, -n - 1) + IVec2::new(rel.x, -rel.y)
                        };
                        format!("{:5.2}", coverage(focus, index, q))
                    })
                    .collect();
                println!("  fx={fx}{row}");
            }
        }
    }
}

// Brute force for the offline scene: half the emittance times the angle of each emitter face facing the probe, sampled
// along the face and occluded by the wall's rectangle.
fn ground_truth(cell: IVec2) -> f32 {
    let p = cell.as_vec2();
    let (lo, hi) = (EMITTER.as_vec2(), EMITTER.as_vec2() + 1.0);
    let wall_blocks = |e: Vec2| {
        // the segment from p to e against [0, 50] x [60, 61]
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        let d = e - p;
        for (o, dd, a, b) in [(p.x, d.x, 0.0, 50.0), (p.y, d.y, 60.0, 61.0)] {
            if dd.abs() < 1e-9 {
                if o < a || o > b {
                    return false;
                }
            } else {
                let (u, v) = ((a - o) / dd, (b - o) / dd);
                t0 = t0.max(u.min(v));
                t1 = t1.min(u.max(v));
            }
        }
        t0 < t1
    };
    let mut faces = vec![];
    if p.x < lo.x {
        faces.push((Vec2::new(lo.x, lo.y), Vec2::new(lo.x, hi.y)));
    } else if p.x > hi.x {
        faces.push((Vec2::new(hi.x, lo.y), Vec2::new(hi.x, hi.y)));
    }
    if p.y < lo.y {
        faces.push((Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y)));
    } else if p.y > hi.y {
        faces.push((Vec2::new(lo.x, hi.y), Vec2::new(hi.x, hi.y)));
    }
    const SAMPLES: usize = 256;
    let mut total = 0.0;
    for (a, b) in faces {
        for s in 0..SAMPLES {
            let (e0, e1) = (a.lerp(b, s as f32 / SAMPLES as f32), a.lerp(b, (s + 1) as f32 / SAMPLES as f32));
            if wall_blocks(e0.lerp(e1, 0.5)) {
                continue;
            }
            let (d0, d1) = (e0 - p, e1 - p);
            total += d0.perp_dot(d1).atan2(d0.dot(d1)).abs();
        }
    }
    total * 0.5 * 5.0
}

// Compares the offline render with the brute force: error statistics over the lit cells at least 3 cells from the
// emitter, and the cells whose error is largest.
#[test]
fn offline_vs_ground_truth() {
    let mut scene = PixelScene::from([(EMITTER, 5.0)]);
    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    let mut errors = vec![];
    for (&cell, &value) in &image {
        let d = cell - EMITTER;
        if d.x.abs().max(d.y.abs()) < 3 || (cell.y >= 60 && cell.y <= 61 && cell.x <= 50) {
            continue;
        }
        let truth = ground_truth(cell);
        let scale = truth.max(expected(cell) * 5.0 * 0.05);
        errors.push(((value - truth) / scale, cell, value, truth));
    }
    errors.sort_by(|a, b| b.0.abs().total_cmp(&a.0.abs()));
    let n = errors.len() as f32;
    let mean = errors.iter().map(|e| e.0.abs()).sum::<f32>() / n;
    let over = |t: f32| errors.iter().filter(|e| e.0.abs() > t).count();
    println!("cells {n}, mean |rel err| {mean:.4}, >2%: {}, >5%: {}, >10%: {}", over(0.02), over(0.05), over(0.1));
    for (err, cell, value, truth) in errors.iter().take(15) {
        println!("  {cell}: render {value:.5} truth {truth:.5} rel err {err:+.3}");
    }
}

// The render's error against the brute force on the lit cells, by where the cell lies: on the diagonals or axes
// through the emitter (within a cell), or elsewhere. Fails if the diagonals' cross comes back.
#[test]
fn offline_error_by_region() {
    let mut scene = PixelScene::from([(EMITTER, 5.0)]);
    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    let mut regions: [(&str, Vec<f32>); 3] = [("diagonals", vec![]), ("axes", vec![]), ("elsewhere", vec![])];
    for (&cell, &value) in &image {
        let d = cell.as_vec2() - (EMITTER.as_vec2() + 0.5);
        let truth = ground_truth(cell);
        // lit and fully unoccluded, so penumbrae and the wall's leaks don't count
        if d.x.abs().max(d.y.abs()) < 3.0 || (truth - expected(cell) * 5.0).abs() > 1e-3 * truth {
            continue;
        }
        let err = (value - truth) / truth;
        let region = if (d.x.abs() - d.y.abs()).abs() <= 1.0 {
            0
        } else if d.x.abs().min(d.y.abs()) <= 1.0 {
            1
        } else {
            2
        };
        if region == 2 && err.abs() > 0.03 {
            println!("  elsewhere {cell} (offset {}): rel err {err:+.3}", cell - EMITTER);
        }
        regions[region].1.push(err);
    }
    for (name, errs) in &regions {
        let mean = errs.iter().map(|e| e.abs()).sum::<f32>() / errs.len() as f32;
        let max = errs.iter().map(|e| e.abs()).fold(0.0f32, f32::max);
        let over = errs.iter().filter(|e| e.abs() > 0.03).count();
        println!("{name:10} cells {:5} mean |rel err| {mean:.4} max {max:.4} >3%: {over}", errs.len());
        if *name != "elsewhere" {
            assert!(max < 0.02, "{name}: max |rel err| {max}");
        }
    }
}

// The render and the brute force along the start of the wall's shadow edge.
#[test]
fn shadow_edge_cells() {
    let mut scene = PixelScene::from([(EMITTER, 5.0)]);
    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    for cell in [
        IVec2::new(51, 62),
        IVec2::new(52, 63),
        IVec2::new(52, 64),
        IVec2::new(53, 65),
        IVec2::new(53, 66),
        IVec2::new(54, 67),
    ] {
        println!("{cell}: render {:.5} truth {:.5}", image[&cell], ground_truth(cell));
    }
}
