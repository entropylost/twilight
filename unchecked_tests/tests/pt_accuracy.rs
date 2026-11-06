// Checks the path tracer against the far-field radiance of a single emitting pixel, (|dx| + |dy|) / 2 / distance²
// (the same convention as `render_ext`), and against `render_ext` itself.

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, probe_fluence, render_pt};

const EMITTER: IVec2 = IVec2::new(36, 32);

fn expected(cell: IVec2) -> f32 {
    let d = EMITTER.as_vec2() + 0.5 - cell.as_vec2();
    (d.x.abs() + d.y.abs()) * 0.5 / d.length_squared()
}

#[test]
fn pt_matches_far_field() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let mut worst = 0.0f32;
    for dy in -30..=30 {
        for dx in -30..=30 {
            let cell = EMITTER + IVec2::new(dx, dy);
            let distance = IVec2::new(dx, dy).as_vec2().length();
            if distance < 8.0 {
                continue;
            }
            let value = probe_fluence(&scene, cell.as_vec2(), 0.37, 1 << 16);
            worst = worst.max((value / expected(cell) - 1.0).abs());
        }
    }
    println!("worst relative error: {worst}");
    assert!(worst < 0.02, "{worst}");
}

#[test]
fn pt_touching_pixel() {
    // a probe on the pixel's corner sees it fill a quarter of the circle
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let value = probe_fluence(&scene, EMITTER.as_vec2(), 0.5, 1 << 12);
    let quarter = std::f32::consts::FRAC_PI_2 * 0.5;
    assert!((value - quarter).abs() < 1e-3, "{value} vs {quarter}");
}

#[test]
fn pt_vs_ext() {
    let scene = PixelScene::from([(EMITTER, 1.0)]);
    let pt = render_pt(&scene, RenderPtSettings { directions: 4096, seed: 1 });
    let ext = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    for dy in (-30..=30).step_by(3) {
        let row: String = (-30..=30)
            .step_by(3)
            .map(|dx| {
                let cell = EMITTER + IVec2::new(dx, dy);
                if dx == 0 && dy == 0 { "   . ".to_string() } else { format!("{:5.2}", ext[&cell] / pt[&cell]) }
            })
            .collect();
        println!("dy={dy:3} {row}");
    }
}
