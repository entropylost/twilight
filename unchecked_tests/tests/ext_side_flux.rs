// What share of the size-1 pieces that start in a pixel leave it sideways? Same mirror of the merge as
// ext_start_tiling, but the weight of the pieces starting in a pixel is kept per kind: [straight line, diagonal line,
// cone]. Prints, for a probe at the origin and pixels (x, y) of the upper root cone, the three weights next to the
// fan's exact values: the share of the pixel's rays that leave through its upper face, (y + 1) / (x + 1), and the
// weight those rays have at the next column, (y + 1) / x.
//   cargo test --release -p unchecked_tests --test ext_side_flux -- --nocapture

use std::collections::HashMap;

use glam::IVec2;
use twilight::extension::{ConeExtension, Extension, ExtensionType, LineExtension};
use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};

type Weights = HashMap<IVec2, [f32; 3]>;

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
        let kind = match extension {
            Extension::Cone(_) => 2,
            Extension::Line(_) if delta.y != 0 => 1,
            Extension::Line(_) => 0,
        };
        pixels.entry(start).or_default()[kind] += weight;
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
    shapes: HashMap<(bool, IVec2), Vec<(IVec2, [f32; 3])>>,
    frustums: HashMap<(i32, i32, i32), Vec<(IVec2, [f32; 3])>>,
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
            for k in 0..3 { pixels.entry(start + pixel).or_default()[k] += w[k] * weight; }
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
                for k in 0..3 { pixels.entry(cone.focus + pixel).or_default()[k] += w[k] * weight; }
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
        let own: Vec<(IVec2, [f32; 3])> = own.into_iter().map(|(pixel, w)| (pixel - cone.focus, w)).collect();
        for &(pixel, w) in &own {
            for k in 0..3 { pixels.entry(cone.focus + pixel).or_default()[k] += w[k] * weight; }
        }
        self.frustums.insert(key, own);
    }
}

#[test]
fn side_flux() {
    let mut tiling = Tiling::default();
    let fxs: Vec<i32> = std::env::var("FX").map(|v| v.split(',').map(|s| s.parse().unwrap()).collect()).unwrap_or(vec![0, 5, 22]);
    for fx in fxs {
        let focus = IVec2::new(fx, 7);
        let mut pixels = Weights::new();
        for index in [-1, 0] {
            tiling.merge(Frustum::Cone(ConeFrustum { focus, size: 1, index }), 1.0, &mut pixels);
        }
        println!("focus.x {fx}");
        let (mut e1, mut e2, mut n) = (0.0f32, 0.0f32, 0);
        for x in 1..40 {
            for y in 0..x {
                let [a, b, c] = pixels.get(&(focus + IVec2::new(x, y))).copied().unwrap_or_default();
                let (share, next) = ((y + 1) as f32 / (x + 1) as f32, (y + 1) as f32 / x as f32);
                e1 += (b + c - next).abs();
                e2 += (b + 0.5 * c - share).abs();
                n += 1;
                if (x <= 6 || x == 13 || x == 32) && (y < 4 || y >= x - 2) {
                    println!(
                        "  ({x:2},{y:2}) straight {a:.4} diag {b:.4} cone {c:.4} | diag+cone {:.4} vs {next:.4} | diag+cone/2 {:.4} vs {share:.4}",
                        b + c, b + 0.5 * c
                    );
                }
            }
        }
        println!("  mean |diag+cone - (y+1)/x| {:.4}, mean |diag+cone/2 - (y+1)/(x+1)| {:.4}", e1 / n as f32, e2 / n as f32);
    }
}
