use std::collections::HashMap;

use glam::{IVec2, Vec2};

use crate::{
    frustum::{ConeFrustum, Frustum, FrustumType, LinearFrustum},
    radiance::{Radiance, ResampleRadiance, ViewDir},
    shape::Polygon,
};

#[derive(Debug, Clone, Copy)]
pub struct Fluence<L: Radiance> {
    // mean transmittance over the extension's far end
    pub transmittance: f32,
    pub radiance: L,
    // A cone extension ends two pixels wide (end_bottom, end_top). What blocks the rays towards one of them need not
    // block the rays towards the other, so the two are kept apart: the transmittance towards end_bottom is
    // `transmittance - spread`, towards end_top `transmittance + spread`. Zero for lines, which end one pixel wide.
    pub spread: f32,
}
impl<L: Radiance> Fluence<L> {
    pub fn map<R: Radiance>(self, f: impl FnOnce(L) -> R) -> Fluence<R> {
        Fluence { transmittance: self.transmittance, radiance: f(self.radiance), spread: self.spread }
    }
    pub fn reproject(self, source: Frustum, target: Frustum) -> Self {
        self.map(|radiance| radiance.reproject(source, target))
    }
    pub fn resample<R: ResampleRadiance<L>>(self, source: Frustum, target: Frustum) -> Fluence<R> {
        self.map(|radiance| R::resample(radiance, source, target))
    }
    pub fn new(transmittance: f32, radiance: L) -> Self {
        Fluence { transmittance, radiance, spread: 0.0 }
    }
    // (towards end_bottom, towards end_top)
    pub fn end_transmittances(&self) -> (f32, f32) {
        (self.transmittance - self.spread, self.transmittance + self.spread)
    }
    // Assume that both have the same focal point.
    pub fn over(&self, other: Fluence<L>) -> Self {
        Fluence::new(self.transmittance * other.transmittance, self.radiance + other.radiance * self.transmittance)
    }
    pub fn average(&self, other: Fluence<L>) -> Self {
        Fluence::new((self.transmittance + other.transmittance) * 0.5, (self.radiance + other.radiance) * 0.5)
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
        Fluence::new(total_transmittance / count as f32, total_radiance)
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
        Fluence::new(total_transmittance / total_weight, total_radiance)
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
    fn delta(&self) -> IVec2 {
        self.end_bottom - self.start
    }
}

fn line(start: IVec2, delta: IVec2) -> Extension {
    Extension::Line(LineExtension { start, end: start + delta })
}
fn cone(start: IVec2, delta: IVec2) -> Extension {
    Extension::Cone(ConeExtension { start, end_bottom: start + delta })
}

// How an extension of size 2s is put together from extensions of size s.
struct Halves {
    start: IVec2,
    // The halves meet at the middle column in the two pixels start + lo and start + hi.
    lo: IVec2,
    hi: IVec2,
    // whether the extension's delta is 2 * lo, so that its lower edge passes through start + lo exactly
    even: bool,
}
impl Halves {
    fn new(extension: Extension) -> Self {
        let (start, delta) = match extension {
            Extension::Line(line) => (line.start, line.delta()),
            Extension::Cone(cone) => (cone.start, cone.delta()),
        };
        let lo = IVec2::new(delta.x / 2, delta.y.div_euclid(2));
        Halves { start, lo, hi: lo + IVec2::Y, even: delta.y.rem_euclid(2) == 0 }
    }
    // For a cone: at the middle it covers one pixel fully and half of the other. Returns (the offset of the full one,
    // of the half one, the half delta of its straight edge, which runs through the full one). Even: the straight edge
    // is the lower one; odd: mirrored.
    fn cone_middle(&self) -> (IVec2, IVec2, IVec2) {
        if self.even { (self.lo, self.hi, self.lo) } else { (self.hi, self.lo, self.hi) }
    }
}

// Adds the share of each pixel's area that the extension covers, times weight, when nothing is occluded. This is area
// coverage only: `get_extension` weights emission by the size-1 pieces that start at a pixel (arrival at its near
// face), not by these shares.
pub fn draw_extension(extension: Extension, weight: f32, pixels: &mut HashMap<IVec2, f32>) {
    if extension.size() == 1 {
        let start = extension.start();
        let side = start + if extension.index() >= 0 { IVec2::Y } else { -IVec2::Y };
        let (start_weight, side_weight) = match extension {
            Extension::Cone(_) => (1.0, 0.5),
            Extension::Line(line) if line.index() != 0 => (0.5, 0.5),
            Extension::Line(_) => (1.0, 0.0),
        };
        *pixels.entry(start).or_default() += weight * start_weight;
        if side_weight != 0.0 {
            *pixels.entry(side).or_default() += weight * side_weight;
        }
        return;
    }
    let halves = Halves::new(extension);
    let Halves { start, lo, hi, even } = halves;
    match extension {
        Extension::Line(_) if even => {
            draw_extension(line(start, lo), weight, pixels);
            draw_extension(line(start + lo, lo), weight, pixels);
        }
        Extension::Line(_) => {
            for (first, second) in [(lo, hi), (hi, lo)] {
                draw_extension(line(start, first), weight * 0.5, pixels);
                draw_extension(line(start + first, second), weight * 0.5, pixels);
            }
        }
        Extension::Cone(_) => {
            let (full, half, edge) = halves.cone_middle();
            for piece in [line(start, edge * 2), cone(start, lo), cone(start + full, lo), line(start + half, edge)] {
                draw_extension(piece, weight * 0.5, pixels);
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
    let f = extension.as_frustum();
    // figure out edge cases later.
    // Size 1 is read straight from the scene and never cached, so looking it up would only cost time.
    if extension.size() == 1 {
        // Emission is attributed by arrival. The decomposition into halves (and the merge) partitions the rays by where
        // they cross the near edge of each column, and at the near side of its column a size-1 piece is exactly as wide
        // as its start pixel. So its rays are the ones that arrive at the start pixel's near face: if the start is
        // solid they see that face and stop. Otherwise the side pixel, if solid, shows them its side face and blocks
        // the rays that leave sideways. (Weighting a face by the area share of the empty pixel in front of it instead
        // disagrees with that partition by half a column: rays reaching the face through a solid row neighbour are not
        // removed, which over-weights every stair-step face of an emitter, depending on the view angle.)
        // Faces are seen on two sides by probes, hence half radiance for each.
        let start = extension.start();
        if let Some(&value) = pixel_scene.get(&start) {
            let face = start.as_vec2() + Vec2::Y * 0.5;
            return Fluence::new(0.0, L::new_directional(f, face, ViewDir::Right) * (value * 0.5));
        }
        let side = start + view_dir as i32 * IVec2::Y;
        let (side_transmittance, radiance) = match pixel_scene.get(&side) {
            Some(&value) => {
                let face = start.as_vec2() + 0.5 + Vec2::Y * 0.5 * view_dir as i32 as f32;
                let dir = if view_dir == ViewDirection::Up { ViewDir::Up } else { ViewDir::Down };
                (0.0, L::new_directional(f, face, dir) * (value * 0.5))
            }
            None => (1.0, L::default()),
        };
        return match extension {
            // the rays that leave through the far side of the start pixel only cross the start; the rays that leave
            // beside it also cross the side pixel
            Extension::Cone(cone) => {
                let (bottom, top) =
                    if cone.index() >= 0 { (1.0, side_transmittance) } else { (side_transmittance, 1.0) };
                Fluence { transmittance: (bottom + top) * 0.5, radiance, spread: (top - bottom) * 0.5 }
            }
            // Every ray of a diagonal line crosses both pixels, so either one blocks it. Treating it as its start
            // alone misses the half of the side pixel that lies on the edge of the root cone, which leaves cross
            // artifacts along the diagonals.
            Extension::Line(line) if line.index() != 0 => Fluence::new(side_transmittance, radiance),
            Extension::Line(_) => Fluence::new(1.0, radiance),
        };
    }

    if let Some(cached) = cache.get(&extension) {
        return *cached;
    }

    let mut get_fluence_reproj =
        |ext: Extension| get_extension(cache, ext, pixel_scene, view_dir).reproject(ext.as_frustum(), f);

    // An extension of size 2s is put together from extensions of size s (see `Halves`). Every piece is weighted by the
    // share of the extension's width it stands for, so occlusion and emission are spread over the extension's real
    // footprint instead of being sampled along a single digital line, which makes a shadow edge snap by up to a pixel,
    // differently for neighbouring probes.
    let halves = Halves::new(extension);
    let Halves { start, lo, hi, even } = halves;
    let result = match extension {
        Extension::Line(_) if even => {
            get_fluence_reproj(line(start, lo)).over(get_fluence_reproj(line(start + lo, lo)))
        }
        // the line passes between the two middle pixels: half of it goes through the lower one, half through the upper
        Extension::Line(_) => {
            let below = get_fluence_reproj(line(start, lo)).over(get_fluence_reproj(line(start + lo, hi)));
            let above = get_fluence_reproj(line(start, hi)).over(get_fluence_reproj(line(start + hi, lo)));
            below.average(above)
        }
        // The cone is 1 -> 1.5 pixels wide over its first half: the mean of the line along its straight edge and the
        // half-size cone. Over the second half, 1.5 -> 2 wide, a line and a cone continue from the full middle pixel
        // (half weight each) and a line of half weight continues from the half one.
        // Each piece is attenuated only by the piece it continues: the second line by the first (together they are
        // the whole line along the straight edge), the second cone by the first cone towards the full pixel, and the
        // narrow line by the first cone towards the half pixel. Attenuating by a mean over pieces instead would let
        // something that blocks one piece dim pieces that pass beside it.
        Extension::Cone(_) => {
            let (full, half, edge) = halves.cone_middle();
            let edge_line = get_fluence_reproj(line(start, edge * 2));
            let first_cone = get_fluence_reproj(cone(start, lo));
            let second_cone = get_fluence_reproj(cone(start + full, lo));
            let narrow = get_fluence_reproj(line(start + half, edge));

            // the end transmittances of a half-size cone, as (towards the pixel on the straight edge, the other one)
            let ends = |cone: Fluence<L>| {
                let (bottom, top) = cone.end_transmittances();
                if even { (bottom, top) } else { (top, bottom) }
            };
            let (to_full, to_half) = ends(first_cone);
            let (second_edge, second_other) = ends(second_cone);
            // the end pixel on the straight edge is reached by the edge line and the second cone, the other one by the
            // second cone and the narrow line
            let end_edge = (edge_line.transmittance + to_full * second_edge) * 0.5;
            let end_other = (to_full * second_other + to_half * narrow.transmittance) * 0.5;
            let (bottom, top) = if even { (end_edge, end_other) } else { (end_other, end_edge) };
            Fluence {
                transmittance: (bottom + top) * 0.5,
                radiance: (edge_line.radiance
                    + first_cone.radiance
                    + second_cone.radiance * to_full
                    + narrow.radiance * to_half)
                    * 0.5,
                spread: (top - bottom) * 0.5,
            }
        }
    };
    cache.insert(extension, result);
    result
}
