// Writes contrast-boosted copies of the offline renders so faint artifacts are visible: the whole image, and a zoom
// around the emitter. Run the offline binary with `-o render.png` (and `-o render_noext.png` without `--ext`) first.

use image::{GrayImage, Luma};

fn boost(input: &str, output: &str) {
    let Ok(image) = image::open(input) else {
        return;
    };
    let image = image.to_luma8();
    let (w, h) = image.dimensions();
    let scale = 4;
    std::fs::create_dir_all("out").unwrap();
    GrayImage::from_fn(w * scale, h * scale, |x, y| {
        let v = image.get_pixel(x / scale, y / scale).0[0] as f32 / 255.0;
        Luma([(v.powf(0.3) * 255.0).round() as u8])
    })
    .save(format!("out/{output}_boost.png"))
    .unwrap();
    // 32x32 cells around the emitter at (36, 32)
    let zoom = 16;
    GrayImage::from_fn(32 * zoom, 32 * zoom, |x, y| {
        let v = image.get_pixel(20 + x / zoom, 16 + y / zoom).0[0] as f32 / 255.0;
        Luma([(v.powf(0.3) * 255.0).round() as u8])
    })
    .save(format!("out/{output}_zoom.png"))
    .unwrap();
}

#[test]
fn boost_render() {
    boost("../render.png", "render");
    boost("../render_noext.png", "render_noext");
}

// Largest 8 bit difference between the extension render and the analytic one.
#[test]
fn compare_renders() {
    let (Ok(a), Ok(b)) = (image::open("../render.png"), image::open("../render_noext.png")) else {
        return;
    };
    let (a, b) = (a.to_luma8(), b.to_luma8());
    let diffs: Vec<i32> = a.pixels().zip(b.pixels()).map(|(p, q)| (p.0[0] as i32 - q.0[0] as i32).abs()).collect();
    let max = diffs.iter().max().unwrap();
    let count = |d: i32| diffs.iter().filter(|x| **x >= d).count();
    println!("max diff {max}, pixels differing by >=1: {}, >=2: {}", count(1), count(2));
    for (i, d) in diffs.iter().enumerate().filter(|(_, d)| **d >= 2) {
        let (x, y) = (i as u32 % a.width(), i as u32 / a.width());
        println!("  ({x}, {y}): {} vs {}", a.get_pixel(x, y).0[0], b.get_pixel(x, y).0[0]);
    }
}

// Largest 8 bit difference of each render from the exact 1 / distance to the example's emitter at (36.5, 32.5).
#[test]
fn compare_exact() {
    for path in ["../render.png", "../render_noext.png"] {
        let Ok(image) = image::open(path) else {
            continue;
        };
        let image = image.to_luma8();
        let mut max = 0;
        let mut over1 = 0;
        for (x, y, p) in image.enumerate_pixels() {
            if x == 0 || y == 0 {
                continue;
            }
            let d = (glam::Vec2::new(x as f32, y as f32) - glam::Vec2::new(36.5, 32.5)).length();
            let exact = ((1.0 / d).clamp(0.0, 1.0) * 255.0).round() as i32;
            let diff = (p.0[0] as i32 - exact).abs();
            max = max.max(diff);
            over1 += (diff > 1) as u32;
        }
        println!("{path}: max diff from exact {max}, pixels off by more than 1: {over1}");
    }
}
