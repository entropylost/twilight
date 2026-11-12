// Shadow leak diagnostics (issue 1 of docs/EXT_DEBUG.md). Splits render_ext along a row below the wall by rotation and
// size-1 cone, compares every frustum of the hierarchy against an exactly traced value (tracing is only done here, in
// the test), and prints the merge terms (short children, long near B / T / M, long far children and their line
// transmittances) of the frustums whose value is wrong.
//
//   cargo test --release -p unchecked_tests --test ext_leak_terms split -- --nocapture
//   cargo test --release -p unchecked_tests --test ext_leak_terms tree -- --nocapture
// Environment: SCENE=pixel|block, Y (row, default 100), X0..X1 (columns), and for `tree`: X, K, I (rotation and cone),
// WALL_END (wall length, 0 = no wall), DEPTH (levels printed), TOL (only print frustums whose error exceeds TOL * |root truth|).

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use glam::{Affine2, IVec2, Vec2};
use twilight::extension::{
    ConeExtension, Extension, ExtensionType, Fluence, LineExtension, PixelScene, ViewDirection, get_extension,
};
use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};
use twilight::radiance::{DirectionalPosition, Inverse2, Radiance, ResampleRadiance};
use twilight::render::ext::{RenderExtSettings, get_radiance_recursive_ext};

type L = DirectionalPosition;
type I = Inverse2;
type ExtCache = HashMap<Extension, Fluence<L>>;
type Cache = HashMap<Frustum, I>;

const SETTINGS: RenderExtSettings = RenderExtSettings { simplify_occlusion: false };

fn env<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

pub fn base_scene() -> PixelScene {
    let mut scene = PixelScene::new();
    if env("SCENE", "pixel".to_string()) == "block" {
        for i in 0..3 {
            for j in 0..3 {
                scene.insert(IVec2::new(36 + i, 32 + j), 2.5);
            }
        }
    } else {
        scene.insert(IVec2::new(36, 32), 5.0);
    }
    // WALL_END: the wall covers x in 0..WALL_END (50 in the offline scenes; 0 removes it)
    for i in 0..env("WALL_END", 50) {
        scene.insert(IVec2::new(i, 60), 0.0);
    }
    scene
}

pub fn rotation(k: usize) -> Affine2 {
    let rotate = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    (0..k).fold(Affine2::IDENTITY, |a, _| rotate * a)
}

pub fn rotated(scene: &PixelScene, k: usize) -> PixelScene {
    let t = rotation(k);
    scene.iter().map(|(p, v)| (t.transform_point2(p.as_vec2() + 0.5).floor().as_ivec2(), *v)).collect()
}

pub fn probe_of(cell: IVec2, k: usize) -> IVec2 {
    rotation(k).transform_point2(cell.as_vec2()).round().as_ivec2()
}

// First solid pixel a ray meets, marching from `origin` (which may lie on a pixel boundary; the ray starts in the pixel
// it heads into). Only +x rays are needed. Returns the pixel, its emittance and the point where the ray enters it.
fn trace(scene: &PixelScene, origin: Vec2, dir: Vec2) -> Option<(IVec2, f32, Vec2)> {
    assert!(dir.x > 0.0);
    let step_y = if dir.y < 0.0 { -1 } else { 1 };
    let mut cell = origin.floor().as_ivec2();
    if dir.y < 0.0 && origin.y == origin.y.floor() {
        cell.y -= 1;
    }
    let mut t = 0.0f32;
    for _ in 0..600 {
        if let Some(&e) = scene.get(&cell) {
            return Some((cell, e, origin + dir * t));
        }
        let tx = ((cell.x + 1) as f32 - origin.x) / dir.x;
        let ty = if dir.y == 0.0 {
            f32::INFINITY
        } else {
            ((cell.y + (step_y + 1) / 2) as f32 - origin.y) / dir.y
        };
        if tx <= ty {
            cell.x += 1;
            t = tx;
        } else {
            cell.y += step_y;
            t = ty;
        }
    }
    None
}

// Exact value of a cone frustum at its focus: the radiance arriving from the directions between its edges, from what
// lies at or beyond its t = 1 column (what is nearer is the parent's business), with the renderer's convention of
// emittance / 2 per face. `keep` filters the hit points (in the frame of the scene), to restrict to a region.
pub fn truth_filtered(scene: &PixelScene, cone: ConeFrustum, keep: impl Fn(Vec2) -> bool) -> f32 {
    let n = 2000;
    let focus = cone.focus.as_vec2();
    let (a0, a1) = (
        (cone.index as f32 / cone.size as f32).atan(),
        ((cone.index + 1) as f32 / cone.size as f32).atan(),
    );
    let step = (a1 - a0) / n as f32;
    let mut total = 0.0;
    for j in 0..n {
        let dir = Vec2::from_angle(a0 + (j as f32 + 0.5) * step);
        let origin = focus + dir * (cone.size as f32 / dir.x);
        // a ray starting inside a solid pixel is blocked, and that pixel's face belongs to whoever is nearer
        if let Some((_, e, hit)) = trace(scene, origin, dir)
            && hit != origin
            && keep(hit)
        {
            total += e * 0.5 * step;
        }
    }
    total
}

pub fn truth(scene: &PixelScene, cone: ConeFrustum) -> f32 {
    truth_filtered(scene, cone, |_| true)
}

fn cone_of(f: Frustum) -> ConeFrustum {
    let Frustum::Cone(c) = f else { unreachable!() };
    c
}

#[test]
fn split() {
    let y: i32 = env("Y", 100);
    let (x0, x1): (i32, i32) = (env("X0", 62), env("X1", 76));
    let base = base_scene();
    println!("row {y}: per (k, i): ext / exact, only where either is nonzero");
    let mut caches: HashMap<(usize, i32), (ExtCache, Cache)> = HashMap::new();
    for x in x0..x1 {
        let mut line = format!("x={x}:");
        let (mut te, mut tt) = (0.0, 0.0);
        for k in 0..4 {
            let scene = rotated(&base, k);
            let focus = probe_of(IVec2::new(x, y), k);
            for i in [-1, 0] {
                let cone = ConeFrustum { focus, size: 1, index: i };
                if scene.contains_key(&(focus + IVec2::new(0, i))) {
                    continue;
                }
                let (ec, c) = caches.entry((k, i)).or_default();
                let e = get_radiance_recursive_ext::<L, I>(ec, c, cone.into(), &scene, SETTINGS).at_focus(cone.into());
                let t = truth(&scene, cone);
                te += e;
                tt += t;
                if e.abs() > 1e-5 || t.abs() > 1e-5 {
                    line += &format!("  k{k} i{i}: {e:.4}/{t:.4}");
                }
            }
        }
        println!("{line}   total {te:.4}/{tt:.4}");
    }
}

struct Ctx<'a> {
    scene: &'a PixelScene,
    ext_cache: ExtCache,
    cache: Cache,
    tol: f32,
    max_depth: usize,
}

fn value(ctx: &mut Ctx, f: Frustum) -> I {
    get_radiance_recursive_ext::<L, I>(&mut ctx.ext_cache, &mut ctx.cache, f, ctx.scene, SETTINGS)
}

// Prints the frustum's terms, read at the frustum's own focus, next to exact values, then recurses into the children
// whose own value is wrong.
fn dump(ctx: &mut Ctx, frustum: Frustum, depth: usize) {
    let c = cone_of(frustum);
    if c.size > 128 || depth > ctx.max_depth {
        return;
    }
    let pad = "  ".repeat(depth);
    let v = value(ctx, frustum).at_focus(frustum);
    let t = truth(ctx.scene, c);
    if (v - t).abs() <= ctx.tol {
        return;
    }
    let view_dir = if c.index >= 0 { ViewDirection::Up } else { ViewDirection::Down };
    let start = c.focus + c.lower_dir();
    let parity = frustum.parity();
    println!(
        "{pad}[s{} j{} @{},{} {}] value {v:.4} exact {t:.4}",
        c.size,
        c.index,
        c.focus.x,
        c.focus.y,
        if parity == 0 { "even" } else { "odd" }
    );
    let scene = ctx.scene;
    let ext_at = |ctx: &mut Ctx, ext: Extension| {
        let fl = get_extension(&mut ctx.ext_cache, ext, scene, view_dir);
        (I::resample(fl.radiance, ext.as_frustum(), frustum).at_focus(frustum), fl.transmittance)
    };
    let line_t = |ctx: &mut Ctx, child: ConeFrustum| {
        get_extension(
            &mut ctx.ext_cache,
            LineExtension { start, end: child.focus + child.lower_dir() }.into(),
            scene,
            view_dir,
        )
        .transmittance
    };
    // exact: what the frustum's own focus receives through the child's angular range from beyond the child's start
    // column (occlusion nearer than that column ignored), and the same with occlusion from the frustum's start column
    let exact_for = |child: ConeFrustum| {
        let child_start_x = (child.focus.x + child.size) as f32;
        // rays from the parent's focus, starting at the parent's start, kept if they hit beyond the child's start
        let own = truth_filtered(scene, c, |hit| hit.x > child_start_x);
        own
    };
    let mut children = vec![];
    if parity == 0 {
        let near = ConeExtension { start, end_bottom: start + c.lower_dir() };
        let (nv, nt) = ext_at(ctx, near.into());
        let x_end = (c.focus.x + 2 * c.size) as f32;
        let near_exact = truth_filtered(scene, c, |hit| hit.x <= x_end);
        println!("{pad}  near cone ext: {nv:.4} (T {nt:.3})   exact near {near_exact:.4}");
        for (f, _) in merge_even(frustum) {
            let cf = cone_of(f);
            let cv = value(ctx, f);
            let lt = line_t(ctx, cf);
            println!(
                "{pad}  far child s{} j{} @{},{}: own {:.4} (exact {:.4}) reprojected {:.4} x lineT {lt:.3}",
                cf.size,
                cf.index,
                cf.focus.x,
                cf.focus.y,
                cv.at_focus(f),
                truth(scene, cf),
                cv.reproject(f, frustum).at_focus(frustum),
            );
            children.push(f);
        }
        println!("{pad}  exact far (both children, with occlusion from start): {:.4}", t - near_exact);
    } else {
        let lower_end = start + c.lower_dir() * 2;
        let middle_end = start + c.lower_dir() + c.upper_dir();
        let (bv, bt) = ext_at(ctx, ConeExtension { start, end_bottom: lower_end }.into());
        let (tv, tt) = ext_at(ctx, ConeExtension { start, end_bottom: middle_end }.into());
        let (mv, mt) = ext_at(ctx, LineExtension { start, end: middle_end }.into());
        let x_end = (c.focus.x + 3 * c.size) as f32;
        let near_exact = truth_filtered(scene, c, |hit| hit.x <= x_end);
        let mut short = 0.0;
        for (f, _) in merge_odd(frustum, false, true) {
            let cf = cone_of(f);
            let cv = value(ctx, f);
            let r = cv.reproject(f, frustum).at_focus(frustum);
            short += r;
            println!(
                "{pad}  short child s{} j{} @{},{}: own {:.4} (exact {:.4}) reprojected {r:.4}",
                cf.size,
                cf.index,
                cf.focus.x,
                cf.focus.y,
                cv.at_focus(f),
                truth(scene, cf),
            );
            children.push(f);
        }
        println!(
            "{pad}  long near: B {bv:.4} (T {bt:.3})  T {tv:.4} (T {tt:.3})  M {mv:.4} (T {mt:.3})  B+T-2M {:.4}   exact near (cols s..3s) {near_exact:.4}",
            bv + tv - 2.0 * mv
        );
        let mut long = bv + tv - 2.0 * mv;
        for (f, _) in merge_odd(frustum, true, true) {
            let cf = cone_of(f);
            let cv = value(ctx, f);
            let lt = line_t(ctx, cf);
            let r = cv.reproject(f, frustum).at_focus(frustum);
            long += r * lt;
            println!(
                "{pad}  long child s{} j{} @{},{}: own {:.4} (exact {:.4}) reprojected {r:.4} x lineT {lt:.3}",
                cf.size,
                cf.index,
                cf.focus.x,
                cf.focus.y,
                cv.at_focus(f),
                truth(scene, cf),
            );
            children.push(f);
        }
        let _ = exact_for;
        println!(
            "{pad}  short {short:.4}  long {long:.4}  -> {:.4}   exact far beyond 3s {:.4}",
            0.5 * (short + long),
            t - near_exact
        );
    }
    for f in children {
        dump(ctx, f, depth + 1);
    }
}

#[test]
fn tree() {
    let (x, y): (i32, i32) = (env("X", 68), env("Y", 100));
    let base = base_scene();
    // by default every (k, i) with a visible error
    let ks: Vec<usize> = std::env::var("K").ok().map(|k| vec![k.parse().unwrap()]).unwrap_or((0..4).collect());
    let is: Vec<i32> = std::env::var("I").ok().map(|i| vec![i.parse().unwrap()]).unwrap_or(vec![-1, 0]);
    for k in ks {
        let scene = rotated(&base, k);
        let focus = probe_of(IVec2::new(x, y), k);
        for &i in &is {
            let cone = ConeFrustum { focus, size: 1, index: i };
            if scene.contains_key(&(focus + IVec2::new(0, i))) {
                continue;
            }
            let mut ctx = Ctx {
                scene: &scene,
                ext_cache: ExtCache::new(),
                cache: Cache::new(),
                tol: 0.0,
                max_depth: env("DEPTH", 8),
            };
            let t = truth(&scene, cone);
            let v = value(&mut ctx, cone.into()).at_focus(cone.into());
            ctx.tol = env("TOL", 0.1) * t.abs().max(v.abs()).max(1e-4);
            println!("=== cell ({x},{y}) k{k} i{i} probe {focus}: ext {v:.4} exact {t:.4}");
            dump(&mut ctx, cone.into(), 0);
        }
    }
}

// ---- inside one extension ----

// the sub-extensions get_extension composes, in its order (first, then the joined seconds)
fn sub_extensions(ext: Extension) -> Vec<Extension> {
    match ext {
        Extension::Line(l) => l.extend().into_iter().map(Extension::Line).collect(),
        Extension::Cone(c) => {
            let d = c.end_bottom - c.start;
            let h = d / 2;
            let top = c.end_bottom + IVec2::Y;
            if (d.y >= 0) == (d.y % 2 == 0) {
                vec![
                    LineExtension { start: c.start, end: c.start + h }.into(),
                    LineExtension { start: c.end_bottom - h, end: c.end_bottom }.into(),
                    LineExtension { start: top - h, end: top }.into(),
                ]
            } else {
                vec![
                    ConeExtension { start: c.start, end_bottom: c.start + h }.into(),
                    ConeExtension { start: c.end_bottom - h, end_bottom: c.end_bottom }.into(),
                ]
            }
        }
    }
}

fn describe(ext: Extension) -> String {
    match ext {
        Extension::Line(l) => format!("line {}..{} d{}", l.start, l.end, l.end - l.start),
        Extension::Cone(c) => format!("cone {}..{} d{}", c.start, c.end_bottom, c.end_bottom - c.start),
    }
}

// Exact fluence of an extension: over the rays of its own frustum (a cone's from its focus, uniform in slope; a
// line's parallel, uniform across its width), starting at its first column, the fraction that leaves its last column
// unblocked, and the radiance its focus receives from faces met within it (cone only).
fn exact_extension(scene: &PixelScene, ext: Extension) -> (f32, f32) {
    exact_extension_n(scene, ext, 2000)
}

fn exact_extension_n(scene: &PixelScene, ext: Extension, n: usize) -> (f32, f32) {
    let size = ext.size() as f32;
    let x_end = (ext.start().x + ext.size()) as f32;
    let (mut open, mut radiance) = (0, 0.0);
    for j in 0..n {
        let u = (j as f32 + 0.5) / n as f32;
        let (origin, dir, weight) = match ext.as_frustum() {
            Frustum::Cone(c) => {
                let dir = Vec2::new(size, c.index as f32 + u);
                let d0 = Vec2::new(size, c.index as f32).to_angle();
                let d1 = Vec2::new(size, c.index as f32 + 1.0).to_angle();
                (c.focus.as_vec2() + dir, dir, (d1 - d0) / n as f32)
            }
            Frustum::Linear(l) => (l.focus + Vec2::Y * (u - 0.5) + l.dir(), l.dir(), 0.0),
        };
        match trace(scene, origin, dir) {
            Some((_, e, hit)) if hit.x <= x_end && hit != origin => radiance += e * 0.5 * weight,
            Some((_, _, hit)) if hit.x < x_end || hit == origin => {}
            _ => open += 1,
        }
        if let Some((_, _, hit)) = trace(scene, origin, dir)
            && hit.x == x_end
            && hit != origin
        {
            // the face on the last column's far side is seen, but does not block the extension itself
            open += 1;
        }
    }
    (radiance, open as f32 / n as f32)
}

fn dump_extension(
    cache: &mut ExtCache,
    scene: &PixelScene,
    view_dir: ViewDirection,
    ext: Extension,
    root: Extension,
    depth: usize,
    max_depth: usize,
) {
    let fl = get_extension(cache, ext, scene, view_dir);
    let r = fl.reproject(ext.as_frustum(), root.as_frustum()).radiance.at_focus(root.as_frustum());
    let (er, et) = exact_extension(scene, ext);
    if fl.transmittance == 1.0 && r == 0.0 && et == 1.0 {
        return;
    }
    println!(
        "{}{}: T {:.3} (exact {et:.3})  radiance at root focus {r:.4}{}",
        "  ".repeat(depth),
        describe(ext),
        fl.transmittance,
        if matches!(ext, Extension::Cone(_)) { format!(" (exact at own focus {er:.4})") } else { String::new() }
    );
    if ext.size() > 1 && depth < max_depth {
        for sub in sub_extensions(ext) {
            dump_extension(cache, scene, view_dir, sub, root, depth + 1, max_depth);
        }
    }
}

// EXT="cone|line,startx,starty,dx,dy" in the frame of rotation K (default 1)
#[test]
fn inside() {
    let spec = env("EXT", "cone,64,51,64,-30".to_string());
    let p: Vec<&str> = spec.split(',').collect();
    let n = |i: usize| p[i].parse::<i32>().unwrap();
    let (start, d) = (IVec2::new(n(1), n(2)), IVec2::new(n(3), n(4)));
    let ext: Extension = if p[0] == "cone" {
        ConeExtension { start, end_bottom: start + d }.into()
    } else {
        LineExtension { start, end: start + d }.into()
    };
    let view_dir = if d.y >= 0 { ViewDirection::Up } else { ViewDirection::Down };
    let scene = rotated(&base_scene(), env("K", 1));
    let mut cache = ExtCache::new();
    dump_extension(&mut cache, &scene, view_dir, ext, ext, 0, env("DEPTH", 8));
}

// ---- linear decomposition of a root value into leaf extension terms ----

struct Term {
    path: String,
    ext: Extension,
    // product of the merge weights and line transmittances on the way down
    weight: f32,
    // the term's contribution, reprojected up the chain to the root
    value: I,
}

fn walk(ext_cache: &mut ExtCache, frustum: Frustum, scene: &PixelScene) -> Vec<Term> {
    let c = cone_of(frustum);
    if c.size > 128 {
        return vec![];
    }
    let view_dir = if c.index >= 0 { ViewDirection::Up } else { ViewDirection::Down };
    let start = c.focus + c.lower_dir();
    let mut out = vec![];
    let mut leaf = |ext_cache: &mut ExtCache, ext: Extension, w: f32, tag: &str| {
        let r = get_extension(ext_cache, ext, scene, view_dir).radiance;
        out.push(Term {
            path: tag.to_string(),
            ext,
            weight: w,
            value: I::resample(r, ext.as_frustum(), frustum) * w,
        });
    };
    let mut kids: Vec<(Frustum, f32, bool, &str)> = vec![];
    if frustum.parity() == 0 {
        leaf(ext_cache, ConeExtension { start, end_bottom: start + c.lower_dir() }.into(), 1.0, "near");
        for (n, (f, _)) in merge_even(frustum).into_iter().enumerate() {
            kids.push((f, 1.0, true, ["E0", "E1"][n]));
        }
    } else {
        let lower_end = start + c.lower_dir() * 2;
        let middle_end = start + c.lower_dir() + c.upper_dir();
        leaf(ext_cache, ConeExtension { start, end_bottom: lower_end }.into(), 0.5, "B");
        leaf(ext_cache, ConeExtension { start, end_bottom: middle_end }.into(), 0.5, "T");
        leaf(ext_cache, LineExtension { start, end: middle_end }.into(), -1.0, "M");
        for (n, (f, _)) in merge_odd(frustum, false, true).into_iter().enumerate() {
            kids.push((f, 0.5, false, ["S0", "S1"][n]));
        }
        for (n, (f, _)) in merge_odd(frustum, true, true).into_iter().enumerate() {
            kids.push((f, 0.5, true, ["L0", "L1"][n]));
        }
    }
    for (f, w, with_t, tag) in kids {
        let cf = cone_of(f);
        let t = if with_t {
            get_extension(ext_cache, LineExtension { start, end: cf.focus + cf.lower_dir() }.into(), scene, view_dir)
                .transmittance
        } else {
            1.0
        };
        for term in walk(ext_cache, f, scene) {
            out.push(Term {
                path: format!("{tag}>{}", term.path),
                ext: term.ext,
                weight: term.weight * w * t,
                value: term.value.reproject(f, frustum) * (w * t),
            });
        }
    }
    out
}

// Per leaf term of one probe's (K, I) cone: weight and value with the wall next to without it. Terms are listed when
// they contribute with or without the wall; `vis` is the exact fraction of the term's no-wall light that the wall
// leaves visible from the probe (rays from the probe to the faces the term's extension sees, which is what the
// reference does), so value_wall should be about value_nowall * vis.
#[test]
fn terms() {
    let (x, y): (i32, i32) = (env("X", 68), env("Y", 100));
    let (k, i): (usize, i32) = (env("K", 1), env("I", -1));
    let wall = rotated(&base_scene(), k);
    let nowall: PixelScene = wall.iter().filter(|&(_, &e)| e > 0.0).map(|(p, e)| (*p, *e)).collect();
    let focus = probe_of(IVec2::new(x, y), k);
    let root: Frustum = ConeFrustum { focus, size: 1, index: i }.into();
    let (mut cw, mut cn) = (ExtCache::new(), ExtCache::new());
    let (tw, tn) = (walk(&mut cw, root, &wall), walk(&mut cn, root, &nowall));
    let sum = |t: &[Term]| t.iter().map(|t| t.value.at_focus(root)).sum::<f32>();
    println!(
        "cell ({x},{y}) k{k} i{i} probe {focus}: sum of terms wall {:.4} nowall {:.4}; exact wall {:.4} nowall {:.4}",
        sum(&tw),
        sum(&tn),
        truth(&wall, cone_of(root)),
        truth(&nowall, cone_of(root))
    );
    // merge identical extensions reached along different paths only in the per-path listing; print per path
    let mut rows: Vec<(f32, String)> = vec![];
    let (mut gain, mut loss) = (0.0, 0.0);
    for (a, b) in tw.iter().zip(&tn) {
        assert_eq!(a.ext, b.ext);
        let (va, vb) = (a.value.at_focus(root), b.value.at_focus(root));
        if va.abs() < 2e-5 && vb.abs() < 2e-5 {
            continue;
        }
        if va.abs() > vb.abs() {
            gain += va - vb;
        } else {
            loss += va - vb;
        }
        rows.push((
            (va - vb).abs().max(va.abs()),
            format!(
                "wall {va:+.4} (w {:+.4})  nowall {vb:+.4} (w {:+.4})  {}  via {}",
                a.weight,
                b.weight,
                describe(a.ext),
                a.path
            ),
        ));
    }
    rows.sort_by(|a, b| b.0.total_cmp(&a.0));
    for (_, r) in rows.iter().take(env("TOP", 60)) {
        println!("{r}");
    }
    println!("{} terms; wall - nowall: gains {gain:+.4} losses {loss:+.4}", rows.len());
}

// Transmittance of line extensions that straddle the end of the wall (frame k = 1: wall = column 67, rows 0..=49),
// against the exact fraction of the parallel band that passes. Per size and sign of the slope: number of straddling
// lines, mean exact T, mean extension T, and how many are fully open / fully closed in the extension.
#[test]
fn line_bias() {
    let scene = rotated(&base_scene(), 1);
    let nolight: PixelScene = scene.iter().filter(|&(_, &e)| e == 0.0).map(|(p, e)| (*p, *e)).collect();
    for size in [1, 2, 4, 8, 16, 32, 64] {
        for sign in [-1, 1] {
            let view_dir = if sign > 0 { ViewDirection::Up } else { ViewDirection::Down };
            let mut cache = ExtCache::new();
            let (mut n, mut exact_sum, mut ext_sum, mut open, mut closed) = (0, 0.0, 0.0, 0, 0);
            let (mut worst, mut worst_ext) = (0.0f32, None);
            for dy in 1..=size {
                let d = IVec2::new(size, dy * sign);
                for sx in (68 - size)..=67 {
                    // the rows from which the band can touch the wall's last row at column 67
                    let at_wall = d.y * (67 - sx) / size;
                    for sy in (49 - at_wall - 3)..=(49 - at_wall + 3) {
                        let ext: Extension = LineExtension { start: IVec2::new(sx, sy), end: IVec2::new(sx, sy) + d }.into();
                        let (_, et) = exact_extension_n(&nolight, ext, 200);
                        if et <= 0.001 || et >= 0.999 {
                            continue;
                        }
                        let t = get_extension(&mut cache, ext, &nolight, view_dir).transmittance;
                        n += 1;
                        exact_sum += et;
                        ext_sum += t;
                        open += (t == 1.0) as i32;
                        closed += (t == 0.0) as i32;
                        if (t - et).abs() > worst {
                            worst = (t - et).abs();
                            worst_ext = Some(ext);
                        }
                    }
                }
            }
            if n > 0 {
                println!(
                    "size {size:2} slope sign {sign:+}: {n:5} straddling lines, exact T mean {:.3}, ext T mean {:.3} (open {open}, closed {closed}), worst |err| {worst:.3} at {}",
                    exact_sum / n as f32,
                    ext_sum / n as f32,
                    worst_ext.map(describe).unwrap_or_default()
                );
            }
        }
    }
}

// ---- oracle experiment: the merge of render/ext.rs fed with exactly traced extension fluences ----

// Exact fluence of an extension over its own frustum's rays: mean transmittance, and every face met inside it added
// at the point where it is hit, weighted by the length of face each ray stands for. Occlusion inside the extension is
// exact per ray, so only the merge's own approximations remain.
fn oracle_extension(scene: &PixelScene, ext: Extension, view_dir: ViewDirection, n: usize) -> Fluence<L> {
    use twilight::radiance::ViewDir;
    let f = ext.as_frustum();
    let size = ext.size() as f32;
    let x0 = ext.start().x as f32;
    let x_end = x0 + size;
    let mut open = 0;
    let mut radiance = L::default();
    for j in 0..n {
        let u = (j as f32 + 0.5) / n as f32;
        let (origin, dir, cone) = match f {
            Frustum::Cone(c) => {
                let dir = Vec2::new(size, c.index as f32 + u);
                (c.focus.as_vec2() + dir, dir, true)
            }
            Frustum::Linear(l) => (l.focus + Vec2::Y * (u - 0.5) + l.dir(), l.dir(), false),
        };
        let Some((cell, e, hit)) = trace(scene, origin, dir) else {
            open += 1;
            continue;
        };
        if hit == origin {
            continue;
        }
        if hit.x > x_end {
            open += 1;
            continue;
        }
        if hit.x == x_end {
            open += 1;
        }
        if e == 0.0 {
            continue;
        }
        let width = if cone { 1.0 + (hit.x - x0) / size } else { 1.0 };
        let vertical = hit.x == cell.x as f32;
        let (face_dir, weight) = if vertical {
            (ViewDir::Right, width / n as f32)
        } else {
            (
                if view_dir == ViewDirection::Up { ViewDir::Up } else { ViewDir::Down },
                width / n as f32 / (dir.y / dir.x).abs(),
            )
        };
        radiance = radiance + L::new_directional(f, hit, face_dir) * (e * 0.5 * weight);
    }
    Fluence::new(open as f32 / n as f32, radiance)
}

// get_radiance_recursive_ext with the extension lookup replaced by `ext_fn`
fn merge_with(
    ext_fn: &mut dyn FnMut(Extension, ViewDirection) -> Fluence<L>,
    cache: &mut Cache,
    frustum: Frustum,
) -> I {
    if let Some(&v) = cache.get(&frustum) {
        return v;
    }
    let cone = cone_of(frustum);
    if cone.size > 128 {
        return I::default();
    }
    let view_dir = if cone.index >= 0 { ViewDirection::Up } else { ViewDirection::Down };
    let start = cone.focus + cone.lower_dir();
    let far = |ext_fn: &mut dyn FnMut(Extension, ViewDirection) -> Fluence<L>,
                   cache: &mut Cache,
                   merged: Vec<(Frustum, f32)>,
                   with_t: bool| {
        let mut total = I::default();
        for (f, _) in merged {
            let c = cone_of(f);
            let t = if with_t {
                ext_fn(LineExtension { start, end: c.focus + c.lower_dir() }.into(), view_dir).transmittance
            } else {
                1.0
            };
            total = total + merge_with(ext_fn, cache, f).reproject(f, frustum) * t;
        }
        total
    };
    let result = if frustum.parity() == 0 {
        let near_ext: Extension = ConeExtension { start, end_bottom: start + cone.lower_dir() }.into();
        let near = ext_fn(near_ext, view_dir).resample::<I>(near_ext.as_frustum(), frustum);
        near.radiance + far(ext_fn, cache, merge_even(frustum), true)
    } else {
        let short = far(ext_fn, cache, merge_odd(frustum, false, true), false);
        let lower_end = start + cone.lower_dir() * 2;
        let middle_end = start + cone.lower_dir() + cone.upper_dir();
        let exts: [Extension; 3] = [
            ConeExtension { start, end_bottom: lower_end }.into(),
            ConeExtension { start, end_bottom: middle_end }.into(),
            LineExtension { start, end: middle_end }.into(),
        ];
        let [b, t, m] = exts.map(|e| ext_fn(e, view_dir).resample::<I>(e.as_frustum(), frustum).radiance);
        let long = b + t + m * -2.0 + far(ext_fn, cache, merge_odd(frustum, true, true), true);
        short * 0.5 + long * 0.5
    };
    cache.insert(frustum, result);
    result
}

// Row profile of the (K, I) cone: renderer's extensions, oracle extensions (same merge), exact. If the oracle column
// matches exact, the artifact comes from get_extension alone; what remains is the merge's share.
#[test]
fn oracle() {
    let y: i32 = env("Y", 100);
    let (x0, x1): (i32, i32) = (env("X0", 60), env("X1", 78));
    let (k, i): (usize, i32) = (env("K", 1), env("I", -1));
    let rays: usize = env("RAYS", 1024);
    let scene = rotated(&base_scene(), k);
    let mut real_cache = ExtCache::new();
    let mut oracle_cache = ExtCache::new();
    let (mut c1, mut c2, mut c3) = (Cache::new(), Cache::new(), Cache::new());
    println!("row {y} k{k} i{i}:   x   ext    merge-copy  oracle-ext  exact");
    for x in x0..x1 {
        let focus = probe_of(IVec2::new(x, y), k);
        let root: Frustum = ConeFrustum { focus, size: 1, index: i }.into();
        let ext = get_radiance_recursive_ext::<L, I>(&mut real_cache, &mut c1, root, &scene, SETTINGS).at_focus(root);
        let mut real_cache2 = std::mem::take(&mut real_cache);
        let copy = merge_with(&mut |e, v| get_extension(&mut real_cache2, e, &scene, v), &mut c2, root).at_focus(root);
        real_cache = real_cache2;
        let orc = merge_with(
            &mut |e, v| *oracle_cache.entry(e).or_insert_with(|| oracle_extension(&scene, e, v, rays)),
            &mut c3,
            root,
        )
        .at_focus(root);
        println!("  {x:3}  {ext:.4}  {copy:.4}  {orc:.4}  {:.4}", truth(&scene, cone_of(root)));
    }
}

// ---- which part of get_extension matters: test-local variants of its recursion ----
//
// mode 0: replica of get_extension (sanity check against the renderer)
// mode 2: every line extension of size > 1 is the exact band (transmittance and emission traced); cones keep
//         get_extension's structure (line over two joined lines, or cone over cone)
// mode 3: lines keep the renderer's pixel assignment for emission, but each line's transmittance is the exact
//         fraction of its band that passes
// mode 4: like 3, but the transmittance of a line with odd dy is the average over its two digital paths (lower half
//         then upper half, upper half then lower half) - no tracing, 2 more lookups per odd line
struct Variant<'a> {
    mode: i32,
    scene: &'a PixelScene,
    cache: ExtCache,
    real: ExtCache,
    rays: usize,
}

impl Variant<'_> {
    fn get(&mut self, ext: Extension, view_dir: ViewDirection) -> Fluence<L> {
        if ext.size() == 1 {
            return get_extension(&mut self.real, ext, self.scene, view_dir);
        }
        if let Some(&v) = self.cache.get(&ext) {
            return v;
        }
        if self.mode == 2 && matches!(ext, Extension::Line(_)) {
            let result = oracle_extension(self.scene, ext, view_dir, self.rays);
            self.cache.insert(ext, result);
            return result;
        }
        let f = ext.as_frustum();
        let subs: Vec<Fluence<L>> =
            sub_extensions(ext).into_iter().map(|s| self.get(s, view_dir).reproject(s.as_frustum(), f)).collect();
        let mut result = match ext {
            Extension::Line(_) => subs[0].over(subs[1]),
            Extension::Cone(_) if subs.len() == 3 => subs[0].over(Fluence::join([subs[1], subs[2]])),
            Extension::Cone(_) => subs[0].over(subs[1]),
        };
        if let Extension::Line(line) = ext {
            match self.mode {
                3 => result.transmittance = oracle_extension(self.scene, ext, view_dir, self.rays).transmittance,
                4 => {
                    let d = line.end - line.start;
                    if d.y % 2 != 0 {
                        let lo = IVec2::new(d.x / 2, d.y.div_euclid(2));
                        let hi = lo + IVec2::Y;
                        let mut t = 0.0;
                        for a in [lo, hi] {
                            let first = self.get(LineExtension { start: line.start, end: line.start + a }.into(), view_dir);
                            let second = self.get(LineExtension { start: line.start + a, end: line.end }.into(), view_dir);
                            t += 0.5 * first.transmittance * second.transmittance;
                        }
                        result.transmittance = t;
                    }
                }
                _ => {}
            }
        }
        self.cache.insert(ext, result);
        result
    }
}

// Row profile of the (K, I) cone for each MODES entry (default 0,3,4,2), next to the renderer and the exact value.
#[test]
fn variants() {
    let y: i32 = env("Y", 100);
    let (x0, x1): (i32, i32) = (env("X0", 62), env("X1", 78));
    let (k, i): (usize, i32) = (env("K", 1), env("I", -1));
    let modes: Vec<i32> = env("MODES", "0,3,4,2".to_string()).split(',').map(|m| m.parse().unwrap()).collect();
    let scene = rotated(&base_scene(), k);
    let mut variants: Vec<(Variant, Cache)> = modes
        .iter()
        .map(|&mode| {
            (Variant { mode, scene: &scene, cache: ExtCache::new(), real: ExtCache::new(), rays: env("RAYS", 256) }, Cache::new())
        })
        .collect();
    let (mut ec, mut c) = (ExtCache::new(), Cache::new());
    println!("row {y} k{k} i{i}:   x   ext   {}  exact", modes.iter().map(|m| format!("mode{m}  ")).collect::<String>());
    for x in x0..x1 {
        let focus = probe_of(IVec2::new(x, y), k);
        let root: Frustum = ConeFrustum { focus, size: 1, index: i }.into();
        let ext = get_radiance_recursive_ext::<L, I>(&mut ec, &mut c, root, &scene, SETTINGS).at_focus(root);
        let mut line = format!("  {x:3}  {ext:.4}");
        for (variant, cache) in &mut variants {
            let v = merge_with(&mut |e, vd| variant.get(e, vd), cache, root).at_focus(root);
            line += &format!("  {v:.4}");
        }
        println!("{line}  {:.4}", truth(&scene, cone_of(root)));
    }
}
