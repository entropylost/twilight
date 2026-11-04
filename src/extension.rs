use std::collections::HashMap;

use glam::{IVec2, Vec2};

use crate::{
    frustum::{ConeFrustum, FrustumType, LinearFrustum},
    shape::Polygon,
};

pub trait RayValue {}

pub trait ExtensionType {
    fn size(&self) -> i32;
    fn start(&self) -> IVec2;
    fn index(&self) -> i32;
    fn polygon(&self) -> Polygon;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Extension {
    Line(LineExtension),
    Cone(ConeExtension),
}
impl ExtensionType for Extension {
    fn size(&self) -> i32 {
        match self {
            Extension::Line(ext) => ext.size(),
            Extension::Cone(ext) => ext.size(),
        }
    }
    fn start(&self) -> IVec2 {
        match self {
            Extension::Line(ext) => ext.start(),
            Extension::Cone(ext) => ext.start(),
        }
    }
    fn polygon(&self) -> Polygon {
        match self {
            Extension::Line(ext) => ext.polygon(),
            Extension::Cone(ext) => ext.polygon(),
        }
    }
    fn index(&self) -> i32 {
        match self {
            Extension::Line(ext) => ext.index(),
            Extension::Cone(ext) => ext.index(),
        }
    }
}

// end is exclusive
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LineExtension {
    start: IVec2,
    end: IVec2,
}
impl ExtensionType for LineExtension {
    fn size(&self) -> i32 {
        self.delta().x
    }
    fn start(&self) -> IVec2 {
        self.start
    }
    fn polygon(&self) -> Polygon {
        LinearFrustum {
            focus: self.start.as_vec2() + Vec2::Y * 0.5,
            size: self.size(),
            index: self.index() as f32,
        }
        .polygon(0.0, 1.0)
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
            LineExtension {
                start: self.start,
                end: (self.start + half_delta),
            },
            LineExtension {
                start: (self.end - half_delta),
                end: self.end,
            },
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
    fn polygon(&self) -> Polygon {
        ConeFrustum {
            focus: self.start - self.delta(),
            size: self.size(),
            index: self.index(),
        }
        .polygon(1.0, 2.0)
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
            LineExtension {
                start: self.start,
                end: self.start + half_delta,
            },
            LineExtension {
                start: self.end_bottom - half_delta,
                end: self.end_bottom,
            },
            LineExtension {
                start: self.end_top() - half_delta,
                end: self.end_top(),
            },
        ]
    }
    fn extend_cones(&self) -> [ConeExtension; 2] {
        let half_delta = self.delta() / 2;
        [
            ConeExtension {
                start: self.start,
                end_bottom: self.start + half_delta,
            },
            ConeExtension {
                start: self.end_bottom - half_delta,
                end_bottom: self.end_bottom,
            },
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
                Extension::Line(LineExtension {
                    start: ext.start + IVec2::Y,
                    end: ext.end_bottom + IVec2::Y,
                }),
                weight * 0.5,
                pixels,
            );
        } else if extension.index() == -2 {
            draw_extension(
                Extension::Line(LineExtension {
                    start: ext.start - IVec2::Y,
                    end: ext.end_bottom - IVec2::Y,
                }),
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

/*
pub struct PixelScene {}

fn get_extension_recursive<V: RayValue>(
    cache: &mut HashMap<Extension, V>,
    extension: Extension,
    pixel_scene: &PixelScene,
) -> V {
    match extension {
        Extension::Line(ext) => {
            todo!()
        }
        Extension::Cone(ext) => {
            if (ext.index() >= 0) == (ext.index() % 2 == 0) {
            } else {
                let first_cone = ConeExtension {
                    start: ext.start,
                    end_bottom: ext.start + half_delta,
                };
                let second_cone = ConeExtension {
                    start: ext.end_bottom - half_delta,
                    end_bottom: ext.end_bottom,
                };
            }

            todo!()
        }
    }
}
 */
