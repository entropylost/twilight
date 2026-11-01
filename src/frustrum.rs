use std::hash::{Hash, Hasher};

use glam::{IVec2, Vec2};
use ordered_float::OrderedFloat;

use crate::shape::{LineSegment, Polygon};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Frustrum {
    Linear(LinearFrustrum),
    Cone(ConeFrustrum),
}
impl Frustrum {
    pub fn focus(&self) -> Vec2 {
        match self {
            Frustrum::Linear(f) => f.focus,
            Frustrum::Cone(f) => f.focus.as_vec2(),
        }
    }
    pub fn dir(&self) -> Vec2 {
        match self {
            Frustrum::Linear(f) => f.dir(),
            Frustrum::Cone(f) => f.dir(),
        }
    }
    pub fn polygon(&self, min: f32, max: f32) -> Polygon {
        let start = self.at(min);
        let end = self.at(max);
        Polygon::new(&[start.start, end.start, end.end, start.end])
    }
    pub fn ray_dir(&self, pos: Vec2) -> Vec2 {
        match self {
            Frustrum::Linear(f) => f.dir(),
            Frustrum::Cone(f) => pos - f.focus.as_vec2(),
        }
    }
    pub fn at(&self, t: f32) -> LineSegment {
        match self {
            Frustrum::Linear(f) => f.at(t),
            Frustrum::Cone(f) => f.at(t),
        }
    }
    pub fn parity(&self) -> i32 {
        match self {
            Frustrum::Linear(f) => f.parity(),
            Frustrum::Cone(f) => f.parity(),
        }
    }
    pub fn size(&self) -> i32 {
        match self {
            Frustrum::Linear(f) => f.size,
            Frustrum::Cone(f) => f.size,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LinearFrustrum {
    // offset towards bottom
    pub focus: Vec2,
    pub size: i32,
    pub index: f32,
}
impl LinearFrustrum {
    pub fn dir(&self) -> Vec2 {
        Vec2::new(self.size as f32, self.index)
    }
    pub fn at(&self, t: f32) -> LineSegment {
        LineSegment::new(
            self.focus - Vec2::Y * 0.5 + self.dir() * t,
            self.focus + Vec2::Y * 0.5 + self.dir() * t,
        )
    }
    pub fn parity(&self) -> i32 {
        assert!(self.focus.x.fract() == 0.0);
        (self.focus.x as i32).div_euclid(self.size).rem_euclid(2)
    }
    fn simplify(&self) -> (OrderedFloat<f32>, OrderedFloat<f32>, i32, OrderedFloat<f32>) {
        (
            OrderedFloat(self.focus.x),
            OrderedFloat(self.focus.y),
            self.size,
            OrderedFloat(self.index),
        )
    }
}
impl PartialEq for LinearFrustrum {
    fn eq(&self, other: &Self) -> bool {
        self.simplify() == other.simplify()
    }
}
impl Eq for LinearFrustrum {}
impl Hash for LinearFrustrum {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.simplify().hash(state);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConeFrustrum {
    pub focus: IVec2,
    pub size: i32,
    pub index: i32,
}
impl ConeFrustrum {
    pub fn lower_dir(&self) -> IVec2 {
        IVec2::new(self.size, self.index)
    }
    pub fn upper_dir(&self) -> IVec2 {
        IVec2::new(self.size, self.index + 1)
    }
    pub fn dir(&self) -> Vec2 {
        Vec2::new(self.size as f32, self.index as f32 + 0.5)
    }
    pub fn at(&self, t: f32) -> LineSegment {
        LineSegment::new(
            self.focus.as_vec2() + self.lower_dir().as_vec2() * t,
            self.focus.as_vec2() + self.upper_dir().as_vec2() * t,
        )
    }
    pub fn parity(&self) -> i32 {
        self.focus.x.div_euclid(self.size).rem_euclid(2)
    }
}

pub fn merge_even(frustrum: Frustrum) -> Vec<(Frustrum, f32)> {
    match frustrum {
        Frustrum::Linear(f) => vec![(
            Frustrum::Linear(LinearFrustrum {
                focus: f.focus,
                size: f.size * 2,
                index: f.index * 2.0,
            }),
            1.0,
        )],
        Frustrum::Cone(f) => vec![
            (
                Frustrum::Cone(ConeFrustrum {
                    focus: f.focus,
                    size: f.size * 2,
                    index: f.index * 2,
                }),
                1.0,
            ),
            (
                Frustrum::Cone(ConeFrustrum {
                    focus: f.focus,
                    size: f.size * 2,
                    index: f.index * 2 + 1,
                }),
                1.0,
            ),
        ],
    }
}
pub fn merge_odd(frustrum: Frustrum, far: bool, skip_linear: bool) -> Vec<(Frustrum, f32)> {
    let offset = if far { 1 } else { -1 };
    let offset_f = offset as f32;
    match frustrum {
        Frustrum::Linear(f) => vec![(
            Frustrum::Linear(LinearFrustrum {
                focus: f.focus + f.dir() * offset_f,
                size: f.size * 2,
                index: f.index * 2.0,
            }),
            1.0,
        )],
        Frustrum::Cone(f) => [
            (
                Frustrum::Cone(ConeFrustrum {
                    focus: f.focus + f.lower_dir() * offset,
                    size: f.size * 2,
                    index: f.index * 2,
                }),
                1.0,
            ),
            (
                Frustrum::Cone(ConeFrustrum {
                    focus: f.focus + f.upper_dir() * offset,
                    size: f.size * 2,
                    index: f.index * 2 + 1,
                }),
                1.0,
            ),
        ]
        .into_iter()
        .chain((!skip_linear).then_some((
            Frustrum::Linear(LinearFrustrum {
                focus: f.focus.as_vec2() + f.dir() * offset as f32,
                size: f.size * 2,
                index: f.dir().y * 2.0,
            }),
            offset as f32,
        )))
        .collect(),
    }
}
