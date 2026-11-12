// Is a change of ext_metrics' "leak worst" (one probe of one row per scene) a property of the renderer, or of where
// the two metric scenes happen to put the shadow edge?
// Scores the shadow metrics of ext_metrics (dip, leak, band err; same definitions) over a family of scenes: a wall on
// row 60 over columns 0..W and a light on rows 32.. that is either above the wall (column 36, as in the metric scenes:
// the shadow edge runs away from the wall, the shadowed probes look past the wall's end towards a light that lies on
// the wall's side) or beyond the wall's end (column 2 W - 37: the shadowed probes are under the wall and look across
// its end). Prints one line per scene and the averages per class.
//   cargo test --release -p unchecked_tests --test ext_leak_sweep -- --nocapture
// SWEEP_ROW=y also prints row y of every scene as x:ext/pt.

use std::collections::HashMap;

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

const WALL_Y: i32 = 60;
type Image = HashMap<IVec2, f32>;

// [dip mean, dip worst, leak mean, leak worst, band err mean], as in ext_metrics
fn shadow_scores(ext: &Image, pt: &Image) -> [f32; 5] {
    let (mut dips, mut leaks, mut bands) = (Vec::new(), Vec::new(), Vec::new());
    for y in WALL_Y + 2..128 {
        let row = |image: &Image| (1..128).map(|x| image[&IVec2::new(x, y)]).collect::<Vec<f32>>();
        let (e, p) = (row(ext), row(pt));
        let peak_at = (0..p.len()).max_by(|&a, &b| p[a].total_cmp(&p[b])).unwrap();
        let peak = p[peak_at];
        dips.push((0..peak_at).map(|x| (e[x] - e[x + 1]).max(0.0)).sum::<f32>() / peak);
        leaks.push((0..p.len()).filter(|&x| p[x] == 0.0).map(|x| e[x]).fold(0.0, f32::max) / peak);
        let in_band = |x: usize| p[x] > 0.05 * peak && p[x] < 0.95 * peak && x <= peak_at;
        let band: Vec<f32> = (0..p.len())
            .filter(|&x| (x.saturating_sub(4)..=(x + 4).min(p.len() - 1)).any(in_band))
            .map(|x| (e[x] - p[x]).abs() / peak)
            .collect();
        if !band.is_empty() {
            bands.push(band.iter().sum::<f32>() / band.len() as f32);
        }
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
    let worst = |v: &[f32]| v.iter().copied().fold(0.0, f32::max);
    [mean(&dips), worst(&dips), mean(&leaks), worst(&leaks), mean(&bands)]
}

#[test]
fn leak_sweep() {
    let mut classes: HashMap<&str, Vec<[f32; 5]>> = HashMap::new();
    println!("scene: dip mean / worst, leak mean / worst, band err mean");
    for size in [1, 3] {
        for wall in [44, 47, 50, 53, 56] {
            for (class, light_x) in [("light above the wall", 36), ("light beyond the wall's end", 2 * wall - 37)] {
                let mut scene = PixelScene::new();
                for i in 0..size {
                    for j in 0..size {
                        scene.insert(IVec2::new(light_x + i, 32 + j), 5.0);
                    }
                }
                for x in 0..wall {
                    scene.insert(IVec2::new(x, WALL_Y), 0.0);
                }
                let ext = render_ext::<DirectionalPosition, Inverse2>(
                    &scene,
                    RenderExtSettings { simplify_occlusion: false },
                );
                let pt = render_pt(&scene, RenderPtSettings { directions: 2048, seed: 0 });
                // SWEEP_ROW=y prints that row of every scene as x:ext/pt
                if let Ok(row) = std::env::var("SWEEP_ROW") {
                    let y: i32 = row.parse().unwrap();
                    let line: Vec<String> = (1..128)
                        .map(|x| format!("{x}:{:.4}/{:.4}", ext[&IVec2::new(x, y)], pt[&IVec2::new(x, y)]))
                        .collect();
                    println!("    row {y}: {}", line.join(" "));
                }
                let s = shadow_scores(&ext, &pt);
                println!(
                    "  light {size}x{size} at x={light_x:2}, wall 0..{wall}: {:.4} / {:.4}, {:.4} / {:.4}, {:.4}",
                    s[0], s[1], s[2], s[3], s[4]
                );
                classes.entry(class).or_default().push(s);
            }
        }
    }
    for (class, scores) in &classes {
        let mean = |k: usize| scores.iter().map(|s| s[k]).sum::<f32>() / scores.len() as f32;
        let worst = |k: usize| scores.iter().map(|s| s[k]).fold(0.0, f32::max);
        println!(
            "{class}: dip mean {:.4} (max worst {:.4}), leak mean {:.4}, leak worst mean {:.4} (max {:.4}), band err {:.4}",
            mean(0),
            worst(1),
            mean(2),
            mean(3),
            worst(3),
            mean(4)
        );
    }
}
