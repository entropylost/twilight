use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
use std::str::FromStr;

use glam::{Affine2, IVec2, Vec2};
use log::debug;

use crate::frustum::{ConeFrustum, Frustum, FrustumType, LinearFrustum, merge_even, merge_odd};
use crate::radiance::{self, Constant, Radiance};
use crate::range2d;
use crate::scene::{RayBundle, Scene};
use crate::shape::LineSegment;

pub mod ext;
pub mod pt;

macro_rules! gen_radiance {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum RadianceKind {
            $($name),*
        }

        impl RadianceKind {
            pub const ALL: &[Self] = &[$(Self::$name),*];

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name)),*
                }
            }

            pub fn render(self, scene: &Scene, settings: RenderSettings) -> HashMap<IVec2, f32> {
                match self {
                    $(Self::$name => render::<radiance::$name>(scene, settings)),*
                }
            }
        }
    };
}

gen_radiance! {
    Constant1d,
    Constant,
    Quadratic,
    QuadraticProjected,
    LinearProjected,
    Gradient,
    Hessian,
    AveragedLinear,
    AverageDepth,
    Inverse2,
}

#[allow(clippy::derivable_impls)]
impl Default for RadianceKind {
    fn default() -> Self {
        Self::QuadraticProjected
    }
}

impl FromStr for RadianceKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.iter().copied().find(|kind| kind.name() == s).ok_or_else(|| format!("Unknown transmission: {s}"))
    }
}

pub const DEFAULT_FRUSTUM_START: f32 = 2.0;

// Nudges the scene off the probe grid so that geometry never lies exactly on a frustum boundary.
const DISAMBIGUATION_OFFSET: Vec2 = Vec2::new(0.0013, 0.0017);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    Short,
    Long,
    Both,
}

impl MergeMode {
    pub const ALL: &[Self] = &[Self::Short, Self::Long, Self::Both];

    pub fn name(self) -> &'static str {
        match self {
            Self::Short => "Short",
            Self::Long => "Long",
            Self::Both => "Both",
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for MergeMode {
    fn default() -> Self {
        Self::Both
    }
}

impl FromStr for MergeMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL.iter().copied().find(|mode| mode.name() == s).ok_or_else(|| format!("Unknown merge mode: {s}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    pub frustum_start: f32,
    pub skip_linear: bool,
    pub pretrace: bool,
    pub merge_mode: MergeMode,
}

fn get_radiance_recursive<I: Radiance>(
    cache: &mut HashMap<Frustum, I>,
    frustum: Frustum,
    scene: &Scene,
    settings: RenderSettings,
) -> I {
    if let Some(&cached) = cache.get(&frustum) {
        return cached;
    }

    let RenderSettings { frustum_start, skip_linear, pretrace, merge_mode } = settings;

    if frustum.size() > 128 {
        debug!("frustum too large: {:?}", frustum);
        return I::default();
    }
    if scene.emitters_within(&frustum.polygon(frustum_start, 128.0 / frustum.size() as f32)).count() == 0 {
        return I::default();
    }

    let transmittance_start = if pretrace { 1.0 } else { frustum_start };

    let mut contribution = |(f, w): (Frustum, f32)| {
        let near = frustum.at(transmittance_start);
        let next_near = f.at(transmittance_start);
        let projected_near = LineSegment::new(
            near.project_along(next_near.start, frustum.ray_dir(next_near.start)).unwrap(),
            near.project_along(next_near.end, frustum.ray_dir(next_near.end)).unwrap(),
        );
        let bundle = RayBundle { start: projected_near, end: next_near };
        let transmittance = scene.trace_bundle(bundle);
        let next_radiance = get_radiance_recursive(cache, f, scene, settings);
        (next_radiance.reproject(f, frustum) * transmittance) * w
    };
    let mut sum = |merged: Vec<(Frustum, f32)>, next_start: f32| {
        merged.into_iter().map(&mut contribution).reduce(|a, b| a + b).unwrap()
            + scene.emission_within(
                frustum,
                &frustum.polygon(frustum_start, next_start),
                frustum.bundle(transmittance_start, next_start),
            )
    };
    let result = if frustum.parity() == 0 {
        sum(merge_even(frustum), frustum_start * 2.0)
    } else {
        let near_start = frustum_start * 2.0 - 1.0;
        let far_start = frustum_start * 2.0 + 1.0;
        // If not using linear rays, also have to subtract the double-contribution between the two starts.
        let overlap_radiance = if skip_linear {
            let overlap = LinearFrustum { focus: frustum.focus(), size: frustum.size(), index: frustum.dir().y };
            scene.emission_within(
                frustum,
                &overlap.polygon(near_start, far_start),
                overlap.bundle(transmittance_start, far_start),
            )
        } else {
            I::default()
        };
        match merge_mode {
            MergeMode::Both => {
                sum(merge_odd(frustum, false, skip_linear), near_start) * 0.5
                    + overlap_radiance * -0.5
                    + sum(merge_odd(frustum, true, skip_linear), far_start) * 0.5
            }
            MergeMode::Short => sum(merge_odd(frustum, false, skip_linear), near_start),
            MergeMode::Long => sum(merge_odd(frustum, true, skip_linear), far_start),
        }
    };

    cache.insert(frustum, result);
    result
}

// Returns the total radiance arriving at each cell in [1, 128)^2.
pub fn render<I: Radiance>(scene: &Scene, settings: RenderSettings) -> HashMap<IVec2, f32> {
    let scene = scene.transform(Affine2::from_translation(DISAMBIGUATION_OFFSET));
    let mut caches = HashMap::<_, HashMap<Frustum, I>>::new();

    let rotate_transform = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [
        Affine2::IDENTITY,
        rotate_transform,
        rotate_transform * rotate_transform,
        rotate_transform * rotate_transform * rotate_transform,
    ];
    let scenes = transforms.map(|t| scene.transform(t));

    let mut image = HashMap::new();
    for cell in range2d(IVec2::splat(1), IVec2::splat(128)) {
        let mut total_radiance = 0.0;
        for (k, (t, scene)) in transforms.iter().zip(&scenes).enumerate() {
            let probe = t.transform_point2(cell.as_vec2());
            assert!((probe - probe.round()).length() < 0.001, "{:?}", probe);
            let probe = probe.round();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe.as_ivec2(), size: 1, index: i });
                let bundle =
                    RayBundle { start: LineSegment::new(probe, probe), end: frustum.at(settings.frustum_start) };
                let emission = scene.emission_within::<Constant>(
                    frustum,
                    &RayBundle { start: LineSegment::new(probe, probe), end: frustum.at(settings.frustum_start) }
                        .as_polygon(),
                    bundle,
                );
                let transmittance_start = if settings.pretrace { 1.0 } else { settings.frustum_start };
                let transmission = scene.trace_bundle(RayBundle {
                    start: LineSegment::new(probe, probe),
                    end: frustum.at(transmittance_start),
                });

                let radiance = get_radiance_recursive(caches.entry(k).or_default(), frustum, scene, settings);
                total_radiance += radiance.at_focus(frustum) * transmission + emission.value;
            }
        }
        image.insert(cell, total_radiance);
    }
    image
}
