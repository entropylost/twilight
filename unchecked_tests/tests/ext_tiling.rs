// Do the unoccluded weights of the whole extension merge tile the plane?
// Mirrors `get_radiance_recursive_ext` (even merge: near cone + two far cones; odd merge: 0.5 short + 0.5 long with the
// near part B + T - 2M) for the eight root cones of a probe (4 rotations x 2 cones, as `render_ext` sets them up), but
// instead of radiance it adds up, per pixel, the weight `draw_extension` says each near extension gives that pixel.
// With nothing occluded every pixel around the probe must end up with weight exactly 1, apart from the four pixels
// touching the probe, which `probe_pixel` handles.
//   cargo test --release -p unchecked_tests --test ext_tiling -- --nocapture

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::{ConeExtension, Extension, LineExtension, draw_extension};
use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};

type Weights = HashMap<IVec2, f32>;

#[derive(Default)]
struct Tiling {
    // draw_extension's weights of (is cone, delta), relative to the extension's start
    shapes: HashMap<(bool, IVec2), Vec<(IVec2, f32)>>,
    // the merged weights of a frustum of (size, index, focus.x mod 256), relative to its focus
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
            draw_extension(relative, 1.0, &mut pixels);
            pixels.into_iter().collect()
        });
        for &(pixel, w) in shape.iter() {
            *pixels.entry(start + pixel).or_default() += w * weight;
        }
    }

    // the structure of get_radiance_recursive_ext with every transmittance 1
    fn merge(&mut self, frustum: Frustum, weight: f32, pixels: &mut Weights) {
        let Frustum::Cone(cone) = frustum else { unreachable!() };
        if cone.size > 128 {
            return;
        }
        // a frustum's weights only depend on its focus through the focus's position within the 256-periodic parity
        // pattern of all the sizes above it
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
            // calculate_long_near
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
fn tiling() {
    let rot = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [Affine2::IDENTITY, rot, rot * rot, rot * rot * rot];
    let mut tiling = Tiling::default();
    // The merge stops at size 128, and where the largest frustums end depends on the probe's place on the lattice: from
    // 129 pixels away the weights fade out. Everything nearer is covered.
    let radius = 128;
    let mut worst_overall = 0.0f32;
    // probes on every parity pattern of the small sizes, and a few elsewhere on the lattice
    let probes = (1..=16)
        .flat_map(|x| (1..=16).map(move |y| IVec2::new(x, y)))
        .chain([IVec2::new(37, 90), IVec2::new(64, 64), IVec2::new(100, 21), IVec2::new(127, 127)]);
    let (mut count, mut printed) = (0, 0);
    for probe in probes {
        let mut pixels = Weights::new();
        for t in transforms {
            let focus = t.transform_point2(probe.as_vec2()).round().as_ivec2();
            let mut rotated = Weights::new();
            for index in [-1, 0] {
                tiling.merge(Frustum::Cone(ConeFrustum { focus, size: 1, index }), 1.0, &mut rotated);
            }
            // back to the unrotated frame, the inverse of how render_ext rotates the scene
            let back = t.inverse();
            for (pixel, w) in rotated {
                *pixels.entry(back.transform_point2(pixel.as_vec2() + 0.5).floor().as_ivec2()).or_default() += w;
            }
        }
        let mut worst = (0.0f32, IVec2::ZERO, 0.0);
        for x in -radius..radius {
            for y in -radius..radius {
                // the four pixels around the probe
                if (-1..=0).contains(&x) && (-1..=0).contains(&y) {
                    continue;
                }
                let w = pixels.get(&(probe + IVec2::new(x, y))).copied().unwrap_or(0.0);
                if (w - 1.0).abs() > worst.0 {
                    worst = ((w - 1.0).abs(), IVec2::new(x, y), w);
                }
            }
        }
        let touching: Vec<f32> = [(-1, -1), (0, -1), (-1, 0), (0, 0)]
            .map(|(x, y)| pixels.get(&(probe + IVec2::new(x, y))).copied().unwrap_or(0.0))
            .to_vec();
        if (worst.0 > 1e-4 && printed < 8) || count < 4 {
            printed += 1;
            println!(
                "probe {probe}: worst |weight - 1| = {:.2e} at offset {} (weight {}); touching pixels {touching:?}",
                worst.0, worst.1, worst.2
            );
        }
        worst_overall = worst_overall.max(worst.0);
        count += 1;
    }
    println!("{count} probes, every pixel within {radius} of each: worst |weight - 1| = {worst_overall:.2e}");
    assert!(worst_overall < 1e-4);
}
