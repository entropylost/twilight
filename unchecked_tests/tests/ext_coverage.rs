// Renders how much of a single emitting pixel each probe sees through the extension renderer, using a radiance that
// ignores distance. The probe's cones partition the plane, so ideally every probe sees the pixel exactly once.

use std::ops::{Add, Mul};

use glam::{IVec2, Vec2};
use image::{GrayImage, Luma};
use twilight::extension::PixelScene;
use twilight::frustum::Frustum;
use twilight::radiance::{Radiance, ResampleRadiance};
use twilight::render::ext::render_ext;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Count {
    value: f32,
}
impl Radiance for Count {
    fn read(self, _source: Frustum, _target: Vec2) -> f32 {
        self.value
    }
    fn new(_frustum: Frustum, _emitter: Vec2) -> Self {
        Self { value: 1.0 }
    }
    fn reproject(self, _source: Frustum, _target: Frustum) -> Self {
        self
    }
}
impl ResampleRadiance<Count> for Count {
    fn resample(original: Count, _source: Frustum, _target: Frustum) -> Self {
        original
    }
}
impl Add for Count {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self { value: self.value + other.value }
    }
}
impl Mul<f32> for Count {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self { value: self.value * rhs }
    }
}

#[test]
fn ext_coverage() {
    let emitter = IVec2::new(36, 32);
    let mut scene = PixelScene::new();
    scene.insert(emitter, 1.0);
    let image = render_ext::<Count, Count>(&scene);

    let scale = 4;
    let png = GrayImage::from_fn(128 * scale, 128 * scale, |x, y| {
        let cell = IVec2::new((x / scale) as i32, (y / scale) as i32);
        let v = image.get(&cell).copied().unwrap_or(0.0);
        // 1 maps to mid grey so over- and under-coverage are both visible
        Luma([(v * 0.5).clamp(0.0, 1.0).mul(255.0).round() as u8])
    });
    std::fs::create_dir_all("out").unwrap();
    png.save("out/ext_coverage.png").unwrap();

    let mut values: Vec<f32> =
        image.iter().filter(|(c, _)| (**c - emitter).abs().max_element() > 2).map(|(_, v)| *v).collect();
    values.sort_by(f32::total_cmp);
    let n = values.len();
    println!(
        "coverage away from emitter: min {} p5 {} median {} p95 {} max {}",
        values[0],
        values[n / 20],
        values[n / 2],
        values[n * 19 / 20],
        values[n - 1]
    );
    for y in (0..128).step_by(4) {
        let row: String = (0..128)
            .step_by(2)
            .map(|x| {
                let v = image.get(&IVec2::new(x, y)).copied().unwrap_or(0.0);
                match v {
                    v if v < 0.05 => ' ',
                    v if v < 0.5 => '.',
                    v if v < 0.9 => '-',
                    v if v < 1.1 => 'o',
                    v if v < 1.5 => '+',
                    _ => '#',
                }
            })
            .collect();
        println!("{row}");
    }
}

// The real carriers, divided by the exact 1 / distance from the probe to the emitting pixel's centre.
fn ratio_report<L: Radiance, I: Radiance + ResampleRadiance<L>>(name: &str) {
    let emitter = IVec2::new(36, 32);
    let mut scene = PixelScene::new();
    scene.insert(emitter, 1.0);
    let image = render_ext::<L, I>(&scene);
    let ratio = |cell: IVec2| {
        let v = image.get(&cell).copied().unwrap_or(0.0);
        v * (cell.as_vec2() - emitter.as_vec2() - 0.5).length()
    };
    let scale = 4;
    let png = GrayImage::from_fn(128 * scale, 128 * scale, |x, y| {
        let cell = IVec2::new((x / scale) as i32, (y / scale) as i32);
        Luma([(ratio(cell) * 0.5).clamp(0.0, 1.0).mul(255.0).round() as u8])
    });
    png.save(format!("out/ext_ratio_{name}.png")).unwrap();
    let mut values: Vec<f32> =
        image.keys().filter(|c| (**c - emitter).abs().max_element() > 2).map(|c| ratio(*c)).collect();
    values.sort_by(f32::total_cmp);
    let n = values.len();
    println!(
        "{name}: ratio min {} p5 {} median {} p95 {} max {}",
        values[0],
        values[n / 20],
        values[n / 2],
        values[n * 19 / 20],
        values[n - 1]
    );
}

#[test]
fn ext_ratio() {
    use twilight::radiance::{AverageDepth, Constant, Inverse2};
    std::fs::create_dir_all("out").unwrap();
    ratio_report::<AverageDepth, Inverse2>("avgpos_inverse2");
    ratio_report::<Inverse2, Inverse2>("inverse2_inverse2");
    ratio_report::<AverageDepth, Constant>("avgpos_constant");
}

#[test]
fn ext_coverage_errors() {
    let emitter = IVec2::new(36, 32);
    let mut scene = PixelScene::new();
    scene.insert(emitter, 1.0);
    let image = render_ext::<Count, Count>(&scene);
    let mut bad: Vec<(IVec2, f32)> = image
        .iter()
        .filter(|(c, v)| (**c - emitter).abs().max_element() > 2 && (**v - 1.0).abs() > 0.01)
        .map(|(c, v)| (*c - emitter, *v))
        .collect();
    bad.sort_by_key(|(c, _)| (c.abs().max_element(), c.x, c.y));
    println!("{} bad probes (offsets from the emitting pixel)", bad.len());
    for (c, v) in bad.iter().take(60) {
        println!("{c} {v}");
    }
}

// The same ratio for the analytic renderer, as a baseline.
#[test]
fn noext_ratio() {
    use twilight::render::{DEFAULT_FRUSTUM_START, MergeMode, RadianceKind, RenderSettings};
    use twilight::scene::{Emitter, Scene};
    std::fs::create_dir_all("out").unwrap();
    let emitter = IVec2::new(36, 32);
    let scene =
        Scene { occluders: vec![], emitters: vec![Emitter { position: emitter.as_vec2() + 0.5, brightness: 1.0 }] };
    let settings = RenderSettings {
        frustum_start: DEFAULT_FRUSTUM_START,
        skip_linear: false,
        pretrace: false,
        merge_mode: MergeMode::Both,
    };
    for kind in [RadianceKind::AveragePosition, RadianceKind::Inverse2, RadianceKind::QuadraticProjected] {
        let image = kind.render(&scene, settings);
        let ratio = |cell: IVec2| {
            image.get(&cell).copied().unwrap_or(0.0) * (cell.as_vec2() - emitter.as_vec2() - 0.5).length()
        };
        let png = GrayImage::from_fn(512, 512, |x, y| {
            let cell = IVec2::new((x / 4) as i32, (y / 4) as i32);
            Luma([(ratio(cell) * 0.5).clamp(0.0, 1.0).mul(255.0).round() as u8])
        });
        png.save(format!("out/noext_ratio_{}.png", kind.name())).unwrap();
        let mut values: Vec<f32> =
            image.keys().filter(|c| (**c - emitter).abs().max_element() > 2).map(|c| ratio(*c)).collect();
        values.sort_by(f32::total_cmp);
        let n = values.len();
        println!(
            "{}: ratio min {} p5 {} median {} p95 {} max {}",
            kind.name(),
            values[0],
            values[n / 20],
            values[n / 2],
            values[n * 19 / 20],
            values[n - 1]
        );
    }
}
