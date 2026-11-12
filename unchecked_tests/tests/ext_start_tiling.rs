// Partition of unity for "emission by arrival" (what `get_extension` does at size 1 since the issue 2 fix), where a
// size-1 piece of an extension stands for the rays that arrive at the near (left) face of its START pixel, instead of
// for pixel area (`draw_extension`, checked by ext_tiling).
// Mirrors `get_radiance_recursive_ext` like ext_tiling does, but adds up, per pixel, the weight of the size-1 pieces
// that start in it. In one rotated frame the two root cones of a probe at the origin span, at the near edge of column
// x >= 1, exactly the pixels with -x <= y < x. With nothing occluded each of them must get weight exactly 1 (so its
// near face and its side face are each counted once) and every other pixel 0.
// Independent of what src/extension.rs does at size 1; it only relies on the decomposition into halves.
//   cargo test --release -p unchecked_tests --test ext_start_tiling -- --nocapture

use std::collections::HashMap;

use glam::IVec2;
use twilight::extension::{ConeExtension, Extension, ExtensionType, LineExtension};
use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};

type Weights = HashMap<IVec2, f32>;

fn line(start: IVec2, delta: IVec2) -> Extension {
    Extension::Line(LineExtension { start, end: start + delta })
}
fn cone(start: IVec2, delta: IVec2) -> Extension {
    Extension::Cone(ConeExtension { start, end_bottom: start + delta })
}

// the decomposition of get_extension / draw_extension, keeping only where the size-1 pieces start
fn start_weights(extension: Extension, weight: f32, pixels: &mut Weights) {
    let (start, delta) = match extension {
        Extension::Line(l) => (l.start, l.end - l.start),
        Extension::Cone(c) => (c.start, c.end_bottom - c.start),
    };
    if extension.size() == 1 {
        *pixels.entry(start).or_default() += weight;
        return;
    }
    let lo = IVec2::new(delta.x / 2, delta.y.div_euclid(2));
    let hi = lo + IVec2::Y;
    let even = delta.y.rem_euclid(2) == 0;
    match extension {
        Extension::Line(_) if even => {
            start_weights(line(start, lo), weight, pixels);
            start_weights(line(start + lo, lo), weight, pixels);
        }
        Extension::Line(_) => {
            for (first, second) in [(lo, hi), (hi, lo)] {
                start_weights(line(start, first), weight * 0.5, pixels);
                start_weights(line(start + first, second), weight * 0.5, pixels);
            }
        }
        Extension::Cone(_) => {
            let (full, half, edge) = if even { (lo, hi, lo) } else { (hi, lo, hi) };
            for piece in [line(start, edge * 2), cone(start, lo), cone(start + full, lo), line(start + half, edge)] {
                start_weights(piece, weight * 0.5, pixels);
            }
        }
    }
}

#[derive(Default)]
struct Tiling {
    shapes: HashMap<(bool, IVec2), Vec<(IVec2, f32)>>,
    frustums: HashMap<(i32, i32, i32), Vec<(IVec2, f32)>>,
}
impl Tiling {
    fn draw(&mut self, extension: Extension, weight: f32, pixels: &mut Weights) {
        let (start, key) = match extension {
            Extension::Line(line) => (line.start, (false, line.end - line.start)),
            Extension::Cone(cone) => (cone.start, (true, cone.end_bottom - cone.start)),
        };
        let shape = self.shapes.entry(key).or_insert_with(|| {
            let mut pixels = Weights::new();
            let relative = match extension {
                Extension::Line(_) => Extension::Line(LineExtension { start: IVec2::ZERO, end: key.1 }),
                Extension::Cone(_) => Extension::Cone(ConeExtension { start: IVec2::ZERO, end_bottom: key.1 }),
            };
            start_weights(relative, 1.0, &mut pixels);
            pixels.into_iter().collect()
        });
        for &(pixel, w) in shape.iter() {
            *pixels.entry(start + pixel).or_default() += w * weight;
        }
    }

    fn merge(&mut self, frustum: Frustum, weight: f32, pixels: &mut Weights) {
        let Frustum::Cone(cone) = frustum else { unreachable!() };
        if cone.size > 128 {
            return;
        }
        let key = (cone.size, cone.index, cone.focus.x.rem_euclid(256));
        if let Some(cached) = self.frustums.get(&key) {
            for &(pixel, w) in cached {
                *pixels.entry(cone.focus + pixel).or_default() += w * weight;
            }
            return;
        }
        let mut own = Weights::new();
        let start = cone.focus + cone.lower_dir();
        if frustum.parity() == 0 {
            self.draw(ConeExtension { start, end_bottom: start + cone.lower_dir() }.into(), 1.0, &mut own);
            for (f, _) in merge_even(frustum) {
                self.merge(f, 1.0, &mut own);
            }
        } else {
            for (f, _) in merge_odd(frustum, false, true) {
                self.merge(f, 0.5, &mut own);
            }
            for (f, _) in merge_odd(frustum, true, true) {
                self.merge(f, 0.5, &mut own);
            }
            let lower_end = start + cone.lower_dir() * 2;
            let middle_end = start + cone.lower_dir() + cone.upper_dir();
            self.draw(ConeExtension { start, end_bottom: lower_end }.into(), 0.5, &mut own);
            self.draw(ConeExtension { start, end_bottom: middle_end }.into(), 0.5, &mut own);
            self.draw(LineExtension { start, end: middle_end }.into(), -1.0, &mut own);
        }
        let own: Vec<(IVec2, f32)> = own.into_iter().map(|(pixel, w)| (pixel - cone.focus, w)).collect();
        for &(pixel, w) in &own {
            *pixels.entry(cone.focus + pixel).or_default() += w * weight;
        }
        self.frustums.insert(key, own);
    }
}

#[test]
fn start_tiling() {
    let mut tiling = Tiling::default();
    let mut worst_overall = 0.0f32;
    let mut count = 0;
    // the weights depend on the probe only through focus.x (parities); y is irrelevant. (focus.x = 255 mod 256 is odd
    // at every size and needs frustums beyond the size limit of 128 for its first column; ext_tiling has the same gap
    // and render_ext never puts a probe there.)
    for fx in (0..64).chain([100, 127, 128, 256, 300]) {
        let focus = IVec2::new(fx, 7);
        let mut pixels = Weights::new();
        for index in [-1, 0] {
            tiling.merge(Frustum::Cone(ConeFrustum { focus, size: 1, index }), 1.0, &mut pixels);
        }
        let mut worst = (0.0f32, IVec2::ZERO, 0.0f32);
        for x in -4..128 {
            for y in -140..140 {
                let expected = if x >= 1 && -x <= y && y < x { 1.0 } else { 0.0 };
                let w = pixels.get(&(focus + IVec2::new(x, y))).copied().unwrap_or(0.0);
                if (w - expected).abs() > worst.0 {
                    worst = ((w - expected).abs(), IVec2::new(x, y), w);
                }
            }
        }
        if worst.0 > 1e-4 || count < 3 {
            println!("focus.x {fx}: worst |weight - expected| = {:.2e} at offset {} (weight {})", worst.0, worst.1, worst.2);
        }
        worst_overall = worst_overall.max(worst.0);
        count += 1;
    }
    println!("{count} probes, columns 1..128: worst |start weight - expected| = {worst_overall:.2e}");
    assert!(worst_overall < 1e-4);
}
