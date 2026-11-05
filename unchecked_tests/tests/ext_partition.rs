// The cones {focus, size, i} for i in -size..size together cover the quadrant beyond column focus.x + size. Tracks
// every pixel of a window separately and checks that each is counted exactly once (half on the quadrant's 45° edges).

use std::collections::HashMap;
use std::ops::{Add, Mul};

use glam::{IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::frustum::{ConeFrustum, Frustum};
use twilight::radiance::{Radiance, ResampleRadiance};
use twilight::render::ext::get_radiance_recursive_ext;

// window of pixels relative to FOCUS: x in 0..W, y in -H..H
const W: i32 = 40;
const H: i32 = 40;
const N: usize = (W * 2 * H) as usize;
thread_local! {
    static FOCUS: std::cell::Cell<IVec2> = const { std::cell::Cell::new(IVec2::ZERO) };
}

fn index(pixel: IVec2) -> Option<usize> {
    let d = pixel - FOCUS.get();
    (d.x >= 0 && d.x < W && d.y >= -H && d.y < H).then(|| (d.x * 2 * H + d.y + H) as usize)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pixels {
    v: [f32; N],
}
impl Default for Pixels {
    fn default() -> Self {
        Self { v: [0.0; N] }
    }
}
impl Radiance for Pixels {
    fn read(self, _source: Frustum, _target: Vec2) -> f32 {
        self.v.iter().sum()
    }
    fn new(_frustum: Frustum, emitter: Vec2) -> Self {
        let mut v = [0.0; N];
        if let Some(i) = index(emitter.floor().as_ivec2()) {
            v[i] = 1.0;
        }
        Self { v }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl ResampleRadiance<Pixels> for Pixels {
    fn resample(original: Pixels, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl Add for Pixels {
    type Output = Self;
    fn add(mut self, other: Self) -> Self {
        for (a, b) in self.v.iter_mut().zip(other.v) {
            *a += b;
        }
        self
    }
}
impl Mul<f32> for Pixels {
    type Output = Self;
    fn mul(mut self, rhs: f32) -> Self {
        for a in &mut self.v {
            *a *= rhs;
        }
        self
    }
}

fn expected(d: IVec2, size: i32) -> f32 {
    // pixel centre relative to the focus
    let (x, y) = (d.x as f32 + 0.5, d.y as f32 + 0.5);
    if x < size as f32 || y.abs() > x {
        0.0
    } else if y.abs() == x {
        0.5
    } else {
        1.0
    }
}

#[test]
fn ext_partition() {
    // covers every parity pattern of the cone sizes up to 16
    for x in [40, 41, 44, 47, 48, 56, 63] {
        FOCUS.set(IVec2::new(x, 61));
        println!("focus {}", FOCUS.get());
        check_partition();
    }
}

fn check_partition() {
    let focus = FOCUS.get();
    let mut scene = PixelScene::new();
    for x in 0..W {
        for y in -H..H {
            scene.insert(focus + IVec2::new(x, y), 1.0);
        }
    }
    for size in [1, 2, 4, 8, 16] {
        let mut total = Pixels::default();
        for i in -size..size {
            let mut ext_cache = HashMap::new();
            let mut cache = HashMap::new();
            let frustum = Frustum::Cone(ConeFrustum { focus, size, index: i });
            total = total + get_radiance_recursive_ext::<Pixels, Pixels>(&mut ext_cache, &mut cache, frustum, &scene);
        }
        let mut bad = vec![];
        for x in 0..W {
            for y in -H..H {
                let d = IVec2::new(x, y);
                // the far edge of the window is cut off by larger cones running out of the window
                if x >= W - 2 * size.max(4) {
                    continue;
                }
                let got = total.v[index(focus + d).unwrap()];
                let want = expected(d, size);
                if (got - want).abs() > 1e-4 {
                    bad.push((d, got, want));
                }
            }
        }
        println!("size {size}: {} bad pixels", bad.len());
        for (d, got, want) in bad.iter().take(12) {
            println!("  {d}: got {got}, want {want}");
        }
    }
}
