// Independent check of a falloff fix (issue 2) on emitters the three metric scenes do not contain: discs of other
// sizes and lattice positions, a diamond (all stair steps), concave emitters (annulus, L), a tilted bar, a big light
// with occluders, two lights. Scores `render_ext` against the path traced reference like ext_metrics:
//   rel err mean / p99 / max, the mean of ext / pt, and the ring spread (std / mean of ext / pt per ring around the
//   light's centroid; only for the scenes with a single unoccluded light).
// Also writes every image to out/falloff_<TAG>_<scene>.bin; with COMPARE=<other tag> it prints how far the two builds
// differ (to check that a cost-only change is bit-identical).
//   TAG=base cargo test --release -p unchecked_tests --test ext_falloff_family -- --nocapture
// NO_PT=1 skips the reference (for the bit-identity comparison only).

use std::collections::HashMap;

use glam::{IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

type Image = HashMap<IVec2, f32>;

fn disc(scene: &mut PixelScene, centre: IVec2, r2: i32, emittance: f32) {
    for i in -20..=20 {
        for j in -20..=20 {
            if i * i + j * j < r2 {
                scene.insert(centre + IVec2::new(i, j), emittance);
            }
        }
    }
}

fn scenes() -> Vec<(&'static str, PixelScene, bool)> {
    let mut out = Vec::new();
    let mut s = PixelScene::new();
    disc(&mut s, IVec2::new(41, 71), 30, 1.0);
    out.push(("disc r5.5 at (41,71)", s, true));

    let mut s = PixelScene::new();
    disc(&mut s, IVec2::new(63, 66), 260, 0.3);
    out.push(("disc r16 at (63,66)", s, true));

    let mut s = PixelScene::new();
    for i in -9i32..=9 {
        for j in -9i32..=9 {
            if i.abs() + j.abs() <= 8 {
                s.insert(IVec2::new(61 + i, 67 + j), 0.5);
            }
        }
    }
    out.push(("diamond r8 at (61,67)", s, true));

    let mut s = PixelScene::new();
    for i in -12i32..=12 {
        for j in -12i32..=12 {
            let d = i * i + j * j;
            if (49..110).contains(&d) {
                s.insert(IVec2::new(60 + i, 62 + j), 0.5);
            }
        }
    }
    out.push(("annulus 7..10.5 at (60,62)", s, true));

    let mut s = PixelScene::new();
    for i in 0..14 {
        for j in 0..14 {
            if i < 4 || j < 4 {
                s.insert(IVec2::new(55 + i, 58 + j), 0.5);
            }
        }
    }
    out.push(("L 14x14 at (55,58)", s, true));

    let mut s = PixelScene::new();
    for i in 0..30 {
        for j in 0..2 {
            s.insert(IVec2::new(50 + i, 60 + i / 3 + j), 0.6);
        }
    }
    out.push(("bar slope 1/3", s, true));

    let mut s = PixelScene::new();
    for i in 0..20 {
        for j in 0..3 {
            s.insert(IVec2::new(50 + i + j, 50 + i), 0.6);
        }
    }
    out.push(("bar slope 1", s, true));

    // a big light whose penumbra is cast by a black disc and a wall
    let mut s = PixelScene::new();
    disc(&mut s, IVec2::new(40, 40), 70, 0.4);
    disc(&mut s, IVec2::new(66, 58), 30, 0.0);
    for i in 0..40 {
        s.insert(IVec2::new(20 + i, 80), 0.0);
    }
    for j in 0..25 {
        s.insert(IVec2::new(90, 20 + j), 0.0);
    }
    out.push(("disc r8 + occluders", s, false));

    let mut s = PixelScene::new();
    disc(&mut s, IVec2::new(35, 90), 40, 0.4);
    for i in 0..3 {
        for j in 0..3 {
            s.insert(IVec2::new(90 + i, 30 + j), 2.5);
        }
    }
    s.insert(IVec2::new(70, 70), 0.0);
    s.insert(IVec2::new(71, 71), 0.0);
    out.push(("disc + block + 2 px occluder", s, false));
    out
}

fn percentile(values: &mut [f32], p: f32) -> f32 {
    values.sort_by(f32::total_cmp);
    values[((values.len() - 1) as f32 * p).round() as usize]
}

#[test]
fn falloff_family() {
    let tag = std::env::var("TAG").unwrap_or_else(|_| "default".into());
    let compare = std::env::var("COMPARE").ok();
    let no_pt = std::env::var("NO_PT").is_ok();
    std::fs::create_dir_all("out").unwrap();
    println!("[{tag}] scene: rel err mean / p99 / max | mean ext/pt | ring spread mean / worst");
    for (k, (name, scene, rings)) in scenes().into_iter().enumerate() {
        let ext = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
        let mut bytes = Vec::new();
        for y in 1..128 {
            for x in 1..128 {
                bytes.extend_from_slice(&ext[&IVec2::new(x, y)].to_le_bytes());
            }
        }
        std::fs::write(format!("out/falloff_{tag}_{k}.bin"), &bytes).unwrap();
        if let Some(other) = &compare {
            let other_bytes = std::fs::read(format!("out/falloff_{other}_{k}.bin")).unwrap();
            let (mut max, mut differing) = (0.0f32, 0);
            for (a, b) in bytes.chunks(4).zip(other_bytes.chunks(4)) {
                let a = f32::from_le_bytes(a.try_into().unwrap());
                let b = f32::from_le_bytes(b.try_into().unwrap());
                differing += (a.to_bits() != b.to_bits()) as usize;
                max = max.max((a - b).abs());
            }
            println!("  {name}: vs {other}: {differing} probes differ, max abs diff {max:.3e}");
        }
        if no_pt {
            continue;
        }
        let pt = render_pt(&scene, RenderPtSettings { directions: 4096, seed: 0 });
        let lit: Vec<IVec2> = pt.iter().filter(|&(c, &p)| p > 1e-4 && !scene.contains_key(c)).map(|(c, _)| *c).collect();
        let mut errors: Vec<f32> = lit.iter().map(|c| (ext[c] - pt[c]).abs() / pt[c]).collect();
        let mean = errors.iter().sum::<f32>() / errors.len() as f32;
        let max = errors.iter().copied().fold(0.0, f32::max);
        let ratio = lit.iter().map(|c| ext[c] / pt[c]).sum::<f32>() / lit.len() as f32;
        let p99 = percentile(&mut errors, 0.99);
        let mut line = format!("  {name}: {mean:.4} / {p99:.4} / {max:.4} | {ratio:.4}");
        if rings {
            let emitters: Vec<Vec2> =
                scene.iter().filter(|&(_, &e)| e > 0.0).map(|(p, _)| p.as_vec2() + 0.5).collect();
            let centre = emitters.iter().sum::<Vec2>() / emitters.len() as f32;
            let reach = emitters.iter().map(|p| (*p - centre).length()).fold(0.0, f32::max);
            let first = reach.ceil() as usize + 3;
            let mut ring = vec![Vec::new(); 50];
            for c in &lit {
                let r = (c.as_vec2() - centre).length().round() as usize;
                if (first..50).contains(&r) {
                    ring[r].push(ext[c] / pt[c]);
                }
            }
            let spreads: Vec<f32> = ring
                .iter()
                .filter(|r| !r.is_empty())
                .map(|r| {
                    let mean = r.iter().sum::<f32>() / r.len() as f32;
                    (r.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / r.len() as f32).sqrt() / mean
                })
                .collect();
            let mean = spreads.iter().sum::<f32>() / spreads.len() as f32;
            let worst = spreads.iter().copied().fold(0.0, f32::max);
            line += &format!(" | {mean:.4} / {worst:.4}");
        }
        println!("{line}");
    }
}
