//! Reference path tracer for a `PixelScene`: from every probe, march a fan of evenly spaced rays through the pixel grid
//! with a DDA and sum the emittance of the first pixel each one hits. The fan is rotated by a random angle per probe,
//! which turns the aliasing of a fixed set of directions into noise.

use std::collections::HashMap;
use std::f32::consts::TAU;

use glam::{IVec2, Vec2};
use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};

use crate::extension::PixelScene;
use crate::range2d;

// A face of emittance e gives radiance e / 2, matching `render_ext`, which weights each of the two faces a probe sees
// of a pixel by 1/2.
const RADIANCE_PER_EMITTANCE: f32 = 0.5;

#[derive(Debug, Clone, Copy)]
pub struct RenderPtSettings {
    // Number of rays traced from each probe, spread evenly over the full circle.
    pub directions: u32,
    // Seeds the per-probe rotation of the ray fan.
    pub seed: u64,
}

// Pixels occupy [p, p + 1)^2, so every pixel lies inside [min, max).
#[derive(Debug, Clone, Copy)]
struct Bounds {
    min: Vec2,
    max: Vec2,
}

impl Bounds {
    fn of(scene: &PixelScene) -> Option<Self> {
        let mut pixels = scene.keys();
        let first = *pixels.next()?;
        let (min, max) = pixels.fold((first, first), |(min, max), &p| (min.min(p), max.max(p)));
        Some(Self { min: min.as_vec2(), max: (max + 1).as_vec2() })
    }

    // The distance along the ray at which it leaves the bounds for good (negative if it never enters them).
    fn exit(&self, origin: Vec2, dir: Vec2) -> f32 {
        let inv = dir.recip();
        let t0 = (self.min - origin) * inv;
        let t1 = (self.max - origin) * inv;
        let near = t0.min(t1).max_element();
        let far = t0.max(t1).min_element();
        // written so that NaN (a ray along a bounds edge) counts as a miss
        if near <= far { far } else { -1.0 }
    }
}

// Marches a ray through the pixel grid and returns the emittance of the first pixel it enters, or None if it leaves
// the scene without hitting anything. A ray starting on a pixel boundary starts in the pixel it heads into.
fn trace(scene: &PixelScene, bounds: Bounds, origin: Vec2, dir: Vec2) -> Option<f32> {
    let t_exit = bounds.exit(origin, dir);
    if t_exit < 0.0 {
        return None;
    }

    let step = IVec2::new(if dir.x < 0.0 { -1 } else { 1 }, if dir.y < 0.0 { -1 } else { 1 });
    let floor = origin.floor();
    let mut cell = floor.as_ivec2();
    // On a boundary while heading in the negative direction, the ray is in the pixel below the boundary.
    if dir.x < 0.0 && origin.x == floor.x {
        cell.x -= 1;
    }
    if dir.y < 0.0 && origin.y == floor.y {
        cell.y -= 1;
    }

    // Distance along the ray to the next vertical / horizontal grid line, and between consecutive ones.
    let t_delta = dir.abs().recip();
    let next_boundary = cell.as_vec2() + step.max(IVec2::ZERO).as_vec2();
    let mut t_max = Vec2::new(
        if dir.x == 0.0 { f32::INFINITY } else { (next_boundary.x - origin.x) / dir.x },
        if dir.y == 0.0 { f32::INFINITY } else { (next_boundary.y - origin.y) / dir.y },
    );

    loop {
        if let Some(&emittance) = scene.get(&cell) {
            return Some(emittance);
        }
        if t_max.x < t_max.y {
            if t_max.x > t_exit {
                return None;
            }
            cell.x += step.x;
            t_max.x += t_delta.x;
        } else {
            if t_max.y > t_exit {
                return None;
            }
            cell.y += step.y;
            t_max.y += t_delta.y;
        }
    }
}

// The radiance arriving at a probe, integrated over all directions.
pub fn probe_fluence(scene: &PixelScene, probe: Vec2, rotation: f32, directions: u32) -> f32 {
    let Some(bounds) = Bounds::of(scene) else {
        return 0.0;
    };
    probe_fluence_in(scene, bounds, probe, rotation, directions)
}

fn probe_fluence_in(scene: &PixelScene, bounds: Bounds, probe: Vec2, rotation: f32, directions: u32) -> f32 {
    let step = TAU / directions as f32;
    let total: f32 = (0..directions)
        .filter_map(|i| {
            let angle = (i as f32 + rotation) * step;
            trace(scene, bounds, probe, Vec2::from_angle(angle))
        })
        .sum();
    total * RADIANCE_PER_EMITTANCE * step
}

// Returns the total radiance arriving at each cell in [1, 128)^2, like `render_ext`.
pub fn render_pt(scene: &PixelScene, settings: RenderPtSettings) -> HashMap<IVec2, f32> {
    let RenderPtSettings { directions, seed } = settings;
    assert!(directions > 0, "need at least one direction");
    let bounds = Bounds::of(scene);
    let mut rng = SmallRng::seed_from_u64(seed);
    range2d(IVec2::splat(1), IVec2::splat(128))
        .map(|cell| {
            let rotation = rng.random::<f32>();
            let value =
                bounds.map_or(0.0, |bounds| probe_fluence_in(scene, bounds, cell.as_vec2(), rotation, directions));
            (cell, value)
        })
        .collect()
}
