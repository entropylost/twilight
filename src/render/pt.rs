//! Reference path tracer for a `PixelScene`: from every probe, march a fan of evenly spaced rays through the pixel grid
//! with a DDA and sum the emittance of the first pixel each one hits. The fan is rotated by a random angle per probe,
//! which turns the aliasing of a fixed set of directions into noise.
//!
//! Light is only gathered directly, so a ray can only pick up emission if it heads into the bounds of the emissive
//! pixels. Next event estimation therefore spends every ray inside the angle those bounds subtend, and stops each one
//! where it leaves them; occluders outside the bounds can't block anything that reaches an emitter. The bounds of the
//! whole scene let each ray skip the empty space before it reaches any pixel.

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
    // Number of rays traced from each probe, spread evenly over the angle the emissive bounds subtend.
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
    fn of(mut pixels: impl Iterator<Item = IVec2>) -> Option<Self> {
        let first = pixels.next()?;
        let (min, max) = pixels.fold((first, first), |(min, max), p| (min.min(p), max.max(p)));
        Some(Self { min: min.as_vec2(), max: (max + 1).as_vec2() })
    }

    // The bounds of the pixels that emit light.
    fn emissive(scene: &PixelScene) -> Option<Self> {
        Self::of(scene.iter().filter(|&(_, &emittance)| emittance > 0.0).map(|(&p, _)| p))
    }

    // The interval of directions, as (start angle, width), in which a ray from the point enters the bounds. A point
    // strictly inside sees them in every direction.
    fn subtended(&self, point: Vec2) -> (f32, f32) {
        if point.cmpgt(self.min).all() && point.cmplt(self.max).all() {
            return (0.0, TAU);
        }
        // Outside a convex box, every corner lies within a half-turn of the direction to its center, so angles
        // relative to that direction don't wrap.
        let axis = (self.min + self.max) * 0.5 - point;
        let corners = [self.min, Vec2::new(self.max.x, self.min.y), self.max, Vec2::new(self.min.x, self.max.y)];
        let (low, high) = corners.into_iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), corner| {
            let to = corner - point;
            let angle = axis.perp_dot(to).atan2(axis.dot(to));
            (low.min(angle), high.max(angle))
        });
        (axis.to_angle() + low, high - low)
    }

    // The distances along the ray at which it enters and leaves the bounds, or None if it misses them.
    fn crossing(&self, origin: Vec2, dir: Vec2) -> Option<(f32, f32)> {
        let inv = dir.recip();
        let t0 = (self.min - origin) * inv;
        let t1 = (self.max - origin) * inv;
        let near = t0.min(t1).max_element();
        let far = t0.max(t1).min_element();
        // written so that NaN (a ray along a bounds edge) counts as a miss
        (near <= far && far >= 0.0).then_some((near, far))
    }
}

#[derive(Debug, Clone, Copy)]
struct SceneBounds {
    // every pixel
    all: Bounds,
    // the pixels that emit light; inside `all`
    emissive: Bounds,
}

impl SceneBounds {
    fn of(scene: &PixelScene) -> Option<Self> {
        Some(Self { all: Bounds::of(scene.keys().copied())?, emissive: Bounds::emissive(scene)? })
    }
}

// Marches a ray through the pixel grid and returns the emittance of the first pixel it enters, or None if it leaves
// the emissive bounds without hitting anything. A ray starting on a pixel boundary starts in the pixel it heads into.
fn trace(scene: &PixelScene, bounds: SceneBounds, origin: Vec2, dir: Vec2) -> Option<f32> {
    // Nothing lies outside the scene bounds, so the march can start where the ray enters them. Rounding may put the
    // new origin just outside, in an empty pixel, which the march steps out of.
    let (t_enter, _) = bounds.all.crossing(origin, dir)?;
    let origin = origin + dir * t_enter.max(0.0);
    let (_, t_exit) = bounds.emissive.crossing(origin, dir)?;

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

// The radiance arriving at a probe, integrated over all directions. `rotation` in [0, 1) offsets the fan of rays by
// that fraction of the spacing between them.
pub fn probe_fluence(scene: &PixelScene, probe: Vec2, rotation: f32, directions: u32) -> f32 {
    let Some(bounds) = SceneBounds::of(scene) else {
        return 0.0;
    };
    probe_fluence_in(scene, bounds, probe, rotation, directions)
}

fn probe_fluence_in(scene: &PixelScene, bounds: SceneBounds, probe: Vec2, rotation: f32, directions: u32) -> f32 {
    let (start, width) = bounds.emissive.subtended(probe);
    let step = width / directions as f32;
    let total: f32 = (0..directions)
        .filter_map(|i| {
            let angle = start + (i as f32 + rotation) * step;
            trace(scene, bounds, probe, Vec2::from_angle(angle))
        })
        .sum();
    total * RADIANCE_PER_EMITTANCE * step
}

// Returns the total radiance arriving at each cell in [1, 128)^2, like `render_ext`.
pub fn render_pt(scene: &PixelScene, settings: RenderPtSettings) -> HashMap<IVec2, f32> {
    let RenderPtSettings { directions, seed } = settings;
    assert!(directions > 0, "need at least one direction");
    let bounds = SceneBounds::of(scene);
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
