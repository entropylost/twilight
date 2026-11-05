use std::collections::HashMap;

use glam::{IVec2, Vec2};

use crate::{
    frustum::{ConeFrustum, Frustum, FrustumType, LinearFrustum},
    radiance::{Radiance, ResampleRadiance, ViewDir},
    shape::Polygon,
};

#[derive(Debug, Clone, Copy)]
pub struct Fluence<L: Radiance> {
    pub transmittance: f32,
    pub radiance: L,
}
impl<L: Radiance> Fluence<L> {
    pub fn map<R: Radiance>(self, f: impl FnOnce(L) -> R) -> Fluence<R> {
        Fluence { transmittance: self.transmittance, radiance: f(self.radiance) }
    }
    pub fn reproject(self, source: Frustum, target: Frustum) -> Self {
        self.map(|radiance| radiance.reproject(source, target))
    }
    pub fn resample<R: ResampleRadiance<L>>(self, source: Frustum, target: Frustum) -> Fluence<R> {
        self.map(|radiance| R::resample(radiance, source, target))
    }
    // Assume that both have the same focal point.
    pub fn over(&self, other: Fluence<L>) -> Self {
        Fluence {
            transmittance: self.transmittance * other.transmittance,
            radiance: self.radiance + other.radiance * self.transmittance,
        }
    }
    pub fn join(values: impl IntoIterator<Item = Self>) -> Self {
        let mut count = 0;
        let mut total_transmittance = 0.0;
        let mut total_radiance = L::default();
        for fluence in values {
            count += 1;
            total_transmittance += fluence.transmittance;
            total_radiance = total_radiance + fluence.radiance;
        }
        Fluence { transmittance: total_transmittance / count as f32, radiance: total_radiance }
    }
    pub fn join_weighted(values: impl IntoIterator<Item = (Self, f32)>) -> Self {
        let mut total_weight = 0.0;
        let mut total_transmittance = 0.0;
        let mut total_radiance = L::default();
        for (fluence, weight) in values {
            total_weight += weight;
            total_transmittance += fluence.transmittance * weight;
            total_radiance = total_radiance + fluence.radiance;
        }
        Fluence { transmittance: total_transmittance / total_weight, radiance: total_radiance }
    }
}

#[enum_delegate::register]
pub trait ExtensionType {
    fn size(&self) -> i32;
    fn start(&self) -> IVec2;
    fn index(&self) -> i32;
    fn as_frustum(&self) -> Frustum;
    fn polygon(&self) -> Polygon {
        self.as_frustum().polygon(1.0, 2.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[enum_delegate::implement(ExtensionType)]
pub enum Extension {
    Line(LineExtension),
    Cone(ConeExtension),
}

// end is exclusive
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineExtension {
    pub start: IVec2,
    pub end: IVec2,
}
impl ExtensionType for LineExtension {
    fn size(&self) -> i32 {
        self.delta().x
    }
    fn start(&self) -> IVec2 {
        self.start
    }
    fn as_frustum(&self) -> Frustum {
        Frustum::Linear(LinearFrustum {
            focus: (self.start - self.delta()).as_vec2() + Vec2::Y * 0.5,
            size: self.size(),
            index: self.index() as f32,
        })
    }
    fn index(&self) -> i32 {
        self.delta().y
    }
}
impl LineExtension {
    pub fn delta(&self) -> IVec2 {
        self.end - self.start
    }
    pub fn extend(&self) -> [LineExtension; 2] {
        let half_delta = self.delta() / 2;
        [
            LineExtension { start: self.start, end: (self.start + half_delta) },
            LineExtension { start: (self.end - half_delta), end: self.end },
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConeExtension {
    pub start: IVec2,
    pub end_bottom: IVec2,
}
impl ExtensionType for ConeExtension {
    fn size(&self) -> i32 {
        self.end_bottom.x - self.start.x
    }
    fn start(&self) -> IVec2 {
        self.start
    }
    fn index(&self) -> i32 {
        self.delta().y
    }
    fn as_frustum(&self) -> Frustum {
        Frustum::Cone(ConeFrustum { focus: self.start - self.delta(), size: self.size(), index: self.index() })
    }
}
impl ConeExtension {
    fn end_top(&self) -> IVec2 {
        self.end_bottom + IVec2::Y
    }
    fn parity(&self) -> bool {
        (self.index() >= 0) == (self.index() % 2 == 0)
    }
    fn delta(&self) -> IVec2 {
        self.end_bottom - self.start
    }
    fn extend_lines(&self) -> [LineExtension; 3] {
        let half_delta = self.delta() / 2;
        [
            LineExtension { start: self.start, end: self.start + half_delta },
            LineExtension { start: self.end_bottom - half_delta, end: self.end_bottom },
            LineExtension { start: self.end_top() - half_delta, end: self.end_top() },
        ]
    }
    fn extend_cones(&self) -> [ConeExtension; 2] {
        let half_delta = self.delta() / 2;
        [
            ConeExtension { start: self.start, end_bottom: self.start + half_delta },
            ConeExtension { start: self.end_bottom - half_delta, end_bottom: self.end_bottom },
        ]
    }
    fn extend(&self) -> Vec<Extension> {
        if self.parity() {
            self.extend_lines().into_iter().map(Extension::Line).collect()
        } else {
            self.extend_cones().into_iter().map(Extension::Cone).collect()
        }
    }
}

pub fn draw_extension(extension: Extension, weight: f32, pixels: &mut HashMap<IVec2, f32>) {
    if extension.size() == 1 {
        *pixels.entry(extension.start()).or_default() += weight;
        return;
    }
    if let Extension::Cone(ext) = extension
        && ext.size() == 2
    {
        if extension.index() == 1 {
            draw_extension(
                Extension::Line(LineExtension { start: ext.start + IVec2::Y, end: ext.end_bottom + IVec2::Y }),
                weight * 0.5,
                pixels,
            );
        } else if extension.index() == -2 {
            draw_extension(
                Extension::Line(LineExtension { start: ext.start - IVec2::Y, end: ext.end_bottom - IVec2::Y }),
                weight * 0.5,
                pixels,
            );
        }
    }
    match extension {
        Extension::Line(ext) => {
            for line_ext in ext.extend() {
                draw_extension(Extension::Line(line_ext), weight, pixels);
            }
        }
        Extension::Cone(ext) => {
            for ext in ext.extend() {
                draw_extension(ext, weight, pixels);
            }
        }
    }
}

// binary occlusion for now
pub type PixelScene = HashMap<IVec2, f32>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ViewDirection {
    Up = 1,
    Down = -1,
}

impl Extension {
    // The pixels that the rays of a size 1 extension pass through, in order, with the share of its rays that cross each.
    // Every ray enters the column through the start pixel. A line of index Â±1 runs at 45Â°, so all its rays also leave
    // through the pixel above or below. A cone (index 0 or -1) fans out from its focus with slopes [0, 1] or [-1, 0];
    // the half of its slopes steeper than 1/2 cross into the pixel beyond its sloped edge before the column ends.
    fn crossed_pixels(&self) -> Vec<(IVec2, f32)> {
        assert_eq!(self.size(), 1);
        let start = self.start();
        match self {
            Extension::Line(ext) if ext.index() == 0 => vec![(start, 1.0)],
            Extension::Line(ext) => {
                assert_eq!(ext.index().abs(), 1, "{ext:?}");
                vec![(start, 1.0), (start + IVec2::Y * ext.index(), 1.0)]
            }
            Extension::Cone(ext) => {
                let side = if ext.index() >= 0 { IVec2::Y } else { -IVec2::Y };
                vec![(start, 1.0), (start + side, 0.5)]
            }
        }
    }
}

// note: pixel_scene is offset by 0.5
pub fn get_extension<L: Radiance>(
    cache: &mut HashMap<Extension, Fluence<L>>,
    extension: Extension,
    pixel_scene: &PixelScene,
    view_dir: ViewDirection,
) -> Fluence<L> {
    if extension.size() <= 0 {
        panic!("Extension size must be positive: {:?}", extension);
    }
    if let Some(cached) = cache.get(&extension) {
        return *cached;
    }

    let f = extension.as_frustum();
    // figure out edge cases later.
    if extension.size() == 1 {
        let pixels = if let Extension::Cone(cone) = extension {
            vec![(cone.start, 1.0), (cone.start + if cone.index() >= 0 { IVec2::Y } else { -IVec2::Y }, 0.5)]
        } else {
            vec![(extension.start(), 1.0)]
        };
        let fluence_values = pixels.into_iter().map(|(pixel, weight)| {
            let fluence = if let Some(_value) = pixel_scene.get(&pixel) {
                Fluence { transmittance: 0.0, radiance: L::default() }
            } else {
                // pixels are always seen on two sides by probes; so give half radiance for both sides.
                // could do something more complex, but may not be necessary.
                let next_pixel = pixel + IVec2::X;
                let next_pixel_emittance = pixel_scene.get(&next_pixel).copied().unwrap_or(0.0);
                let side_pixel = pixel + view_dir as i32 * IVec2::Y;
                let side_pixel_emittance = pixel_scene.get(&side_pixel).copied().unwrap_or(0.0);
                Fluence {
                    transmittance: 1.0,
                    radiance: (L::new_directional(f, pixel.as_vec2() + 0.5 + Vec2::X * 0.5, ViewDir::Right)
                        * next_pixel_emittance
                        * 0.5
                        + L::new_directional(
                            f,
                            pixel.as_vec2() + 0.5 + Vec2::Y * 0.5 * view_dir as i32 as f32,
                            if view_dir == ViewDirection::Up { ViewDir::Up } else { ViewDir::Down },
                        ) * side_pixel_emittance
                            * 0.5)
                        * weight,
                }
            };
            (fluence, weight)
        });
        return Fluence::join_weighted(fluence_values);

        /*return if let Some(value) = pixel_scene.get(&pixel) {
            Fluence { transmittance: 0.0, radiance: L::new(f, pixel.as_vec2() + 0.5) * *value }
        } else {
            Fluence { transmittance: 1.0, radiance: L::default() }
        };*/
    }

    let mut get_fluence_reproj =
        |ext: Extension| get_extension(cache, ext, pixel_scene, view_dir).reproject(ext.as_frustum(), f);

    let result = match extension {
        Extension::Line(line) => {
            let [first, second] = line.extend().map(|ext| get_fluence_reproj(ext.into()));
            first.over(second)
        }
        Extension::Cone(cone) => {
            if cone.parity() {
                let [first, second1, second2] = cone.extend_lines().map(|ext| get_fluence_reproj(ext.into()));
                first.over(Fluence::join([second1, second2]))
            } else {
                let [first, second] = cone.extend_cones().map(|ext| get_fluence_reproj(ext.into()));
                first.over(second)
            }
        }
    };
    cache.insert(extension, result);
    result
}
