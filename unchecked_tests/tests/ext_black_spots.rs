// Diagnoses the two black spots left of the emitter block in the offline `--ext` render, which were negative radiance
// from Inverse2 extrapolating 1 / value past zero.

use glam::IVec2;
use twilight::extension::PixelScene;
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::render_ext;

fn offline_scene() -> PixelScene {
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
    scene
}

#[test]
fn ext_black_spots() {
    let image = render_ext::<DirectionalPosition, Inverse2>(&offline_scene());
    for y in 26..48 {
        let row: Vec<String> = (26..50).map(|x| format!("{:5.2}", image[&IVec2::new(x, y)])).collect();
        println!("y={y:2} {}", row.join(" "));
    }
    let negative: Vec<_> = image.iter().filter(|(_, v)| **v < 0.0).collect();
    assert!(negative.is_empty(), "negative radiance at {negative:?}");
}

// Splits the radiance at a dark cell into the contributions of its eight root frustums.
#[test]
fn ext_black_spot_roots() {
    use std::collections::HashMap;
    use std::f32::consts::FRAC_PI_2;

    use glam::{Affine2, Vec2};
    use twilight::frustum::{ConeFrustum, Frustum};
    use twilight::radiance::Radiance;
    use twilight::render::ext::get_radiance_recursive_ext;

    let scene = offline_scene();
    let rotate = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [Affine2::IDENTITY, rotate, rotate * rotate, rotate * rotate * rotate];
    for cell in [IVec2::new(30, 31), IVec2::new(31, 31), IVec2::new(29, 31), IVec2::new(50, 31), IVec2::new(49, 31)] {
        for (k, t) in transforms.iter().enumerate() {
            let scene = scene
                .iter()
                .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                .collect::<PixelScene>();
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let r = get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(
                    &mut HashMap::new(),
                    &mut HashMap::new(),
                    frustum,
                    &scene,
                );
                println!("cell {cell} k={k} i={i:2}: {:7.3} {:?}", r.at_focus(frustum), r);
            }
        }
    }
}

// Finds the frustums under the dark cell's negative root whose radiance is negative although their children's isn't.
#[test]
fn ext_black_spot_origin() {
    use std::collections::HashMap;

    use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};
    use twilight::radiance::Radiance;
    use twilight::render::ext::get_radiance_recursive_ext;

    let scene = offline_scene();
    let root = Frustum::Cone(ConeFrustum { focus: IVec2::new(30, 31), size: 1, index: 0 });
    let mut cache = HashMap::new();
    get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(&mut HashMap::new(), &mut cache, root, &scene);
    let negative = |r: &Inverse2| r.samples.iter().any(|&s| s < -1e-6);
    let mut entries: Vec<_> = cache.iter().filter(|(_, r)| negative(r)).collect();
    entries.sort_by_key(|(f, _)| f.size());
    for (f, r) in entries {
        let children: Vec<Frustum> = if f.parity() == 0 {
            merge_even(*f).into_iter().map(|(c, _)| c).collect()
        } else {
            merge_odd(*f, false, true).into_iter().chain(merge_odd(*f, true, true)).map(|(c, _)| c).collect()
        };
        let child_negative = children.iter().any(|c| cache.get(c).is_some_and(negative));
        println!("{:?} {:?} at_focus={:.3} child_negative={child_negative}", f, r.samples, r.at_focus(*f));
    }
}

// Splits the first negative frustum's short and long merges into their near and far parts.
#[test]
fn ext_black_spot_parts() {
    for (focus, size, index) in [((30, 31), 2, 0), ((28, 31), 4, 0)] {
        black_spot_parts(IVec2::new(focus.0, focus.1), size, index);
    }
}

fn black_spot_parts(focus: IVec2, size: i32, index: i32) {
    use std::collections::HashMap;

    use twilight::extension::{ExtensionType, LineExtension, get_extension, ViewDirection};
    use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_odd};
    use twilight::radiance::{Radiance, ResampleRadiance};
    use twilight::render::ext::get_radiance_recursive_ext;

    let scene = offline_scene();
    let cone = ConeFrustum { focus, size, index };
    let frustum = Frustum::Cone(cone);
    println!("== {cone:?} parity {}", frustum.parity());
    let mut ext_cache = HashMap::new();
    let mut cache = HashMap::new();
    let start = cone.focus + cone.lower_dir();

    let near_ext = LineExtension { start, end: start + cone.lower_dir() + cone.upper_dir() };
    let near = get_extension::<DirectionalPosition>(&mut ext_cache, near_ext.into(), &scene, ViewDirection::Up);
    let near_i = Inverse2::resample(near.radiance, near_ext.as_frustum(), frustum);
    println!("short near {:?} -> {:?} ({:?}) at_focus={:.3}", near_ext, near.radiance, near_i.samples, near_i.at_focus(frustum));
    println!("  L read at focus: {:.3}", near.radiance.read(near_ext.as_frustum(), frustum.focus()));
    for (f, _) in merge_odd(frustum, false, true) {
        let r = get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(&mut ext_cache, &mut cache, f, &scene);
        let rp = r.reproject(f, frustum);
        println!("short far {:?} {:?} -> {:?} at_focus={:.3}", f, r.samples, rp.samples, rp.at_focus(frustum));
    }
    for (f, _) in merge_odd(frustum, true, true) {
        let r = get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(&mut ext_cache, &mut cache, f, &scene);
        let rp = r.reproject(f, frustum);
        println!("long far {:?} {:?} -> {:?} at_focus={:.3}", f, r.samples, rp.samples, rp.at_focus(frustum));
    }
}

// Lists the cached frustums whose Inverse2 samples imply an emitter (the column where 1 / value reaches zero) before
// their far region starts, which no real emitter behind the near part can cause.
#[test]
fn ext_black_spot_poles() {
    use std::collections::HashMap;

    use twilight::frustum::{ConeFrustum, Frustum, FrustumType};
    use twilight::render::ext::get_radiance_recursive_ext;

    let scene = offline_scene();
    let root = Frustum::Cone(ConeFrustum { focus: IVec2::new(30, 31), size: 1, index: 0 });
    let mut cache = HashMap::new();
    get_radiance_recursive_ext::<DirectionalPosition, Inverse2>(&mut HashMap::new(), &mut cache, root, &scene);
    let range = 1.0 / (2.0 - std::f32::consts::FRAC_1_SQRT_2);
    let mut entries: Vec<_> = cache.iter().collect();
    entries.sort_by_key(|(f, _)| (f.size(), f.focus().x as i32, f.focus().y as i32));
    for (f, r) in entries {
        let [f0, f1] = r.samples;
        if f0 <= 0.0 || f1 <= 0.0 {
            if f0 != 0.0 || f1 != 0.0 {
                println!("{f:?} nonpositive {:?}", r.samples);
            }
            continue;
        }
        let x = range * f.size() as f32 * std::f32::consts::FRAC_1_SQRT_2;
        // 1 / value is linear in the column c: 1/f1 at -x, 1/f0 at x
        let pole = if f0 > f1 { -x + 2.0 * x * (1.0 / f1) / (1.0 / f1 - 1.0 / f0) } else { f32::INFINITY };
        if pole < f.size() as f32 {
            println!("{f:?} {:?} pole at column {pole:.2} (far starts at {})", r.samples, f.size());
        }
    }
}
