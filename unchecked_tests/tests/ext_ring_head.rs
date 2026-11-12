// Is the ring spread next to a block light (ext_metrics "rotation", innermost rings) a property of the extension
// decomposition's weights, or of the lossy carriers that hold the emission?
// Renders the two shadow scenes of ext_metrics with two decompositions and scores the rings above the wall exactly as
// ext_metrics does:
//   new:  the renderer's `get_extension`
//   head: `head_get_extension` below, a copy of `get_extension` at commit e4dd3e7 (single digital path per extension)
// each with three carrier pairs (extension carrier / merge carrier):
//   dp:     DirectionalPosition / Inverse2, the renderer's
//   exact:  ExactFaces / Inverse2: every emitting face kept apart in the extensions
//   exact2: ExactFaces / ExactFaces: nothing is summarised anywhere, so only the weights are left
// If `exact2` gives the same spread for both decompositions, a difference under `dp` is the carriers' doing.
//   cargo test --release -p unchecked_tests --test ext_ring_head -- --nocapture
// SCENES=block-shadow restricts the scenes. Needs several GB of memory.
//
// How `head` is rendered without a second renderer: `get_radiance_recursive_ext` only reads extensions through its
// cache, so the scene is rendered once to learn which extensions the merge asks for, then the cache is refilled with
// `head_get_extension`'s values for exactly those and the merge is run again. (Size 1 is never read from the cache, but
// there the two versions differ only in a cone's transmittance, which the merge does not use.)
// NOTE: that stopped being true when the renderer moved to emission by arrival (a solid start pixel emits its own near
// face, see `get_extension`): the size-1 cone at the root now carries different emission than `head_get_extension`'s,
// and `probe_pixel` no longer adds the next pixel's face. `head` is therefore a mix at the root cone's first column;
// the numbers recorded in docs/EXT_DEBUG.md from this test predate that change.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, FRAC_PI_4};
use std::ops::{Add, Mul};

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::{
    ConeExtension, Extension, ExtensionType, Fluence, LineExtension, PixelScene, ViewDirection,
};
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{Constant, DirectionalPosition, Inverse2, Radiance, ResampleRadiance, ViewDir};
use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};

// Up to FACES faces, each with its own position (absolute, so reprojection is the identity) and summed weight.
const FACES: usize = 12;
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct ExactFaces {
    // (position, weight, 0 = seen horizontally / 1 = vertically); weight 0 marks a free slot
    faces: [(Vec2, f32, u8); FACES],
}
impl Radiance for ExactFaces {
    fn read(self, _source: Frustum, target: Vec2) -> f32 {
        self.faces
            .iter()
            .filter(|f| f.1 != 0.0)
            .map(|&(pos, e, axis)| {
                let v = pos - target;
                e * v[axis as usize].abs() / v.length_squared()
            })
            .sum()
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        unimplemented!()
    }
    fn new_directional(_frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        let mut faces = [(Vec2::ZERO, 0.0, 0); FACES];
        faces[0] = (emitter, 1.0, view_dir.is_vertical() as u8);
        Self { faces }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl ResampleRadiance<ExactFaces> for ExactFaces {
    fn resample(original: ExactFaces, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl Add for ExactFaces {
    type Output = Self;
    fn add(mut self, o: Self) -> Self {
        for &(pos, e, axis) in o.faces.iter().filter(|f| f.1 != 0.0) {
            let slot = self
                .faces
                .iter_mut()
                .find(|f| f.1 == 0.0 || (f.0 == pos && f.2 == axis))
                .expect("more faces in one extension than ExactFaces holds");
            *slot = (pos, slot.1 + e, axis);
        }
        self
    }
}
impl Mul<f32> for ExactFaces {
    type Output = Self;
    fn mul(mut self, r: f32) -> Self {
        for f in &mut self.faces {
            f.1 *= r;
        }
        self
    }
}

type ExtCache<L> = HashMap<Extension, Fluence<L>>;

// `get_extension` as of commit e4dd3e7. Unlike the original it also caches size 1, which changes no value.
fn head_get_extension<L: Radiance>(
    cache: &mut ExtCache<L>,
    extension: Extension,
    pixel_scene: &PixelScene,
    view_dir: ViewDirection,
) -> Fluence<L> {
    if let Some(cached) = cache.get(&extension) {
        return *cached;
    }
    let f = extension.as_frustum();
    let result = if extension.size() == 1 {
        let pixel_fluence = |pixel: IVec2, weight: f32| {
            if pixel_scene.contains_key(&pixel) {
                Fluence::new(0.0, L::default())
            } else {
                let next_pixel_emittance = pixel_scene.get(&(pixel + IVec2::X)).copied().unwrap_or(0.0);
                let side_pixel = pixel + view_dir as i32 * IVec2::Y;
                let side_pixel_emittance = pixel_scene.get(&side_pixel).copied().unwrap_or(0.0);
                Fluence::new(
                    1.0,
                    (L::new_directional(f, pixel.as_vec2() + 0.5 + Vec2::X * 0.5, ViewDir::Right)
                        * next_pixel_emittance
                        * 0.5
                        + L::new_directional(
                            f,
                            pixel.as_vec2() + 0.5 + Vec2::Y * 0.5 * view_dir as i32 as f32,
                            if view_dir == ViewDirection::Up { ViewDir::Up } else { ViewDir::Down },
                        ) * side_pixel_emittance
                            * 0.5)
                        * weight,
                )
            }
        };
        match extension {
            Extension::Cone(cone) => {
                let side = cone.start + if cone.index() >= 0 { IVec2::Y } else { -IVec2::Y };
                Fluence::join_weighted(
                    [(cone.start, 1.0), (side, 0.5)].map(|(pixel, weight)| (pixel_fluence(pixel, weight), weight)),
                )
            }
            Extension::Line(line) if line.index() != 0 => {
                let side = line.start + IVec2::Y * line.index().signum();
                pixel_fluence(line.start, 0.5).over(pixel_fluence(side, 0.5))
            }
            Extension::Line(line) => pixel_fluence(line.start, 1.0),
        }
    } else {
        let mut get = |ext: Extension| head_get_extension(cache, ext, pixel_scene, view_dir).reproject(ext.as_frustum(), f);
        let line = |start: IVec2, end: IVec2| Extension::Line(LineExtension { start, end });
        match extension {
            Extension::Line(l) => {
                let [first, second] = l.extend().map(|ext| get(ext.into()));
                first.over(second)
            }
            Extension::Cone(cone) => {
                let (start, end_bottom) = (cone.start, cone.end_bottom);
                let end_top = end_bottom + IVec2::Y;
                let half = (end_bottom - start) / 2;
                if (cone.index() >= 0) == (cone.index() % 2 == 0) {
                    let first = get(line(start, start + half));
                    let seconds = [get(line(end_bottom - half, end_bottom)), get(line(end_top - half, end_top))];
                    first.over(Fluence::join(seconds))
                } else {
                    let first = get(ConeExtension { start, end_bottom: start + half }.into());
                    first.over(get(ConeExtension { start: end_bottom - half, end_bottom }.into()))
                }
            }
        }
    };
    cache.insert(extension, result);
    result
}

// copy of the private `probe_pixel` in src/render/ext.rs
fn probe_pixel(cone: ConeFrustum, scene: &PixelScene) -> Fluence<Constant> {
    let pixel = cone.focus + IVec2::new(0, cone.index);
    if let Some(&value) = scene.get(&pixel) {
        return Fluence::new(0.0, Constant { value: value * 0.5 * FRAC_PI_4 });
    }
    // the next pixel's face is emitted by the cone's first extension (emission by arrival)
    Fluence::new(1.0, Constant { value: 0.0 })
}

type Image = HashMap<IVec2, f32>;

// `render_ext`, optionally with the extensions of commit e4dd3e7.
fn render<L: Radiance, I: Radiance + ResampleRadiance<L>>(scene: &PixelScene, head: bool) -> Image {
    let settings = RenderExtSettings { simplify_occlusion: false };
    let rot = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [Affine2::IDENTITY, rot, rot * rot, rot * rot * rot];
    let scenes = transforms.map(|t| {
        scene.iter().map(|(k, v)| (t.transform_point2(k.as_vec2() + 0.5).floor().as_ivec2(), *v)).collect::<PixelScene>()
    });
    let mut image = Image::new();
    // one (rotation, cone) at a time, to hold only one extension cache in memory
    for (t, scene) in transforms.iter().zip(&scenes) {
        for i in [-1, 0] {
            let run = |ext_cache: &mut ExtCache<L>, image: Option<&mut Image>| {
                let mut cache = HashMap::<Frustum, I>::new();
                let mut image = image;
                for x in 1..128 {
                    for y in 1..128 {
                        let cell = IVec2::new(x, y);
                        let cone = ConeFrustum {
                            focus: t.transform_point2(cell.as_vec2()).round().as_ivec2(),
                            size: 1,
                            index: i,
                        };
                        let frustum = Frustum::Cone(cone);
                        let near = probe_pixel(cone, scene);
                        let value = if near.transmittance == 0.0 {
                            near.radiance.value
                        } else {
                            let far = get_radiance_recursive_ext(ext_cache, &mut cache, frustum, scene, settings);
                            near.over(Fluence::new(1.0, Constant { value: far.at_focus(frustum) })).radiance.value
                        };
                        if let Some(image) = image.as_deref_mut() {
                            *image.entry(cell).or_default() += value;
                        }
                    }
                }
            };
            let mut ext_cache = ExtCache::<L>::new();
            if !head {
                run(&mut ext_cache, Some(&mut image));
                continue;
            }
            run(&mut ext_cache, None);
            let keys: Vec<Extension> = ext_cache.keys().copied().collect();
            drop(ext_cache);
            let view_dir = if i >= 0 { ViewDirection::Up } else { ViewDirection::Down };
            let mut head_cache = ExtCache::<L>::new();
            for &key in &keys {
                head_get_extension(&mut head_cache, key, scene, view_dir);
            }
            let filled = head_cache.len();
            run(&mut head_cache, Some(&mut image));
            // the merge found everything it asked for, so nothing was computed by the renderer's own get_extension
            assert_eq!(head_cache.len(), filled);
        }
    }
    image
}

const WALL_Y: i32 = 60;

// the "rotation" score of ext_metrics
fn score(tag: &str, ext: &Image, pt: &Image, light: Vec2, rings: std::ops::Range<usize>) {
    let mut values = vec![Vec::new(); rings.end];
    for (&cell, &p) in pt {
        if cell.y >= WALL_Y - 1 {
            continue;
        }
        let r = (cell.as_vec2() - light).length().round() as usize;
        if rings.contains(&r) && p > 1e-4 {
            values[r].push(ext[&cell] / p);
        }
    }
    let spreads: Vec<f32> = values[rings.start..]
        .iter()
        .filter(|ring| !ring.is_empty())
        .map(|ring| {
            let mean = ring.iter().sum::<f32>() / ring.len() as f32;
            let var = ring.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / ring.len() as f32;
            var.sqrt() / mean
        })
        .collect();
    let mean = spreads.iter().sum::<f32>() / spreads.len() as f32;
    let worst = spreads.iter().copied().fold(0.0, f32::max);
    println!(
        "  {tag:12} ring spread mean {mean:.4} worst {worst:.4}  radius {}..: {:?}",
        rings.start,
        spreads.iter().take(6).map(|s| (s * 10000.0).round() / 10000.0).collect::<Vec<_>>()
    );
}

#[test]
fn ring_head() {
    let only = std::env::var("SCENES").ok();
    for (name, size, emittance, rings) in [("block-shadow", 3, 2.5, 5..25), ("pixel-shadow", 1, 5.0, 4..25)] {
        if let Some(only) = &only
            && !only.split(',').any(|n| n == name)
        {
            continue;
        }
        let mut scene = PixelScene::new();
        for i in 0..size {
            for j in 0..size {
                scene.insert(IVec2::new(36 + i, 32 + j), emittance);
            }
        }
        for i in 0..50 {
            scene.insert(IVec2::new(i, WALL_Y), 0.0);
        }
        let light = Vec2::new(36.0, 32.0) + size as f32 * 0.5;
        let pt = render_pt(&scene, RenderPtSettings { directions: 4096, seed: 0 });
        println!("== {name} ==");
        for (tag, head) in [("new", false), ("head", true)] {
            let dp = render::<DirectionalPosition, Inverse2>(&scene, head);
            score(&format!("{tag} dp"), &dp, &pt, light, rings.clone());
            let exact = render::<ExactFaces, Inverse2>(&scene, head);
            score(&format!("{tag} exact"), &exact, &pt, light, rings.clone());
            let exact2 = render::<ExactFaces, ExactFaces>(&scene, head);
            score(&format!("{tag} exact2"), &exact2, &pt, light, rings.clone());
        }
    }
}
