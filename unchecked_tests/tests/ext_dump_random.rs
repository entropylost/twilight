// Dumps `render_ext` of a cluttered scene (random single-pixel occluders, short walls, three lights) to
// out/ext_random_<TAG>.csv (x,y,ext), and, when COMPARE=<other tag> is set, prints how far the two dumps differ.
// Used to check whether a change that is bit-identical on the three metric scenes (e.g. reading a far cone's
// transmittance from the near cone extension's end instead of a separate line extension) changes results in general:
// dump with one build, change src, dump with another TAG and COMPARE.
//   TAG=a cargo test --release -p unchecked_tests --test ext_dump_random -- --nocapture
//   TAG=b COMPARE=a cargo test --release -p unchecked_tests --test ext_dump_random -- --nocapture

use std::fmt::Write;

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};

fn scene() -> PixelScene {
    let mut scene = PixelScene::new();
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = |n: i32| {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((state >> 33) % n as u64) as i32
    };
    for _ in 0..160 {
        scene.insert(IVec2::new(next(128), next(128)), 0.0);
    }
    for _ in 0..8 {
        let (x, y, horizontal, length) = (next(110), next(110), next(2) == 0, 3 + next(12));
        for k in 0..length {
            scene.insert(IVec2::new(x, y) + if horizontal { IVec2::X } else { IVec2::Y } * k, 0.0);
        }
    }
    for i in 0..3 {
        for j in 0..3 {
            scene.insert(IVec2::new(40 + i, 50 + j), 2.0);
        }
    }
    scene.insert(IVec2::new(90, 30), 5.0);
    for i in 0..5 {
        scene.insert(IVec2::new(80 + i, 95 + (i * 2) / 3), 1.0);
    }
    scene
}

#[test]
fn dump_random() {
    let tag = std::env::var("TAG").unwrap_or_else(|_| "default".into());
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene(), RenderExtSettings { simplify_occlusion: false });
    let mut csv = String::from("x,y,ext\n");
    for y in 1..128 {
        for x in 1..128 {
            writeln!(csv, "{x},{y},{}", image[&IVec2::new(x, y)]).unwrap();
        }
    }
    std::fs::create_dir_all("out").unwrap();
    std::fs::write(format!("out/ext_random_{tag}.csv"), &csv).unwrap();
    if let Ok(other) = std::env::var("COMPARE") {
        let other_csv = std::fs::read_to_string(format!("out/ext_random_{other}.csv")).unwrap();
        let (mut max, mut differing, mut sum, mut count) = (0.0f32, 0, 0.0f32, 0);
        for (a, b) in csv.lines().zip(other_csv.lines()).skip(1) {
            let value = |line: &str| line.rsplit(',').next().unwrap().parse::<f32>().unwrap();
            let (a, b) = (value(a), value(b));
            let diff = (a - b).abs();
            max = max.max(diff);
            differing += (diff > 0.0) as usize;
            sum += diff / a.max(b).max(1e-6);
            count += 1;
        }
        println!(
            "{tag} vs {other}: {differing} of {count} probes differ, max abs diff {max:.6}, mean rel diff {:.6}",
            sum / count as f32
        );
    }
}
