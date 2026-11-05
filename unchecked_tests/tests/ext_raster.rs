// Prints which pixels an extension samples (via draw_extension) around a window, to compare an extension's rasterised
// footprint against the true cone / line it stands for. Set EXT="cone sx sy ex ey" or "line sx sy ex ey" and
// WIN="x0 y0 x1 y1"; '#' marks pixels listed in WALL="x y0 y1" (a vertical wall column).

use std::collections::HashMap;

use glam::IVec2;
use twilight::extension::{ConeExtension, Extension, LineExtension, draw_extension};

fn nums(name: &str, default: &str) -> Vec<i32> {
    std::env::var(name).unwrap_or(default.into()).split_whitespace().filter_map(|v| v.parse().ok()).collect()
}

#[test]
fn ext_raster() {
    let spec = std::env::var("EXT").unwrap_or("cone 64 50 128 36".into());
    let kind = spec.split_whitespace().next().unwrap().to_string();
    let n: Vec<i32> = spec.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    let (s, e) = (IVec2::new(n[0], n[1]), IVec2::new(n[2], n[3]));
    let ext: Extension = if kind == "line" {
        LineExtension { start: s, end: e }.into()
    } else {
        ConeExtension { start: s, end_bottom: e }.into()
    };
    let w = nums("WIN", "60 44 100 54");
    let wall = nums("WALL", "67 0 49");
    let mut pixels = HashMap::new();
    draw_extension(ext, 1.0, &mut pixels);
    println!("{ext:?}");
    for y in (w[1]..=w[3]).rev() {
        let row: String = (w[0]..=w[2])
            .map(|x| {
                let v = pixels.get(&IVec2::new(x, y)).copied().unwrap_or(0.0);
                let is_wall = x == wall[0] && (wall[1]..=wall[2]).contains(&y);
                match (is_wall, v > 0.0) {
                    (true, true) => 'X',
                    (true, false) => '#',
                    (false, true) => if v >= 0.99 { 'o' } else { '.' },
                    _ => ' ',
                }
            })
            .collect();
        println!("{y:4} |{row}|");
    }
}
