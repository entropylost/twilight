// The shadow scores of ext_metrics (dip, leak, rel err; see the top of ext_metrics.rs) for the two shadow scenes with
// `simplify_occlusion` on and off, since ext_metrics only covers it off.
//   cargo test --release -p unchecked_tests --test ext_simplify -- --nocapture

use std::collections::HashMap;

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

const WALL_Y: i32 = 60;
type Image = HashMap<IVec2, f32>;

fn scene(block: bool) -> PixelScene {
    let mut scene = PixelScene::new();
    let n = if block { 3 } else { 1 };
    for i in 0..n {
        for j in 0..n {
            scene.insert(IVec2::new(36 + i, 32 + j), if block { 2.5 } else { 5.0 });
        }
    }
    for i in 0..50 {
        scene.insert(IVec2::new(i, WALL_Y), 0.0);
    }
    scene
}

fn score(name: &str, scene: &PixelScene, ext: &Image, pt: &Image) {
    let errors: Vec<f32> = pt
        .iter()
        .filter(|&(cell, &p)| p > 1e-4 && !scene.contains_key(cell))
        .map(|(cell, &p)| (ext[cell] - p).abs() / p)
        .collect();
    let rel = errors.iter().sum::<f32>() / errors.len() as f32;
    let (mut dips, mut leaks) = (Vec::new(), Vec::new());
    for y in WALL_Y + 2..128 {
        let row = |image: &Image| (1..128).map(|x| image[&IVec2::new(x, y)]).collect::<Vec<f32>>();
        let (e, p) = (row(ext), row(pt));
        let peak_at = (0..p.len()).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
        let peak = p[peak_at];
        dips.push((0..peak_at).map(|x| (e[x] - e[x + 1]).max(0.0)).sum::<f32>() / peak);
        leaks.push((0..p.len()).filter(|&x| p[x] == 0.0).map(|x| e[x]).fold(0.0, f32::max) / peak);
    }
    let stats = |v: &[f32]| (v.iter().sum::<f32>() / v.len() as f32, v.iter().copied().fold(0.0, f32::max));
    let ((dip, dip_worst), (leak, leak_worst)) = (stats(&dips), stats(&leaks));
    println!("{name:28} rel {rel:.4}  dip {dip:.4}/{dip_worst:.4}  leak {leak:.4}/{leak_worst:.4}");
}

#[test]
fn simplify() {
    for (name, block) in [("block-shadow", true), ("pixel-shadow", false)] {
        let scene = scene(block);
        let pt = render_pt(&scene, RenderPtSettings { directions: 4096, seed: 0 });
        for simplify_occlusion in [false, true] {
            let ext = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion });
            score(&format!("{name} simplify={simplify_occlusion}"), &scene, &ext, &pt);
        }
    }
}
