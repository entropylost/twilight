use std::f32::consts::{FRAC_1_SQRT_2, SQRT_2};
use std::ops::{Add, Mul};

use glam::{Mat2, Vec2};

use crate::frustum::{Frustum, FrustumType};

// The direction the ray that is viewing the object is facing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewDir {
    Left,
    Right,
    Down,
    Up,
}
impl ViewDir {
    pub fn as_vec2(&self) -> Vec2 {
        match self {
            Self::Left => -Vec2::X,
            Self::Right => Vec2::X,
            Self::Down => -Vec2::Y,
            Self::Up => Vec2::Y,
        }
    }
    pub fn is_vertical(&self) -> bool {
        matches!(self, Self::Down | Self::Up)
    }
    pub fn is_horizontal(&self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

pub trait ResampleRadiance<L: Radiance>: Radiance {
    fn resample(original: L, source: Frustum, target: Frustum) -> Self;
}

pub trait Radiance: Copy + Add<Output = Self> + Mul<f32, Output = Self> + Default {
    fn read(self, source: Frustum, target: Vec2) -> f32;
    fn at_focus(self, frustum: Frustum) -> f32 {
        self.read(frustum, frustum.focus())
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self;
    #[allow(unused_variables)]
    fn new_directional(frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        Self::new(frustum, emitter)
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self;
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Constant1d {
    pub value: f32,
}
impl Radiance for Constant1d {
    fn read(self, _source: Frustum, _target: Vec2) -> f32 {
        self.value
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self { value: 1.0 / (emitter.x - frustum.focus().x).abs() }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl Add for Constant1d {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value }
    }
}
impl Mul<f32> for Constant1d {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Constant {
    pub value: f32,
}
impl<L: Radiance> ResampleRadiance<L> for Constant {
    fn resample(original: L, source: Frustum, target: Frustum) -> Self {
        Self { value: original.read(source, target.focus()) }
    }
}
impl Radiance for Constant {
    fn read(self, _source: Frustum, _target: Vec2) -> f32 {
        self.value
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self { value: 1.0 / (emitter - frustum.focus()).length() }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl Add for Constant {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value }
    }
}
impl Mul<f32> for Constant {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Quadratic {
    pub value: f32,
    // along frustum direction; we don't need moments along perpendicular direction due to small cone size
    // currently just using both derivatives at the focus point
    pub moments: [f32; 2],
}
impl Radiance for Quadratic {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        let source_focus = source.focus();
        let dist = (target - source_focus).dot(source.dir().normalize());
        self.value + self.moments[0] * dist + 0.5 * self.moments[1] * dist * dist
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        let focus = frustum.focus();
        let dir = frustum.dir();
        let dist = (emitter - focus).dot(dir.normalize());
        Self { value: 1.0 / dist, moments: [1.0 / (dist * dist), 2.0 / (dist * dist * dist)] }
    }
    // TODO: This doesn't rotate properly.
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let source_focus = source.focus();
        let target_focus = target.focus();
        let dist = (target_focus - source_focus).dot(source.dir().normalize());
        // let ratio = target.dir().normalize().dot(source.dir().normalize());
        Self {
            value: self.value + self.moments[0] * dist + 0.5 * self.moments[1] * dist * dist,
            moments: [self.moments[0] + self.moments[1] * dist, self.moments[1]],
        }
    }
}
impl Add for Quadratic {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            value: self.value + other.value,
            moments: [self.moments[0] + other.moments[0], self.moments[1] + other.moments[1]],
        }
    }
}
impl Mul<f32> for Quadratic {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, moments: [self.moments[0] * rhs, self.moments[1] * rhs] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct QuadraticProjected {
    pub value: f32,
    // along the x-axis
    pub moments: [f32; 2],
}
impl Radiance for QuadraticProjected {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        let source_focus = source.focus();
        let dist = (target - source_focus).x;
        self.value + self.moments[0] * dist + 0.5 * self.moments[1] * dist * dist
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        let focus = frustum.focus();
        let dist = (emitter - focus).length();
        let projected_dist = (emitter - focus).x;
        let multiplier = projected_dist / dist;
        Self {
            value: multiplier / projected_dist,
            moments: [
                multiplier / (projected_dist * projected_dist),
                2.0 * multiplier / (projected_dist * projected_dist * projected_dist),
            ],
        }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let source_focus = source.focus();
        let target_focus = target.focus();
        let dist = (target_focus - source_focus).x;
        Self {
            value: self.value + self.moments[0] * dist + 0.5 * self.moments[1] * dist * dist,
            moments: [self.moments[0] + self.moments[1] * dist, self.moments[1]],
        }
    }
}
impl Add for QuadraticProjected {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            value: self.value + other.value,
            moments: [self.moments[0] + other.moments[0], self.moments[1] + other.moments[1]],
        }
    }
}
impl Mul<f32> for QuadraticProjected {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, moments: [self.moments[0] * rhs, self.moments[1] * rhs] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LinearProjected {
    // approximate, at focus (also doing projected)
    pub value: f32,
    pub moment: f32,
}
impl Radiance for LinearProjected {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        let source_focus = source.focus();
        let dist = (target - source_focus).x;
        self.value + self.moment * dist
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        let focus = frustum.focus();
        let dist = (emitter - focus).length();
        let projected_dist = (emitter - focus).x; // a
        let multiplier = projected_dist / dist;

        // target function: multiplier / projected_dist wrt. projected_dist
        Self { value: multiplier / projected_dist, moment: multiplier / (projected_dist * projected_dist) }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let source_focus = source.focus();
        let target_focus = target.focus();
        let dist = (target_focus - source_focus).x;
        Self { value: self.value + self.moment * dist, moment: self.moment }
    }
}
impl Add for LinearProjected {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value, moment: self.moment + other.moment }
    }
}
impl Mul<f32> for LinearProjected {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, moment: self.moment * rhs }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Gradient {
    pub value: f32,
    // spatial derivatives in both directions
    pub gradient: Vec2,
}
impl Radiance for Gradient {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        let source_focus = source.focus();
        let delta = target - source_focus;
        self.value + self.gradient.dot(delta)
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        let focus = frustum.focus();
        let dist = (emitter - focus).length();
        Self { value: 1.0 / dist, gradient: (emitter - focus).normalize() * (1.0 / (dist * dist)) }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let source_focus = source.focus();
        let target_focus = target.focus();
        Self { value: self.value + self.gradient.dot(target_focus - source_focus), gradient: self.gradient }
    }
}
impl Add for Gradient {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value, gradient: self.gradient + other.gradient }
    }
}
impl Mul<f32> for Gradient {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, gradient: self.gradient * rhs }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hessian {
    pub value: f32,
    // spatial derivatives in both directions
    pub gradient: Vec2,
    // symmetric second spatial derivatives
    pub hessian: Mat2,
}
// Mat2::default() is the identity, so Default can't be derived.
impl Default for Hessian {
    fn default() -> Self {
        Self { value: 0.0, gradient: Vec2::ZERO, hessian: Mat2::ZERO }
    }
}
impl Radiance for Hessian {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        let source_focus = source.focus();
        let delta = target - source_focus;
        let hessian_delta = self.hessian * delta;
        self.value + self.gradient.dot(delta) + 0.5 * delta.dot(hessian_delta)
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        let focus = frustum.focus();
        let dist = (emitter - focus).length();
        let dir = (emitter - focus).normalize();
        let outer = Mat2::from_cols(dir * dir.x, dir * dir.y);
        Self {
            value: 1.0 / dist,
            gradient: dir * (1.0 / (dist * dist)),
            hessian: (outer * 3.0 - Mat2::IDENTITY) * (1.0 / (dist * dist * dist)),
        }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let source_focus = source.focus();
        let target_focus = target.focus();
        let delta = target_focus - source_focus;
        let hessian_delta = self.hessian * delta;
        Self {
            value: self.value + self.gradient.dot(delta) + 0.5 * delta.dot(hessian_delta),
            gradient: self.gradient + hessian_delta,
            hessian: self.hessian,
        }
    }
}
impl Add for Hessian {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            value: self.value + other.value,
            gradient: self.gradient + other.gradient,
            hessian: self.hessian + other.hessian,
        }
    }
}
impl Mul<f32> for Hessian {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, gradient: self.gradient * rhs, hessian: self.hessian * rhs }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AveragedLinear {
    // approximate, at focus (also doing projected)
    pub value: f32,
    pub slope: f32,
}
impl Radiance for AveragedLinear {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        let source_focus = source.focus();
        let dist = (target - source_focus).x;
        self.value + self.slope * dist
    }
    // least squares optimal between -c and c.
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        let focus = frustum.focus();
        let dist = (emitter - focus).length();
        let projected_dist = (emitter - focus).x; // a
        let multiplier = projected_dist / dist;

        let average_range = frustum.size() as f32 * 0.5; // c

        let at = (average_range / projected_dist).atanh();

        // target function: multiplier / projected_dist wrt. projected_dist
        Self {
            value: multiplier * at / average_range,
            slope: multiplier * 3.0 * (projected_dist * at - average_range) / average_range.powi(3),
        }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let source_focus = source.focus();
        let target_focus = target.focus();
        let dist = (target_focus - source_focus).x;
        Self { value: self.value + self.slope * dist, slope: self.slope }
    }
}
impl Add for AveragedLinear {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value, slope: self.slope + other.slope }
    }
}
impl Mul<f32> for AveragedLinear {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, slope: self.slope * rhs }
    }
}

// Moment HRC's far-part carrier: the value for two readers on the frustum's centre line, at column offsets
// ±INVERSE2_RANGE * size / √2 from the focus (the Chebyshev nodes of ±INVERSE2_RANGE * size). Any other reader
// projects onto the centre line and interpolates 1 / value linearly in the projected column, since the value falls
// off about as 1 / distance. Sums and scales act on the samples, so they are exact; only reading elsewhere is
// approximate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Inverse2 {
    // the values at the forward and the backward sample
    pub samples: [f32; 2],
}
// A frustum of size s is read at its parent's samples, at most INVERSE2_RANGE * s / (2√2) columns along the parent's
// centre line from a parent focus 0 or s / 2 columns away, so up to s / 2 * (1 + INVERSE2_RANGE / √2) columns from
// its own focus. INVERSE2_RANGE = 1 / (2 - 1 / √2) makes that exactly INVERSE2_RANGE * s, so reads stay within the
// range (up to a few percent beyond it, since the child's centre line is not quite the parent's).
const INVERSE2_RANGE: f32 = 1.0 / (2.0 - FRAC_1_SQRT_2);
// samples are interpolated harmonically only when both are above this fraction of the larger
const INVERSE2_SAMPLE_RATIO_MIN: f32 = 1e-12;
// The far part starts a size ahead of the focus, at forward weight w = 1 / 2 + √2 / (2 * INVERSE2_RANGE) = √2. No
// emitter is nearer, so extrapolated 1 / value may reach zero no earlier than there (nor nearer than as far behind the
// backward sample). Steeper samples, e.g. from faces seen at a grazing angle, which fall off as 1 / distance², would
// otherwise put a pole within the read range and give huge or negative values.
const INVERSE2_POLE_MIN: f32 = SQRT_2;
impl Inverse2 {
    // the samples' column offsets from the focus, along the centre line
    fn sample_columns(frustum: Frustum) -> [f32; 2] {
        let x = INVERSE2_RANGE * frustum.size() as f32 * FRAC_1_SQRT_2;
        [x, -x]
    }
    fn sample_points(frustum: Frustum) -> [Vec2; 2] {
        // dir's x component is the size, so dir / size is one column along the centre line
        let step = frustum.dir() / frustum.size() as f32;
        Self::sample_columns(frustum).map(|x| frustum.focus() + step * x)
    }
    // w is the forward sample's weight: 1 / value interpolated linearly, or the value itself when a sample is near zero
    fn interpolate(self, w: f32) -> f32 {
        let [f0, f1] = self.samples;
        if f0.min(f1) > INVERSE2_SAMPLE_RATIO_MIN * f0.max(f1).max(1e-30) {
            let reciprocal = w / f0 + (1.0 - w) / f1;
            let reciprocal = if w > 1.0 {
                reciprocal.max((INVERSE2_POLE_MIN - w) / (INVERSE2_POLE_MIN - 1.0) / f0)
            } else if w < 0.0 {
                reciprocal.max((INVERSE2_POLE_MIN - 1.0 + w) / (INVERSE2_POLE_MIN - 1.0) / f1)
            } else {
                reciprocal
            };
            1.0 / reciprocal
        } else {
            w * f0 + (1.0 - w) * f1
        }
    }
}
impl<L: Radiance> ResampleRadiance<L> for Inverse2 {
    fn resample(original: L, source: Frustum, target: Frustum) -> Self {
        Self { samples: Self::sample_points(target).map(|p| original.read(source, p)) }
    }
}
impl Radiance for Inverse2 {
    // the value for a reader at target, from its projection onto the centre line
    fn read(self, frustum: Frustum, target: Vec2) -> f32 {
        let dir = frustum.dir();
        // the projection's column offset from the focus
        let x = (target - frustum.focus()).dot(dir) / dir.length_squared() * dir.x;
        let [x0, x1] = Self::sample_columns(frustum);
        self.interpolate((x - x1) / (x0 - x1))
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self { samples: Self::sample_points(frustum).map(|p| 1.0 / (emitter - p).length()) }
    }
    // resampled at the target's sample points
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        Self { samples: Self::sample_points(target).map(|p| self.read(source, p)) }
    }
}
impl Add for Inverse2 {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { samples: [self.samples[0] + other.samples[0], self.samples[1] + other.samples[1]] }
    }
}
impl Mul<f32> for Inverse2 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { samples: self.samples.map(|v| v * rhs) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AverageDepth {
    pub value: f32,
    pub weighted_depth: f32,
}
impl Radiance for AverageDepth {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        if self.value == 0.0 {
            return 0.0;
        }
        let depth = self.weighted_depth / self.value;
        let emitter_pos = source.focus() + depth * source.dir() / source.dir().x;
        self.value / (emitter_pos - target).length()
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self { value: 1.0, weighted_depth: (emitter - frustum.focus()).x }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        let source_focus = source.focus();
        let target_focus = target.focus();
        let dist = (target_focus - source_focus).x;
        // depth is measured from the focus, so moving the focus forward brings the emitter closer
        Self { value: self.value, weighted_depth: self.weighted_depth - dist * self.value }
    }
}
impl Add for AverageDepth {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value, weighted_depth: self.weighted_depth + other.weighted_depth }
    }
}
impl Mul<f32> for AverageDepth {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, weighted_depth: self.weighted_depth * rhs }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AveragePosition {
    pub value: f32,
    pub weighted_offset: Vec2,
}
impl Radiance for AveragePosition {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        if self.value == 0.0 {
            return 0.0;
        }
        let emitter_pos = source.focus() + self.weighted_offset / self.value;
        self.value / (emitter_pos - target).length()
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self { value: 1.0, weighted_offset: emitter - frustum.focus() }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        // the offset is measured from the focus, so moving the focus forward brings the emitter closer
        let delta = target.focus() - source.focus();
        Self { value: self.value, weighted_offset: self.weighted_offset - delta * self.value }
    }
}
impl ResampleRadiance<AveragePosition> for AveragePosition {
    fn resample(original: AveragePosition, source: Frustum, target: Frustum) -> Self {
        original.reproject(source, target)
    }
}
impl Add for AveragePosition {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value, weighted_offset: self.weighted_offset + other.weighted_offset }
    }
}
impl Mul<f32> for AveragePosition {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs, weighted_offset: self.weighted_offset * rhs }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DirectionalPosition {
    pub directional_emission: Vec2,
    pub weighted_offset: Vec2,
}
impl DirectionalPosition {
    // needs to be linear. Could potentially square it, but not sure.
    fn total_emission(&self) -> f32 {
        self.directional_emission.element_sum()
    }
}
impl Radiance for DirectionalPosition {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        if self.total_emission() == 0.0 {
            return 0.0;
        }
        let emitter_pos = source.focus() + self.weighted_offset / self.total_emission();
        let view_angle = (emitter_pos - target).normalize();

        // TODO: Could be more accurate.
        (self.directional_emission.x * view_angle.x.abs() + self.directional_emission.y * view_angle.y.abs())
            / (emitter_pos - target).length()
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self { directional_emission: Vec2::splat(0.5), weighted_offset: emitter - frustum.focus() }
    }
    fn new_directional(frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        let directional_radiance = if view_dir.is_horizontal() { Vec2::new(1.0, 0.0) } else { Vec2::new(0.0, 1.0) };
        Self { directional_emission: directional_radiance, weighted_offset: emitter - frustum.focus() }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        // the offset is measured from the focus, so moving the focus forward brings the emitter closer
        let delta = target.focus() - source.focus();
        Self {
            directional_emission: self.directional_emission,
            weighted_offset: self.weighted_offset - delta * self.total_emission(),
        }
    }
}
impl ResampleRadiance<DirectionalPosition> for DirectionalPosition {
    fn resample(original: DirectionalPosition, source: Frustum, target: Frustum) -> Self {
        original.reproject(source, target)
    }
}
impl Add for DirectionalPosition {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            directional_emission: self.directional_emission + other.directional_emission,
            weighted_offset: self.weighted_offset + other.weighted_offset,
        }
    }
}
impl Mul<f32> for DirectionalPosition {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { directional_emission: self.directional_emission * rhs, weighted_offset: self.weighted_offset * rhs }
    }
}
