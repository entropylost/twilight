// Dumps the offline --ext scene's radiance per probe direction, with and without the wall, so the shadow below the
// wall and the wall's own cells can be inspected numerically. Writes out/ext_shadow.csv with columns
// x, y, wall, k, i, value where k is the scene rotation and i the size-1 cone index.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::fmt::Write;

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance};
use twilight::range2d;
use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext};

fn scene(wall: bool) -> PixelScene {
    let mut scene = PixelScene::new();
    let pos = IVec2::new(36, 32);
    for i in 0..8 {
        for j in 0..8 {
            scene.insert(pos + IVec2::new(i, j), 1.0);
        }
    }
    if wall {
        for i in 0..50 {
            scene.insert(IVec2::new(i, 60), 0.0);
        }
    }
    scene
}

// render_ext, but keeping each (rotation, cone index) contribution apart
// gate: also block a size-1 cone when the pixel between the probe and the cone's first extension is solid
fn render_split(scene: &PixelScene, gate: bool) -> HashMap<(IVec2, usize, i32), f32> {
    let rotate = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [Affine2::IDENTITY, rotate, rotate * rotate, rotate * rotate * rotate];
    let scenes = transforms.map(|t| {
        scene.iter().map(|(k, v)| (t.transform_point2(k.as_vec2() + 0.5).floor().as_ivec2(), *v)).collect::<PixelScene>()
    });
    let mut caches = HashMap::<_, (HashMap<_, _>, HashMap<Frustum, Inverse2>)>::new();
    let mut out = HashMap::new();
    for cell in range2d(IVec2::splat(1), IVec2::splat(128)) {
        for (k, (t, scene)) in transforms.iter().zip(&scenes).enumerate() {
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let (ext_cache, cache) = caches.entry((k, i)).or_default();
                let radiance: Inverse2 =
                    get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(ext_cache, cache, frustum, scene, RenderExtSettings { simplify_occlusion: false });
                let adjacent = probe + IVec2::new(0, i);
                let transmission = if gate && scene.get(&adjacent) == Some(&0.0) { 0.0 } else { 1.0 };
                out.insert((cell, k, i), radiance.at_focus(frustum) * transmission);
            }
        }
    }
    out
}

#[test]
fn ext_shadow() {
    let mut csv = String::from("x,y,wall,k,i,value\n");
    for wall in [false, true] {
        for ((cell, k, i), v) in render_split(&scene(wall), std::env::var("GATE").is_ok()) {
            writeln!(csv, "{},{},{},{},{},{}", cell.x, cell.y, wall as i32, k, i, v).unwrap();
        }
    }
    std::fs::create_dir_all("out").unwrap();
    std::fs::write("out/ext_shadow.csv", csv).unwrap();
}
