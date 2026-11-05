// Prints render_ext values on probes close to emitters, to check that probes touching or one pixel away from a light
// still receive its emission (and that nothing leaks through thin walls). Run with --nocapture.

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, render_ext};

fn block(scene: &mut PixelScene, pos: IVec2, size: IVec2, value: f32) {
    for i in 0..size.x {
        for j in 0..size.y {
            scene.insert(pos + IVec2::new(i, j), value);
        }
    }
}

fn print_window(image: &std::collections::HashMap<IVec2, f32>, min: IVec2, max: IVec2) {
    print!("     ");
    for x in min.x..max.x {
        print!("{x:>6}");
    }
    println!();
    for y in min.y..max.y {
        print!("{y:>4} ");
        for x in min.x..max.x {
            print!("{:>6.3}", image.get(&IVec2::new(x, y)).copied().unwrap_or(f32::NAN));
        }
        println!();
    }
}

#[test]
fn ext_near_light() {
    // the offline --ext emitter
    let mut scene = PixelScene::new();
    block(&mut scene, IVec2::new(36, 32), IVec2::splat(8), 1.0);
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    println!("8x8 emitter at (36..44, 32..40), probes around its top-left corner");
    print_window(&image, IVec2::new(28, 24), IVec2::new(42, 38));

    // a single pixel light
    let mut scene = PixelScene::new();
    scene.insert(IVec2::new(64, 64), 1.0);
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    println!("1x1 emitter at pixel (64, 64), probes (60..70)^2");
    print_window(&image, IVec2::splat(60), IVec2::splat(70));

    // a light directly behind a 1px wall: probes on the far side must stay dark
    let mut scene = PixelScene::new();
    block(&mut scene, IVec2::new(64, 60), IVec2::new(1, 9), 1.0);
    block(&mut scene, IVec2::new(62, 56), IVec2::new(1, 17), 0.0);
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    println!("emitter column x=64 (y 60..69) behind wall x=62 (y 56..73)");
    print_window(&image, IVec2::new(58, 54), IVec2::new(68, 76));
}

// render_ext without its probe-pixel step: the size-1 cones' radiance summed as it was before
fn render_without_probe_pixel(scene: &PixelScene) -> std::collections::HashMap<IVec2, f32> {
    use std::collections::HashMap;
    use std::f32::consts::FRAC_PI_2;

    use glam::{Affine2, Vec2};
    use twilight::frustum::{ConeFrustum, Frustum};
    use twilight::radiance::Radiance;
    use twilight::range2d;
    use twilight::render::ext::get_radiance_recursive_ext;

    let rotate = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [Affine2::IDENTITY, rotate, rotate * rotate, rotate * rotate * rotate];
    let scenes = transforms.map(|t| {
        scene
            .iter()
            .map(|(k, v)| (t.transform_point2(k.as_vec2() + 0.5).floor().as_ivec2(), *v))
            .collect::<PixelScene>()
    });
    let mut caches = HashMap::<_, (HashMap<_, _>, HashMap<Frustum, Inverse2>)>::new();
    let mut image = HashMap::new();
    for cell in range2d(IVec2::splat(1), IVec2::splat(128)) {
        let mut total = 0.0;
        for (k, (t, scene)) in transforms.iter().zip(&scenes).enumerate() {
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let (ext_cache, cache) = caches.entry((k, i)).or_default();
                let radiance: Inverse2 = get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(
                    ext_cache,
                    cache,
                    frustum,
                    scene,
                    RenderExtSettings { simplify_occlusion: false },
                );
                total += radiance.at_focus(frustum);
            }
        }
        image.insert(cell, total);
    }
    image
}

// On the offline --ext scene, the probe-pixel step darkens the probes under the wall and only changes probes that touch
// a solid pixel or are a pixel from an emitter.
#[test]
fn ext_probe_pixel_changes() {
    let mut scene = PixelScene::new();
    block(&mut scene, IVec2::new(36, 32), IVec2::splat(8), 1.0);
    block(&mut scene, IVec2::new(0, 60), IVec2::new(50, 1), 0.0);
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene, RenderExtSettings { simplify_occlusion: false });
    let reference = render_without_probe_pixel(&scene);

    // the wall's underside; its top, y = 60, faces the light
    for x in 1..50 {
        assert_eq!(image[&IVec2::new(x, 61)], 0.0, "probe ({x}, 61) is under the wall");
    }
    // cell c touches pixels c - (0|1, 0|1); the probe step's next pixel is at most one further
    let touches = |cell: IVec2, reach: i32, wanted: &dyn Fn(f32) -> bool| {
        (-1 - reach..=reach).any(|dx| {
            (-1 - reach..=reach).any(|dy| scene.get(&(cell + IVec2::new(dx, dy))).is_some_and(|&v| wanted(v)))
        })
    };
    let mut changed = 0;
    for (cell, &value) in &image {
        let before = reference[cell];
        if (value - before).abs() > 1e-6 {
            changed += 1;
            assert!(
                touches(*cell, 0, &|_| true) || touches(*cell, 1, &|v| v > 0.0),
                "probe {cell} changed from {before} to {value} away from the scene"
            );
        }
    }
    println!("{changed} probes changed");
}
