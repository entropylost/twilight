// Independent check of the shadow-leak root cause / fix candidates (issue 1 of docs/EXT_DEBUG.md) on scenes other than
// the three offline ones: walls in other orientations and positions, a slit, a block occluder, a staircase wall, and
// unoccluded lights of several sizes at odd positions (rotational artifacts). Everything is scored against render_pt.
//   cargo test --release -p unchecked_tests --test ext_leak_refute general -- --nocapture
//   cargo test --release -p unchecked_tests --test ext_leak_refute cost -- --nocapture
// Per scene:
//   rel err: mean / p99 of |ext - pt| / pt where the reference is lit
//   leak: over probes the reference leaves black that have a lit probe within 4 cells, ext / (brightest reference
//         value within 4 cells); mean and max
//   xtv: excess total variation along rows and columns, sum(max(0, TV(ext) - TV(pt))) / sum(TV(pt)); a profile that
//        is as monotone as the reference gives 0, spikes and dips raise it
//   ring: (unoccluded scenes) spread (std / mean) of ext / pt per ring around the light, mean and worst ring
// SCENES=name,name restricts the scenes; PNG=1 writes out/refute_<scene>.png (ext | pt side by side, gamma 2.2).

use std::collections::HashMap;
use std::time::Instant;

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::{Extension, Fluence, PixelScene};
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

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
    // unoccluded: score rings around the light's centroid, radius range
    rings: Option<std::ops::Range<usize>>,
}

fn cases() -> Vec<Case> {
    let mut v = Vec::new();
    let mut add = |name, rings, build: &dyn Fn(&mut PixelScene)| {
        let mut scene = PixelScene::new();
        build(&mut scene);
        v.push(Case { name, scene, rings });
    };
    add("vwall", None, &|s| {
        rect(s, 90..91, 41..42, 5.0);
        rect(s, 57..58, 30..128, 0.0);
    });
    add("hwall-up", None, &|s| {
        rect(s, 70..71, 95..96, 5.0);
        rect(s, 61..128, 70..71, 0.0);
    });
    add("slit", None, &|s| {
        rect(s, 64..65, 20..21, 5.0);
        rect(s, 0..59, 50..51, 0.0);
        rect(s, 66..128, 50..51, 0.0);
    });
    add("blockocc", None, &|s| {
        rect(s, 30..33, 30..33, 2.5);
        rect(s, 60..70, 55..65, 0.0);
    });
    add("diag", None, &|s| {
        rect(s, 20..21, 64..65, 5.0);
        for i in 0..30 {
            rect(s, 50 + i..51 + i, 40 + i..42 + i, 0.0);
        }
    });
    add("thickwall", None, &|s| {
        rect(s, 43..46, 35..38, 2.5);
        rect(s, 0..71, 77..81, 0.0);
    });
    add("pix-odd", Some(4..40), &|s| rect(s, 71..72, 53..54, 5.0));
    add("block2", Some(4..40), &|s| rect(s, 50..52, 77..79, 3.0));
    add("block5", Some(6..40), &|s| rect(s, 61..66, 59..64, 1.0));
    add("bar", Some(7..40), &|s| rect(s, 60..67, 70..71, 2.0));
    v
}

fn percentile(values: &mut [f32], p: f32) -> f32 {
    values.sort_by(f32::total_cmp);
    values[((values.len() - 1) as f32 * p).round() as usize]
}

fn score(case: &Case, ext: &Image, pt: &Image) {
    let solid = |c: &IVec2| case.scene.contains_key(c);
    let mut errors: Vec<f32> =
        pt.iter().filter(|&(c, &p)| p > 1e-4 && !solid(c)).map(|(c, &p)| (ext[c] - p).abs() / p).collect();
    let mean = errors.iter().sum::<f32>() / errors.len() as f32;
    let p99 = percentile(&mut errors, 0.99);

    let mut leaks = Vec::new();
    for (&c, &p) in pt {
        if p != 0.0 || solid(&c) {
            continue;
        }
        let mut near = 0.0f32;
        for dx in -4..=4 {
            for dy in -4..=4 {
                let n = c + IVec2::new(dx, dy);
                if !solid(&n) {
                    near = near.max(pt.get(&n).copied().unwrap_or(0.0));
                }
            }
        }
        if near > 1e-4 {
            leaks.push(ext[&c] / near);
        }
    }
    let (leak_mean, leak_max) = if leaks.is_empty() {
        (0.0, 0.0)
    } else {
        (leaks.iter().sum::<f32>() / leaks.len() as f32, leaks.iter().copied().fold(0.0, f32::max))
    };

    // excess total variation along rows and columns, segments broken at solid pixels
    let (mut excess, mut tv_ref) = (0.0, 0.0);
    for axis in 0..2 {
        for a in 1..128 {
            let (mut te, mut tp) = (0.0, 0.0);
            let mut flush = |te: &mut f32, tp: &mut f32| {
                excess += (*te - *tp).max(0.0);
                tv_ref += *tp;
                (*te, *tp) = (0.0, 0.0);
            };
            for b in 1..127 {
                let (c, n) = if axis == 0 {
                    (IVec2::new(b, a), IVec2::new(b + 1, a))
                } else {
                    (IVec2::new(a, b), IVec2::new(a, b + 1))
                };
                if solid(&c) || solid(&n) {
                    flush(&mut te, &mut tp);
                    continue;
                }
                te += (ext[&n] - ext[&c]).abs();
                tp += (pt[&n] - pt[&c]).abs();
            }
            flush(&mut te, &mut tp);
        }
    }
    print!(
        "{:10} rel {mean:.4}/{p99:.4}  leak {leak_mean:.4}/{leak_max:.4}  xtv {:.4}",
        case.name,
        excess / tv_ref
    );

    if let Some(range) = &case.rings {
        let n = case.scene.len() as f32;
        let light = case.scene.keys().map(|k| k.as_vec2() + 0.5).sum::<Vec2>() / n;
        let mut rings = vec![Vec::new(); range.end];
        for (&c, &p) in pt {
            let r = (c.as_vec2() - light).length().round() as usize;
            if range.contains(&r) && p > 1e-4 && !solid(&c) {
                rings[r].push(ext[&c] / p);
            }
        }
        let spreads: Vec<f32> = rings[range.start..]
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
        print!("  ring {mean:.4}/{worst:.4}");
    }
    println!();
}

fn write_png(name: &str, ext: &Image, pt: &Image) {
    let mut lit: Vec<f32> = pt.values().copied().filter(|&v| v > 1e-4).collect();
    let max = percentile(&mut lit, 0.5) * 3.0;
    const S: u32 = 3;
    let mut img = image::GrayImage::new(2 * 128 * S, 128 * S);
    for (k, image) in [ext, pt].into_iter().enumerate() {
        for (&c, &v) in image {
            let g = ((v / max).clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
            for i in 0..S {
                for j in 0..S {
                    img.put_pixel((c.x as u32 + 128 * k as u32) * S + i, c.y as u32 * S + j, image::Luma([g]));
                }
            }
        }
    }
    std::fs::create_dir_all("out").unwrap();
    img.save(format!("out/refute_{name}.png")).unwrap();
}

#[test]
fn general() {
    let only = std::env::var("SCENES").ok();
    println!("EXT_MODE = {:?}", std::env::var("EXT_MODE").ok());
    for case in cases() {
        if let Some(only) = &only
            && !only.split(',').any(|n| n == case.name)
        {
            continue;
        }
        let ext =
            render_ext::<DirectionalPosition, Inverse2>(&case.scene, RenderExtSettings { simplify_occlusion: false });
        let pt = render_pt(&case.scene, RenderPtSettings { directions: 2048, seed: 0 });
        score(&case, &ext, &pt);
        if std::env::var("PNG").is_ok() {
            let tag = format!("{}_m{}", case.name, std::env::var("EXT_MODE").unwrap_or("0".into()));
            write_png(&tag, &ext, &pt);
        }
    }
}

// Number of distinct extensions / frustums evaluated for a whole render (the caches of render_ext), and the time.
#[test]
fn cost() {
    println!("EXT_MODE = {:?}", std::env::var("EXT_MODE").ok());
    let mut scene = PixelScene::new();
    rect(&mut scene, 36..37, 32..33, 5.0);
    rect(&mut scene, 0..50, 60..61, 0.0);
    let rot = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(std::f32::consts::FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [Affine2::IDENTITY, rot, rot * rot, rot * rot * rot];
    let scenes = transforms.map(|t| {
        scene.iter().map(|(k, v)| (t.transform_point2(k.as_vec2() + 0.5).floor().as_ivec2(), *v)).collect::<PixelScene>()
    });
    let start = Instant::now();
    let mut caches =
        HashMap::<_, (HashMap<Extension, Fluence<DirectionalPosition>>, HashMap<Frustum, Inverse2>)>::new();
    for x in 1..128 {
        for y in 1..128 {
            for (k, (t, scene)) in transforms.iter().zip(&scenes).enumerate() {
                let probe = t.transform_point2(Vec2::new(x as f32, y as f32)).round().as_ivec2();
                for i in [-1, 0] {
                    let (ext_cache, cache) = caches.entry((k, i)).or_default();
                    get_radiance_recursive_ext(
                        ext_cache,
                        cache,
                        Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i }),
                        scene,
                        RenderExtSettings { simplify_occlusion: false },
                    );
                }
            }
        }
    }
    let elapsed = start.elapsed();
    let exts: usize = caches.values().map(|c| c.0.len()).sum();
    let (mut lines, mut cones) = (0, 0);
    for c in caches.values() {
        for e in c.0.keys() {
            match e {
                Extension::Line(_) => lines += 1,
                Extension::Cone(_) => cones += 1,
            }
        }
    }
    let frustums: usize = caches.values().map(|c| c.1.len()).sum();
    println!("extensions cached {exts} (lines {lines}, cones {cones}), frustums cached {frustums}, time {elapsed:?}");
}

// ext / pt around an unoccluded single-pixel light (the "pix-odd" scene), to see where a variant's falloff deviates.
// R = half window (default 8).
#[test]
fn near() {
    let mut scene = PixelScene::new();
    rect(&mut scene, 71..72, 53..54, 5.0);
    let r: i32 = std::env::var("R").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let ext = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    let pt = render_pt(&scene, RenderPtSettings { directions: 8192, seed: 0 });
    for y in 53 - r..=54 + r {
        let mut line = String::new();
        for x in 71 - r..=72 + r {
            let c = IVec2::new(x, y);
            line += &format!("{:5.0}", (ext[&c] / pt[&c] - 1.0) * 1000.0);
        }
        println!("{line}");
    }
}
