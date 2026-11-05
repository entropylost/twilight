use std::collections::HashMap;

use glam::{IVec2, Vec2};

use crate::{
    frustum::{ConeFrustum, Frustum, FrustumType, LinearFrustum},
    radiance::{Radiance, ViewDir},
    shape::Polygon,
};

#[derive(Debug, Clone, Copy)]
pub struct Fluence<L: Radiance> {
    pub transmittance: f32,
    pub radiance: L,
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
        let pixel = extension.start();
        /*return if let Some(value) = pixel_scene.get(&pixel) {
            Fluence { transmittance: 0.0, radiance: L::new(f, pixel.as_vec2() + 0.5) * *value }
        } else {
            Fluence { transmittance: 1.0, radiance: L::default() }
        };*/
        return if let Some(_value) = pixel_scene.get(&pixel) {
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
                radiance: L::new_directional(f, next_pixel.as_vec2() + 0.5, ViewDir::Right)
                    * next_pixel_emittance
                    * 0.5
                    + L::new_directional(
                        f,
                        side_pixel.as_vec2() + 0.5,
                        if view_dir == ViewDirection::Up { ViewDir::Up } else { ViewDir::Down },
                    ) * side_pixel_emittance
                        * 0.5,
            }
        };
    }

    let mut get_fluence_reproj = |ext: Extension| {
        let fluence = get_extension(cache, ext, pixel_scene, view_dir);
        Fluence { transmittance: fluence.transmittance, radiance: fluence.radiance.reproject(ext.as_frustum(), f) }
    };

    let result = match extension {
        Extension::Line(line) => {
            let [first, second] = line.extend();
            let first_fluence = get_fluence_reproj(first.into());
            let second_fluence = get_fluence_reproj(second.into());
            Fluence {
                transmittance: first_fluence.transmittance * second_fluence.transmittance,
                radiance: first_fluence.radiance + second_fluence.radiance * first_fluence.transmittance,
            }
        }
        Extension::Cone(cone) => {
            if cone.parity() {
                let [first, second1, second2] = cone.extend_lines();
                let first_fluence = get_fluence_reproj(first.into());
                let second1_fluence = get_fluence_reproj(second1.into());
                let second2_fluence = get_fluence_reproj(second2.into());
                Fluence {
                    transmittance: first_fluence.transmittance
                        * (second1_fluence.transmittance + second2_fluence.transmittance)
                        / 2.0,
                    radiance: first_fluence.radiance
                        + (second1_fluence.radiance + second2_fluence.radiance) * first_fluence.transmittance,
                }
            } else {
                let [first, second] = cone.extend_cones();
                let first_fluence = get_fluence_reproj(first.into());
                let second_fluence = get_fluence_reproj(second.into());
                Fluence {
                    transmittance: first_fluence.transmittance * second_fluence.transmittance,
                    radiance: first_fluence.radiance + second_fluence.radiance * first_fluence.transmittance,
                }
            }
        }
    };
    cache.insert(extension, result);
    result
}
