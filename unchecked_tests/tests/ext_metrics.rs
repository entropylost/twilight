// Scores `render_ext` against the path traced reference on the three offline scenes, so that fixes to the shadow leak
// and to the circle light's falloff can be compared with the same numbers. Run with
//   cargo test --release -p unchecked_tests --test ext_metrics -- --nocapture
// Prints, per scene:
//   - rel err: mean / 99th percentile / max of |ext - pt| / pt over all probes the reference lights
//   - rotation: per ring around the light, the spread (std / mean) of ext / pt; a falloff that matches the reference
//     up to a radial factor gives 0, spokes raise it. Reported as the mean and the worst ring.
//   - shadow scenes only, over the rows below the wall:
//       dip: for each row, the total amount ext decreases while walking from the dark side up to where the reference
//            peaks (the reference only rises there), relative to the row's reference peak. Mean and worst row.
//       leak: the largest ext value at a probe where the reference is black, relative to the row's reference peak.
//       band err: mean |ext - pt| / row peak within the reference's penumbra (5%..95% of the row peak) ± 4 columns.
// Set EXT_METRICS_DUMP=1 to also write out/ext_metrics_<scene>.csv with columns x, y, ext, pt, and
// EXT_METRICS_ROWS=y1,y2 to print those rows' profiles.

use std::collections::HashMap;
use std::fmt::Write;

use glam::{IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

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
    println!(
        "    per radius {}..: {:?}",
        case.rings.start,
        spreads.iter().map(|s| (s * 1000.0).round() / 1000.0).collect::<Vec<_>>()
    );

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

fn print_rows(ext: &Image, pt: &Image) {
    let Ok(rows) = std::env::var("EXT_METRICS_ROWS") else {
        return;
    };
    for y in rows.split(',').map(|y| y.trim().parse::<i32>().unwrap()) {
        println!("  row {y}: x: ext / pt");
        let mut line = String::new();
        for x in 1..128 {
            let cell = IVec2::new(x, y);
            write!(line, "{x}:{:.4}/{:.4} ", ext[&cell], pt[&cell]).unwrap();
        }
        println!("    {line}");
    }
}

#[test]
fn ext_metrics() {
    let cases = [
        Case { name: "block-shadow", scene: block_shadow(), light: Vec2::new(37.5, 33.5), rings: 5..25, shadow: true },
        Case { name: "pixel-shadow", scene: pixel_shadow(), light: Vec2::new(36.5, 32.5), rings: 4..25, shadow: true },
        Case { name: "circle-light", scene: circle_light(), light: Vec2::new(64.5, 64.5), rings: 13..60, shadow: false },
    ];
    for case in &cases {
        let ext =
            render_ext::<DirectionalPosition, Inverse2>(&case.scene, RenderExtSettings { simplify_occlusion: false });
        let pt = render_pt(&case.scene, RenderPtSettings { directions: PT_DIRECTIONS, seed: 0 });
        score(case, &ext, &pt);
        print_rows(&ext, &pt);
        if std::env::var("EXT_METRICS_DUMP").is_ok() {
            let mut csv = String::from("x,y,ext,pt\n");
            for y in 1..128 {
                for x in 1..128 {
                    let cell = IVec2::new(x, y);
                    writeln!(csv, "{x},{y},{},{}", ext[&cell], pt[&cell]).unwrap();
                }
            }
            std::fs::create_dir_all("out").unwrap();
            std::fs::write(format!("out/ext_metrics_{}.csv", case.name), csv).unwrap();
        }
    }
}
