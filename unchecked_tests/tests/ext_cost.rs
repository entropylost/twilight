// Run cost of the extension renderer: the whole pixel-shadow render (extensions + merge, as `render_ext` does it, without
// the per-probe read). Prints the number of distinct extensions / frustums cached and the best time of RUNS (default 3)
// runs, for the std hasher and for the renderer's own `CacheHasher`.
//   HASHER=cache cargo test --release -p unchecked_tests --test ext_cost -- --nocapture
// Set HASHER=std or HASHER=cache and time each in a process of its own: whichever runs second in one process is about
// 1.5 s slower, because the first leaves more than a GB of freed tables behind.
// To compare two versions of the algorithm like for like, run this file in both trees on the same machine.

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::time::{Duration, Instant};

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::{Extension, Fluence, PixelScene};
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{DirectionalPosition, Inverse2};
use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext};

type Caches<S> = (HashMap<Extension, Fluence<DirectionalPosition>, S>, HashMap<Frustum, Inverse2, S>);

fn run<S: BuildHasher + Default>(name: &str) {
    let mut scene = PixelScene::new();
    scene.insert(IVec2::new(36, 32), 5.0);
    for x in 0..50 {
        scene.insert(IVec2::new(x, 60), 0.0);
    }
    let rot = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(std::f32::consts::FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [Affine2::IDENTITY, rot, rot * rot, rot * rot * rot];
    let scenes = transforms.map(|t| {
        scene.iter().map(|(k, v)| (t.transform_point2(k.as_vec2() + 0.5).floor().as_ivec2(), *v)).collect::<PixelScene>()
    });
    let runs: usize = std::env::var("RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let mut best = Duration::MAX;
    let mut counts = (0, 0, 0);
    for _ in 0..runs {
        let start = Instant::now();
        let mut caches = HashMap::<(usize, i32), Caches<S>>::new();
        for x in 1..128 {
            for y in 1..128 {
                for (k, (t, scene)) in transforms.iter().zip(&scenes).enumerate() {
                    let probe = t.transform_point2(Vec2::new(x as f32, y as f32)).round().as_ivec2();
                    for i in [-1, 0] {
                        let (ext_cache, cache) = caches.entry((k, i)).or_default();
                        get_radiance_recursive_ext(
                            ext_cache,
                            cache,
                            Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i }),
                            scene,
                            RenderExtSettings { simplify_occlusion: false },
                        );
                    }
                }
            }
        }
        best = best.min(start.elapsed());
        let lines = caches.values().flat_map(|c| c.0.keys()).filter(|e| matches!(e, Extension::Line(_))).count();
        let extensions: usize = caches.values().map(|c| c.0.len()).sum();
        counts = (lines, extensions - lines, caches.values().map(|c| c.1.len()).sum::<usize>());
    }
    let (lines, cones, frustums) = counts;
    println!(
        "{name:5} hasher: extensions cached {} (lines {lines}, cones {cones}), frustums cached {frustums}, best time {best:?}",
        lines + cones
    );
}

#[test]
fn cost() {
    let only = std::env::var("HASHER").ok();
    if only.as_deref() != Some("cache") {
        run::<std::hash::RandomState>("std");
    }
    if only.as_deref() != Some("std") {
        run::<std::hash::BuildHasherDefault<twilight::extension::CacheHasher>>("cache");
    }
}
