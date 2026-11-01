use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::str::FromStr;

use glam::{Affine2, IVec2, Vec2};
use log::debug;

use crate::frustrum::{ConeFrustrum, Frustrum, LinearFrustrum, merge_even, merge_odd};
use crate::range2d;
use crate::scene::{RayBundle, Scene};
use crate::shape::LineSegment;
use crate::transmission::{self, Constant, Transmission};

macro_rules! transmissions {
    ($($name:ident),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum TransmissionKind {
            $($name),*
        }

        impl TransmissionKind {
            pub const ALL: &[Self] = &[$(Self::$name),*];

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$name => stringify!($name)),*
                }
            }

            pub fn render(self, scene: &Scene, settings: RenderSettings) -> HashMap<IVec2, f32> {
                match self {
                    $(Self::$name => render::<transmission::$name>(scene, settings)),*
                }
            }
        }
    };
}

transmissions! {
    Constant1d,
    Constant,
    Quadratic,
    QuadraticProjected,
    LinearProjected,
    Gradient,
    Hessian,
    AveragedLinear,
}

#[allow(clippy::derivable_impls)]
impl Default for TransmissionKind {
    fn default() -> Self {
        Self::QuadraticProjected
    }
}

impl FromStr for TransmissionKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.name() == s)
            .ok_or_else(|| format!("Unknown transmission: {s}"))
    }
}

pub const DEFAULT_FRUSTRUM_START: f32 = 2.0;

// Nudges the scene off the probe grid so that geometry never lies exactly on a frustrum boundary.
const DISAMBIGUATION_OFFSET: Vec2 = Vec2::new(0.0013, 0.0017);

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderSettings {
    pub frustrum_start: f32,
    pub skip_linear: bool,
}

fn get_radiance_recursive<T: Transmission>(
    cache: &mut HashMap<Frustrum, T>,
    frustrum: Frustrum,
    scene: &Scene,
    settings: RenderSettings,
) -> T {
    if let Some(&cached) = cache.get(&frustrum) {
        return cached;
    }

    let RenderSettings {
        frustrum_start,
        skip_linear,
    } = settings;

    if frustrum.size() > 128 {
        debug!("Frustrum too large: {:?}", frustrum);
        return T::default();
    }
    let frustrum_poly = frustrum.polygon(frustrum_start, 128.0 / frustrum.size() as f32);
    if scene
        .emitters
        .iter()
        .all(|emitter| !frustrum_poly.contains_one_sided(emitter.position))
    {
        return T::default();
    }

    let near = frustrum.at(frustrum_start);
    let mut contribution = |(f, w): (Frustrum, f32)| {
        let next_near = f.at(frustrum_start);
        let projected_near = LineSegment::new(
            near.project_along(next_near.start, frustrum.ray_dir(next_near.start))
                .unwrap(),
            near.project_along(next_near.end, frustrum.ray_dir(next_near.end))
                .unwrap(),
        );
        let bundle = RayBundle {
            start: projected_near,
            end: next_near,
        };
        let transmittance = scene.trace_bundle(bundle);
        let next_radiance = get_radiance_recursive(cache, f, scene, settings);
        (next_radiance.reproject(f, frustrum) * transmittance) * w
    };
    let mut sum = |merged: Vec<(Frustrum, f32)>, next_start: f32| {
        let start = frustrum.at(frustrum_start);
        let end = frustrum.at(next_start);
        let bundle = RayBundle { start, end };
        merged.into_iter().map(&mut contribution).reduce(|a, b| a + b).unwrap()
            + scene.emission_within(frustrum, bundle)
    };
    let result = if frustrum.parity() == 0 {
        sum(merge_even(frustrum), frustrum_start * 2.0)
    } else {
        let near_start = frustrum_start * 2.0 - 1.0;
        let far_start = frustrum_start * 2.0 + 1.0;
        // If not using linear rays, also have to subtract the double-contribution between the two starts.
        let overlap_radiance = if skip_linear {
            let overlap = LinearFrustrum {
                focus: frustrum.focus(),
                size: frustrum.size(),
                index: frustrum.dir().y,
            };
            let bundle = RayBundle {
                start: overlap.at(near_start),
                end: overlap.at(far_start),
            };
            scene.emission_within(frustrum, bundle)
        } else {
            T::default()
        };
        sum(merge_odd(frustrum, false, skip_linear), near_start) * 0.5
            + overlap_radiance * -0.5
            + sum(merge_odd(frustrum, true, skip_linear), far_start) * 0.5
    };

    cache.insert(frustrum, result);
    result
}

// Returns the total radiance arriving at each cell in [1, 128)^2.
pub fn render<T: Transmission>(scene: &Scene, settings: RenderSettings) -> HashMap<IVec2, f32> {
    let scene = scene.transform(Affine2::from_translation(DISAMBIGUATION_OFFSET));
    let mut caches = HashMap::<_, HashMap<Frustrum, T>>::new();

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
                let frustrum = Frustrum::Cone(ConeFrustrum {
                    focus: probe.as_ivec2(),
                    size: 1,
                    index: i,
                });
                let bundle = RayBundle {
                    start: LineSegment::new(probe, probe),
                    end: frustrum.at(settings.frustrum_start),
                };
                let emission = scene.emission_within::<Constant>(frustrum, bundle);
                let transmission = scene.trace_bundle(bundle);

                let radiance = get_radiance_recursive(caches.entry(k).or_default(), frustrum, scene, settings);
                total_radiance += radiance.value() * transmission + emission.value;
            }
        }
        image.insert(cell, total_radiance);
    }
    image
}
