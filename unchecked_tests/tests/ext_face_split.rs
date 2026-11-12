// Where does the shadow edge of `render_ext` sit for each kind of face of the light, compared with the exact answer?
// A probe below the wall and right of the light sees the light's bottom faces (near faces of the rotation that looks
// up) and its right faces (side faces of that rotation). With binary occlusion by the wall the exact value of either
// kind is emittance / 2 * (the angle under which the visible part of the faces is seen), which is computed here in
// closed form (no path tracing).
// Per row below the wall, walking from the dark side up to the column where the exact value peaks:
//   shift = sum of (ext - exact) / peak: how far ext's edge is displaced into the dark side, in pixels
//   blur  = sum of |ext - exact| / peak
//   edge  = ext / peak at the last probe where the exact value is 0 (what ext_metrics calls leak, mostly)
// PART=all (default) scores the whole render. PART=near / PART=side score one kind of face alone: the render then uses
// `OneKind`, a wrapper around `DirectionalPosition` whose `new_directional` drops the faces of the other kind (near
// faces are the ones `get_extension` emits with `ViewDir::Right`, side faces the ones with `Up` / `Down`), so no switch
// in src/ is needed and the same test runs on any commit. ROWS=1 also prints every row.
//   cargo test --release -p unchecked_tests --test ext_face_split -- --nocapture

use std::ops::{Add, Mul};

use glam::{IVec2, Vec2};
use twilight::extension::PixelScene;
use twilight::frustum::Frustum;
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance, ViewDir};
use twilight::render::ext::{RenderExtSettings, render_ext};

const WALL_Y: i32 = 60;

// DirectionalPosition that only takes the emission of one kind of face: side faces if SIDE, else near faces.
#[derive(Debug, Clone, Copy, Default)]
struct OneKind<const SIDE: bool>(DirectionalPosition);
impl<const SIDE: bool> Radiance for OneKind<SIDE> {
    fn read(self, source: Frustum, target: Vec2) -> f32 {
        self.0.read(source, target)
    }
    fn new(frustum: Frustum, emitter: Vec2) -> Self {
        Self(DirectionalPosition::new(frustum, emitter))
    }
    fn new_directional(frustum: Frustum, emitter: Vec2, view_dir: ViewDir) -> Self {
        if view_dir.is_vertical() == SIDE {
            Self(DirectionalPosition::new_directional(frustum, emitter, view_dir))
        } else {
            Self::default()
        }
    }
    fn reproject(self, source: Frustum, target: Frustum) -> Self {
        Self(self.0.reproject(source, target))
    }
}
impl<const SIDE: bool> Add for OneKind<SIDE> {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}
impl<const SIDE: bool> Mul<f32> for OneKind<SIDE> {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self(self.0 * rhs)
    }
}

fn scene(size: i32, emittance: f32, wall: i32) -> PixelScene {
    let mut scene = PixelScene::new();
    for i in 0..size {
        for j in 0..size {
            scene.insert(IVec2::new(36 + i, 32 + j), emittance);
        }
    }
    for i in 0..wall {
        scene.insert(IVec2::new(i, WALL_Y), 0.0);
    }
    scene
}

// The angle under which the probe sees the part of the segment a..b that is not hidden by the wall, whose end is the
// corner (wall, WALL_Y): a ray from below towards the upper left is blocked iff it crosses y = WALL_Y left of it.
fn visible_angle(probe: Vec2, a: Vec2, b: Vec2, wall: f32) -> f32 {
    let crossing = |t: f32| {
        let q = a + (b - a) * t;
        probe.x + (q.x - probe.x) * (probe.y - WALL_Y as f32) / (probe.y - q.y)
    };
    let (c0, c1) = (crossing(0.0), crossing(1.0));
    // the crossing is monotonic along the segment (it is seen from one side), so the visible part is one interval
    let (t0, t1) = if c0 >= wall && c1 >= wall {
        (0.0, 1.0)
    } else if c0 < wall && c1 < wall {
        return 0.0;
    } else {
        let (mut lo, mut hi) = (0.0f32, 1.0f32);
        for _ in 0..40 {
            let mid = (lo + hi) * 0.5;
            if (crossing(mid) >= wall) == (c0 >= wall) { lo = mid } else { hi = mid }
        }
        if c0 >= wall { (0.0, lo) } else { (hi, 1.0) }
    };
    let (u, v) = (a + (b - a) * t0 - probe, a + (b - a) * t1 - probe);
    u.perp_dot(v).atan2(u.dot(v)).abs()
}

#[test]
fn face_split() {
    let part = std::env::var("PART").unwrap_or_else(|_| "all".into());
    for (name, size, emittance, wall) in [
        ("block-shadow", 3, 2.5, 50),
        ("pixel-shadow", 1, 5.0, 50),
        ("pixel wall 47", 1, 5.0, 47),
        ("block wall 53", 3, 2.5, 53),
        ("pixel wall 60", 1, 5.0, 60),
    ] {
        let scene = scene(size, emittance, wall);
        let settings = RenderExtSettings { simplify_occlusion: false };
        let ext = match part.as_str() {
            "near" => render_ext::<OneKind<false>, Inverse2>(&scene, settings),
            "side" => render_ext::<OneKind<true>, Inverse2>(&scene, settings),
            _ => render_ext::<DirectionalPosition, Inverse2>(&scene, settings),
        };
        let (left, right, top, bottom) = (36.0, (36 + size) as f32, 32.0, (32 + size) as f32);
        let exact = |x: i32, y: i32| {
            let probe = Vec2::new(x as f32, y as f32);
            let near = visible_angle(probe, Vec2::new(left, bottom), Vec2::new(right, bottom), wall as f32);
            let side = if probe.x > right {
                visible_angle(probe, Vec2::new(right, top), Vec2::new(right, bottom), wall as f32)
            } else {
                0.0
            };
            emittance * 0.5
                * match part.as_str() {
                    "near" => near,
                    "side" => side,
                    _ => near + side,
                }
        };
        let (mut shifts, mut blurs, mut edges) = (Vec::new(), Vec::new(), Vec::new());
        let mut lit_error = 0.0f32;
        let mut rows = Vec::new();
        for y in WALL_Y + 2..128 {
            let e: Vec<f32> = (1..128).map(|x| ext[&IVec2::new(x, y)]).collect();
            let p: Vec<f32> = (1..128).map(|x| exact(x, y)).collect();
            // Only probes right of the light (left of it the left faces take over), and only up to a few columns past
            // the edge: further right the probe is nearer to the light sideways than vertically, where the rotation
            // that looks left takes over and the two kinds of face swap roles.
            let from = (right as usize + 1).min(p.len() - 1);
            let Some(last_dark) = (from..p.len()).filter(|&x| p[x] == 0.0).last() else { continue };
            let to = (last_dark + 8).min(p.len() - 1);
            let peak = (from..=to).map(|x| p[x]).fold(0.0, f32::max);
            if peak == 0.0 {
                continue;
            }
            shifts.push((from..=to).map(|x| e[x] - p[x]).sum::<f32>() / peak);
            blurs.push((from..=to).map(|x| (e[x] - p[x]).abs()).sum::<f32>() / peak);
            edges.push(e[last_dark] / peak);
            lit_error = lit_error.max(((e[to] - p[to]) / p[to]).abs());
            rows.push(y);
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let worst = edges.iter().copied().fold(0.0, f32::max);
        println!(
            "[{part}] {name}: shift mean {:+.4} px | blur mean {:.4} px | edge mean {:.4} worst {:.4} | rel err 8 columns past the edge: {:.4}",
            mean(&shifts),
            mean(&blurs),
            mean(&edges),
            worst,
            lit_error
        );
        if std::env::var("ROWS").is_ok() {
            for (i, y) in rows.iter().enumerate() {
                println!("    row {y}: shift {:+.3} blur {:.3} edge {:.3}", shifts[i], blurs[i], edges[i]);
            }
        }
    }
}
