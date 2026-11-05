// Prints the pixels that differ between the grayscale PNGs in $PNG_DIFF_A and $PNG_DIFF_B.

#[test]
fn png_diff() {
    let (Ok(a), Ok(b)) = (std::env::var("PNG_DIFF_A"), std::env::var("PNG_DIFF_B")) else {
        return;
    };
    let a = image::open(a).unwrap().into_luma8();
    let b = image::open(b).unwrap().into_luma8();
    let mut changed = 0;
    let mut max = 0;
    for (x, y, pa) in a.enumerate_pixels() {
        let (va, vb) = (pa.0[0] as i32, b.get_pixel(x, y).0[0] as i32);
        if va != vb {
            if changed < 30 {
                println!("  (x={x},y={y}) {va} -> {vb}");
            }
            changed += 1;
            max = max.max((va - vb).abs());
        }
    }
    println!("changed pixels {changed}, max abs {max}");
}
