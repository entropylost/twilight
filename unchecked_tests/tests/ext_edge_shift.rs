// Is a higher "leak worst" in ext_metrics (one probe of one row) a shadow edge that moved, or noise at the edge?
// For the two shadow scenes, per row below the wall, walking from the dark side up to the reference's peak:
//   shift:  sum of (ext - pt) / peak. The row's profile is a step from 0 to the peak, so this is the distance in
//           pixels by which ext's shadow edge is displaced into the dark side (positive) or the lit side (negative).
//   blur:   sum of |ext - pt| / peak, pixels.
//   leak:   as in ext_metrics (max ext where pt == 0, / peak).
// Prints the mean over rows of each, and writes the per-row leaks to out/edge_shift_<TAG>_<scene>.txt; with
// COMPARE=<tag> it counts the rows whose leak went up / down against that build.
//   TAG=base cargo test --release -p unchecked_tests --test ext_edge_shift -- --nocapture

use std::collections::HashMap;

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

const WALL_Y: i32 = 60;
type Image = HashMap<IVec2, f32>;

fn scene(size: i32, emittance: f32, wall: i32) -> PixelScene {
    let mut scene = PixelScene::new();
    for i in 0..size {
        for j in 0..size {
            scene.insert(IVec2::new(36 + i, 32 + j), emittance);
        }
    }
    for i in 0..wall {
        scene.insert(IVec2::new(i, WALL_Y), 0.0);
    }
    scene
}

#[test]
fn edge_shift() {
    let tag = std::env::var("TAG").unwrap_or_else(|_| "default".into());
    let compare = std::env::var("COMPARE").ok();
    std::fs::create_dir_all("out").unwrap();
    for (name, scene) in [
        ("block-shadow", scene(3, 2.5, 50)),
        ("pixel-shadow", scene(1, 5.0, 50)),
        ("pixel wall 47", scene(1, 5.0, 47)),
        ("block wall 53", scene(3, 2.5, 53)),
    ] {
        let ext = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
        let pt = render_pt(&scene, RenderPtSettings { directions: 4096, seed: 0 });
        let (mut shifts, mut blurs, mut leaks) = (Vec::new(), Vec::new(), Vec::new());
        for y in WALL_Y + 2..128 {
            let row = |image: &Image| (1..128).map(|x| image[&IVec2::new(x, y)]).collect::<Vec<f32>>();
            let (e, p) = (row(&ext), row(&pt));
            let peak_at = (0..p.len()).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
            let peak = p[peak_at];
            shifts.push((0..=peak_at).map(|x| e[x] - p[x]).sum::<f32>() / peak);
            blurs.push((0..=peak_at).map(|x| (e[x] - p[x]).abs()).sum::<f32>() / peak);
            leaks.push((0..p.len()).filter(|&x| p[x] == 0.0).map(|x| e[x]).fold(0.0, f32::max) / peak);
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
        let mut sorted = leaks.clone();
        sorted.sort_by(f32::total_cmp);
        println!(
            "[{tag}] {name}: shift mean {:+.4} rms {:.4} px | blur mean {:.4} px | leak mean {:.4} median {:.4} p90 {:.4} worst {:.4}",
            mean(&shifts),
            rms(&shifts),
            mean(&blurs),
            mean(&leaks),
            sorted[sorted.len() / 2],
            sorted[sorted.len() * 9 / 10],
            sorted[sorted.len() - 1],
        );
        let file = format!("out/edge_shift_{}_{}.txt", tag, name.replace(' ', "_"));
        std::fs::write(&file, leaks.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        if let Some(other) = &compare {
            let other_file = format!("out/edge_shift_{}_{}.txt", other, name.replace(' ', "_"));
            let other: Vec<f32> =
                std::fs::read_to_string(other_file).unwrap().lines().map(|l| l.parse().unwrap()).collect();
            let up = leaks.iter().zip(&other).filter(|(a, b)| a > &&(**b + 0.005)).count();
            let down = leaks.iter().zip(&other).filter(|(a, b)| **a + 0.005 < **b).count();
            println!("    rows whose leak is higher: {up}, lower: {down}, of {}", leaks.len());
        }
    }
}
