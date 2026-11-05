// Writes the raw `offline --ext` radiance values to $EXT_DUMP (and $EXT_DUMP.s for simplify_occlusion), or, when
// $EXT_DUMP_BASE is also set, compares them against the dumps at $EXT_DUMP_BASE and prints the largest difference.

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::*;
use twilight::render::ext::{RenderExtSettings, render_ext};

fn values(simplify_occlusion: bool) -> Vec<f32> {
    let mut scene = PixelScene::new();
    let pos = IVec2::new(36, 32);
    for i in 0..8 {
        for j in 0..8 {
            scene.insert(pos + IVec2::new(i, j), 1.0);
        }
    }
    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion });
    let mut cells = image.into_iter().collect::<Vec<_>>();
    cells.sort_by_key(|(cell, _)| (cell.y, cell.x));
    cells.into_iter().map(|(_, value)| value).collect()
}

#[test]
fn ext_dump() {
    let Ok(path) = std::env::var("EXT_DUMP") else {
        return;
    };
    let base = std::env::var("EXT_DUMP_BASE").ok();
    for (simplify_occlusion, suffix) in [(false, ""), (true, ".s")] {
        let values = values(simplify_occlusion);
        let text = values.iter().map(|v| format!("{v:e}")).collect::<Vec<_>>().join("\n");
        std::fs::write(format!("{path}{suffix}"), &text).unwrap();
        if let Some(base) = &base {
            let base = std::fs::read_to_string(format!("{base}{suffix}")).unwrap();
            let base = base.lines().map(|l| l.parse::<f32>().unwrap()).collect::<Vec<_>>();
            assert_eq!(base.len(), values.len());
            let (mut max_abs, mut max_rel) = (0.0f32, 0.0f32);
            for (a, b) in base.iter().zip(&values) {
                max_abs = max_abs.max((a - b).abs());
                max_rel = max_rel.max((a - b).abs() / a.abs().max(1e-6));
            }
            println!("simplify_occlusion={simplify_occlusion}: max abs {max_abs:e}, max rel {max_rel:e}");
        }
    }
}
