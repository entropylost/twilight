// Investigates the plus and cross shaped artifacts through the emitter in the offline `--ext` render: probes on the
// root cones' boundaries (the axes and diagonals through the emitter) see a single emitting pixel only half the time.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::ops::{Add, Mul};

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance, ResampleRadiance, ViewDir};
use twilight::render::ext::{get_radiance_recursive_ext, render_ext};

// Counts each face the pixel is seen through: x for the faces seen moving horizontally, y for vertically.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct FaceCount {
    faces: Vec2,
}
impl Radiance for FaceCount {
    fn read(self, _source: Frustum, _target: Vec2) -> f32 {
        self.faces.element_sum()
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        Self { faces: Vec2::splat(0.5) }
    }
    fn new_directional(_frustum: Frustum, _emitter: Vec2, view_dir: ViewDir) -> Self {
        Self { faces: if view_dir.is_horizontal() { Vec2::X } else { Vec2::Y } }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl ResampleRadiance<FaceCount> for FaceCount {
    fn resample(original: FaceCount, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl Add for FaceCount {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { faces: self.faces + other.faces }
    }
}
impl Mul<f32> for FaceCount {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { faces: self.faces * rhs }
    }
}

const EMITTER: IVec2 = IVec2::new(36, 32);

fn single_pixel() -> PixelScene {
    PixelScene::from([(EMITTER, 1.0)])
}

fn transforms() -> [Affine2; 4] {
    let rotate = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    [Affine2::IDENTITY, rotate, rotate * rotate, rotate * rotate * rotate]
}

// Per root cone (rotation k, index i): which faces of the emitting pixel the probe sees.
#[test]
fn cross_root_faces() {
    let scene = single_pixel();
    for offset in [IVec2::new(-6, 0), IVec2::new(-6, 1), IVec2::new(-6, 2), IVec2::new(-6, -6), IVec2::new(-6, -5), IVec2::new(6, -6), IVec2::new(6, 5), IVec2::new(-6, 6)] {
        let cell = EMITTER + offset;
        let mut line = format!("offset {offset}:");
        for (k, t) in transforms().iter().enumerate() {
            let scene = scene
                .iter()
                .map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v))
                .collect::<PixelScene>();
            let probe = t.transform_point2(cell.as_vec2()).round().as_ivec2();
            for i in [-1, 0] {
                let frustum = Frustum::Cone(ConeFrustum { focus: probe, size: 1, index: i });
                let r = get_radiance_recursive_ext::<FaceCount, FaceCount>(
                    &mut HashMap::new(),
                    &mut HashMap::new(),
                    frustum,
                    &scene,
                );
                if r.faces != Vec2::ZERO {
                    line += &format!(" k={k} i={i:2} faces(h,v)={:?}", r.faces.to_array());
                }
            }
        }
        println!("{line}");
    }
}

// The real carriers' radiance from one pixel, over what an unoccluded pixel subtends: (|cos| + |sin|) / 2 / distance
// (both faces at half weight).
#[test]
fn cross_ratio() {
    let image = render_ext::<DirectionalPosition, Inverse2>(&single_pixel());
    let ratio = |cell: IVec2| {
        let d = EMITTER.as_vec2() + 0.5 - cell.as_vec2();
        let expected = (d.x.abs() + d.y.abs()) / d.length() * 0.5 / d.length();
        image[&cell] / expected
    };
    for dy in -10i32..=10 {
        let row: Vec<String> = (-10..=10)
            .map(|dx| {
                let cell = EMITTER + IVec2::new(dx, dy);
                if dx.abs().max(dy.abs()) < 2 { "  .  ".to_string() } else { format!("{:5.2}", ratio(cell)) }
            })
            .collect();
        println!("dy={dy:3} {}", row.join(""));
    }
}

// With the probe fixed, moves a single emitting pixel around it and prints how much of each of its faces all root cones
// together see. Each face should be seen at weight 0.5 (h: seen moving horizontally, v: vertically).
#[test]
fn cross_face_maps() {
    let probe = IVec2::new(64, 64);
    let mut maps = [[[0.0f32; 21]; 21]; 2];
    for dy in -10i32..=10 {
        for dx in -10i32..=10 {
            if dx.abs().max(dy.abs()) < 2 {
                continue;
            }
            let pixel = probe + IVec2::new(dx, dy);
            let mut faces = Vec2::ZERO;
            for t in transforms() {
                let scene = PixelScene::from([(t.transform_point2(pixel.as_vec2() + 0.5).floor().as_ivec2(), 1.0)]);
                let focus = t.transform_point2(probe.as_vec2()).round().as_ivec2();
                // faces are counted in the rotated frame, so swap them back for odd rotations
                let swap = (t.matrix2.x_axis.x).abs() < 0.5;
                for i in [-1, 0] {
                    let frustum = Frustum::Cone(ConeFrustum { focus, size: 1, index: i });
                    let r = get_radiance_recursive_ext::<FaceCount, FaceCount>(
                        &mut HashMap::new(),
                        &mut HashMap::new(),
                        frustum,
                        &scene,
                    );
                    faces += if swap { Vec2::new(r.faces.y, r.faces.x) } else { r.faces };
                }
            }
            maps[0][(dy + 10) as usize][(dx + 10) as usize] = faces.x;
            maps[1][(dy + 10) as usize][(dx + 10) as usize] = faces.y;
        }
    }
    for (name, map) in ["h", "v"].iter().zip(maps) {
        println!("{name} faces, pixel offset from probe (x right, y down the list):");
        for (row, dy) in map.iter().zip(-10..) {
            let row: String = row.iter().map(|v| format!("{v:5.2}")).collect();
            println!("  dy={dy:3}{row}");
        }
    }
}

// Prints a row above the offline scene's emitter block and a column left of it, to measure the plus.
#[test]
fn plus_profile() {
    let mut scene = PixelScene::new();
    for i in 0..8 {
        for j in 0..8 {
            scene.insert(IVec2::new(36 + i, 32 + j), 1.0);
        }
    }
    let image = render_ext::<DirectionalPosition, Inverse2>(&scene);
    let row: Vec<String> = (28..53).map(|x| format!("{x}:{:.3}", image[&IVec2::new(x, 16)])).collect();
    println!("row y=16: {}", row.join(" "));
    let col: Vec<String> = (24..49).map(|y| format!("{y}:{:.3}", image[&IVec2::new(16, y)])).collect();
    println!("col x=16: {}", col.join(" "));
}
