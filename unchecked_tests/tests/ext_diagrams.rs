// Draws the diagrams of docs/EXT_MERGING.md: how `get_extension` puts an extension together from smaller ones, and how
// `get_radiance_recursive_ext` merges cones with them. The shapes are the renderer's own (`ExtensionType::polygon`,
// `FrustumType::polygon`, `merge_even`, `merge_odd`); the list of pieces of each mode is copied from `get_extension` and
// `calculate_long_near`, so it has to be updated by hand when those change.
//   cargo test --release -p unchecked_tests --test ext_diagrams
// writes docs/diagrams/*.svg. y points up in the diagrams, as "bottom" and "top" do in the code.

use std::fmt::Write;

use glam::{IVec2, Vec2};
use twilight::extension::{ConeExtension, Extension, ExtensionType, LineExtension};
use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};

const BLUE: &str = "#2f6fbe";
const ORANGE: &str = "#e07b00";
const GREEN: &str = "#2e9b57";
const PURPLE: &str = "#8455c7";
const RED: &str = "#d0342c";
const GREY: &str = "#777777";
const INK: &str = "#1b1b1b";

struct Svg {
    body: String,
    scale: f32,
    // where the world's origin is, in px
    origin: Vec2,
    width: f32,
    height: f32,
}

impl Svg {
    fn new(scale: f32) -> Self {
        Svg { body: String::new(), scale, origin: Vec2::ZERO, width: 0.0, height: 0.0 }
    }
    fn grow(&mut self, x: f32, y: f32) {
        self.width = self.width.max(x);
        self.height = self.height.max(y);
    }
    fn px(&self, p: Vec2) -> Vec2 {
        Vec2::new(self.origin.x + p.x * self.scale, self.origin.y - p.y * self.scale)
    }
    // Starts a panel showing the pixels min..max (max exclusive), with its top left corner at (left, top) px, and
    // returns the px below it.
    fn panel(&mut self, left: f32, top: f32, min: IVec2, max: IVec2, title: &str) -> f32 {
        let grid_top = top + 22.0;
        self.origin = Vec2::new(left - min.x as f32 * self.scale, grid_top + max.y as f32 * self.scale);
        self.text_px(left, top + 14.0, title, 13.0, "start", INK, true);
        for x in min.x..=max.x {
            self.seg(IVec2::new(x, min.y).as_vec2(), IVec2::new(x, max.y).as_vec2(), "#d4d4d4", 1.0, false);
        }
        for y in min.y..=max.y {
            self.seg(IVec2::new(min.x, y).as_vec2(), IVec2::new(max.x, y).as_vec2(), "#d4d4d4", 1.0, false);
        }
        let size = (max - min).as_vec2() * self.scale;
        self.grow(left + size.x + 20.0, grid_top + size.y + 10.0);
        grid_top + size.y + 10.0
    }
    fn poly(&mut self, points: &[Vec2], fill: &str, opacity: f32, stroke: &str, dashed: bool) {
        let points: Vec<String> = points.iter().map(|&p| self.px(p)).map(|p| format!("{:.1},{:.1}", p.x, p.y)).collect();
        let dash = if dashed { " stroke-dasharray=\"5 4\"" } else { "" };
        writeln!(
            self.body,
            "<polygon points=\"{}\" fill=\"{fill}\" fill-opacity=\"{opacity}\" stroke=\"{stroke}\" stroke-width=\"1.6\" stroke-linejoin=\"round\"{dash}/>",
            points.join(" ")
        )
        .unwrap();
    }
    fn pixel(&mut self, pixel: IVec2, fill: &str, opacity: f32, stroke: &str) {
        let p = pixel.as_vec2();
        self.poly(&[p, p + Vec2::X, p + Vec2::ONE, p + Vec2::Y], fill, opacity, stroke, false);
    }
    fn seg(&mut self, a: Vec2, b: Vec2, stroke: &str, width: f32, dashed: bool) {
        let (a, b) = (self.px(a), self.px(b));
        let dash = if dashed { " stroke-dasharray=\"5 4\"" } else { "" };
        writeln!(
            self.body,
            "<line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" stroke=\"{stroke}\" stroke-width=\"{width}\" stroke-linecap=\"round\"{dash}/>",
            a.x, a.y, b.x, b.y
        )
        .unwrap();
    }
    fn dot(&mut self, p: Vec2, fill: &str) {
        let p = self.px(p);
        writeln!(self.body, "<circle cx=\"{:.1}\" cy=\"{:.1}\" r=\"4\" fill=\"{fill}\"/>", p.x, p.y).unwrap();
    }
    // text at a world position
    fn label(&mut self, p: Vec2, text: &str, anchor: &str, color: &str) {
        let p = self.px(p);
        self.text_px(p.x, p.y + 4.0, text, 11.5, anchor, color, false);
    }
    fn text_px(&mut self, x: f32, y: f32, text: &str, size: f32, anchor: &str, color: &str, bold: bool) {
        let text = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
        let weight = if bold { " font-weight=\"600\"" } else { "" };
        // a white halo keeps labels readable on top of shapes and grid lines
        writeln!(
            self.body,
            "<text xml:space=\"preserve\" x=\"{x:.1}\" y=\"{y:.1}\" font-size=\"{size}\" text-anchor=\"{anchor}\" fill=\"{color}\"{weight} stroke=\"#ffffff\" stroke-width=\"3\" paint-order=\"stroke\">{text}</text>"
        )
        .unwrap();
        let width = text.chars().count() as f32 * size * 0.56;
        let right = match anchor {
            "start" => x + width,
            "middle" => x + width / 2.0,
            _ => x,
        };
        self.grow(right + 12.0, y + 10.0);
    }
    // lines of caption below each other; returns the px below them
    fn lines(&mut self, x: f32, y: f32, lines: &[&str]) -> f32 {
        for (i, line) in lines.iter().enumerate() {
            self.text_px(x, y + 14.0 + i as f32 * 18.0, line, 12.5, "start", INK, false);
        }
        y + 14.0 + lines.len() as f32 * 18.0
    }
    fn save(&self, name: &str) {
        let (w, h) = (self.width.ceil() + 8.0, self.height.ceil() + 8.0);
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"Segoe UI, Helvetica, Arial, sans-serif\">\n<rect width=\"{w}\" height=\"{h}\" fill=\"#ffffff\"/>\n{}</svg>\n",
            self.body
        );
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/diagrams");
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(format!("{dir}/{name}.svg"), svg).unwrap();
    }

    fn extension(&mut self, extension: Extension, color: &str, opacity: f32) {
        self.poly(&extension.polygon().vertices, color, opacity, color, false);
    }
    fn outline(&mut self, points: &[Vec2]) {
        self.poly(points, "none", 0.0, GREY, true);
    }
    // names a pixel, in its centre
    fn mark(&mut self, pixel: IVec2, text: &str, color: &str) {
        self.pixel(pixel, "none", 0.0, color);
        self.label(pixel.as_vec2() + 0.5, text, "middle", color);
    }
}

fn line(start: IVec2, delta: IVec2) -> Extension {
    Extension::Line(LineExtension { start, end: start + delta })
}
fn cone(start: IVec2, delta: IVec2) -> Extension {
    Extension::Cone(ConeExtension { start, end_bottom: start + delta })
}
fn centre(points: &[Vec2]) -> Vec2 {
    points.iter().sum::<Vec2>() / points.len() as f32
}

// The size-1 extensions, which are read straight from the scene.
fn leaf() {
    let mut svg = Svg::new(46.0);
    let start = IVec2::new(1, 1);
    let panels: [(&str, Extension, i32, &[&str]); 4] = [
        (
            "Line, index 0",
            line(start, IVec2::new(1, 0)),
            1,
            &["crosses the start pixel only", "T = 1 (the side pixel only emits)"],
        ),
        (
            "Line, index 1 (diagonal)",
            line(start, IVec2::new(1, 1)),
            1,
            &["every ray crosses both pixels", "T = 0 if the side pixel is solid"],
        ),
        (
            "Cone, index 0 (view up)",
            cone(start, IVec2::new(1, 0)),
            1,
            &["T towards end_bottom = 1", "T towards end_top = 0 if side is solid"],
        ),
        (
            "Cone, index -1 (view down)",
            cone(start, IVec2::new(1, -1)),
            -1,
            &["T towards end_top = 1", "T towards end_bottom = 0 if side is solid"],
        ),
    ];
    let mut bottom = 0.0;
    for (k, (title, extension, view, caption)) in panels.into_iter().enumerate() {
        let left = 16.0 + k as f32 * 232.0;
        let below = svg.panel(left, 10.0, IVec2::new(0, 0), IVec2::new(4, 3), title);
        let side = start + IVec2::Y * view;
        svg.extension(extension, BLUE, 0.3);
        svg.mark(start, "start", INK);
        svg.mark(side, "side", INK);
        // the start pixel's near face, and the face that the side pixel shows
        svg.seg(start.as_vec2(), start.as_vec2() + Vec2::Y, RED, 4.0, false);
        let face = start.as_vec2() + Vec2::Y * if view > 0 { 1.0 } else { 0.0 };
        svg.seg(face, face + Vec2::X, ORANGE, 4.0, false);
        if let Extension::Cone(c) = extension {
            svg.label(c.end_bottom.as_vec2() + Vec2::new(0.08, 0.5), "end_bottom", "start", BLUE);
            svg.label(c.end_bottom.as_vec2() + Vec2::new(0.08, 1.5), "end_top", "start", BLUE);
        }
        bottom = svg.lines(left, below, caption);
    }
    svg.lines(
        16.0,
        bottom + 8.0,
        &[
            "Size 1 is read from the scene (never cached). The rays of a piece are those that arrive at the start pixel's near face (red).",
            "Start solid: the piece sees that face, emission × ½, T = 0, nothing else is read.",
            "Start empty, side solid: the piece sees the side pixel's face towards the start (orange), emission × ½, and T is as written above.",
            "Both empty: no emission, T = 1. The side pixel is start + (0, 1) for view up (cone index ≥ 0) and start − (0, 1) for view down.",
        ],
    );
    svg.save("leaf");
}

fn line_even() {
    let mut svg = Svg::new(40.0);
    let (start, lo) = (IVec2::ZERO, IVec2::new(2, 1));
    let whole = line(start, lo * 2);
    let below = svg.panel(16.0, 10.0, IVec2::new(0, 0), IVec2::new(6, 4), "Line of size 2s, even index: delta = 2·lo");
    let (a, b) = (line(start, lo), line(start + lo, lo));
    svg.extension(a, BLUE, 0.35);
    svg.extension(b, ORANGE, 0.35);
    svg.outline(&whole.polygon().vertices);
    svg.mark(start, "start", INK);
    svg.mark(start + lo, "start+lo", INK);
    svg.mark(start + lo * 2, "end", INK);
    svg.label(centre(&a.polygon().vertices) + Vec2::new(-0.1, 0.75), "A", "middle", BLUE);
    svg.label(centre(&b.polygon().vertices) + Vec2::new(0.1, -0.75), "B", "middle", ORANGE);
    svg.lines(
        16.0,
        below,
        &[
            "The two halves follow each other along the same line:",
            "A = line(start, lo) (blue),   B = line(start+lo, lo) (orange)",
            "result = A over B:   T = T(A) · T(B),   L = L(A) + T(A) · L(B)",
        ],
    );
    svg.save("line_even");
}

fn line_odd() {
    let mut svg = Svg::new(40.0);
    let (start, lo, hi) = (IVec2::ZERO, IVec2::new(2, 0), IVec2::new(2, 1));
    let whole = line(start, lo + hi);
    let mut bottom = 0.0;
    for (k, (title, first, second)) in
        [("Below: through start+lo", lo, hi), ("Above: through start+hi", hi, lo)].into_iter().enumerate()
    {
        let left = 16.0 + k as f32 * 270.0;
        bottom = svg.panel(left, 10.0, IVec2::new(0, 0), IVec2::new(6, 3), title);
        svg.extension(line(start, first), BLUE, 0.35);
        svg.extension(line(start + first, second), ORANGE, 0.35);
        svg.outline(&whole.polygon().vertices);
        svg.mark(start, "start", INK);
        svg.mark(start + lo, "lo", if first == lo { INK } else { GREY });
        svg.mark(start + hi, "hi", if first == hi { INK } else { GREY });
        svg.mark(start + lo + hi, "end", INK);
    }
    svg.lines(
        16.0,
        bottom,
        &[
            "Line of size 2s, odd index: delta = lo + hi, with hi = lo + (0, 1). It passes between the two middle pixels,",
            "so half of it goes through each. Blue: first half, orange: second half, dashed: the whole line.",
            "below = line(start, lo) over line(start+lo, hi)        above = line(start, hi) over line(start+hi, lo)",
            "result = ½ · below + ½ · above   (T and L both averaged)",
        ],
    );
    svg.save("line_odd");
}

// The four pieces of a cone extension of size 2s: `even` picks delta = 2·lo (straight edge below) or lo + hi (above).
fn cone_halves(even: bool) {
    let mut svg = Svg::new(36.0);
    let start = IVec2::ZERO;
    let lo = IVec2::new(2, if even { 1 } else { 0 });
    let hi = lo + IVec2::Y;
    let delta = if even { lo * 2 } else { lo + hi };
    let (full, half, edge) = if even { (lo, hi, lo) } else { (hi, lo, hi) };
    let whole = cone(start, delta);
    let rows = if even { 4 } else { 3 };
    let pieces = [
        ("1. edge line  (× ½)", line(start, edge * 2), ORANGE),
        ("2. first cone  (× ½)", cone(start, lo), BLUE),
        ("3. second cone  (× ½)", cone(start + full, lo), GREEN),
        ("4. narrow line  (× ½)", line(start + half, edge), PURPLE),
    ];
    let mut bottom = 0.0;
    for (k, (title, piece, color)) in pieces.into_iter().enumerate() {
        let left = 16.0 + k as f32 * 222.0;
        bottom = svg.panel(left, 10.0, IVec2::new(0, 0), IVec2::new(5, rows), title);
        svg.extension(piece, color, 0.4);
        svg.outline(&whole.polygon().vertices);
        svg.mark(start + full, "full", INK);
        svg.mark(start + half, "half", INK);
        svg.mark(start + delta, "eb", GREY);
        svg.mark(start + delta + IVec2::Y, "et", GREY);
    }
    let (edge_end, other_end, side) = if even { ("eb", "et", "lower") } else { ("et", "eb", "upper") };
    let delta_text = if even { "even index: delta = 2·lo" } else { "odd index: delta = lo + hi" };
    svg.lines(
        16.0,
        bottom,
        &[
            &format!(
                "Cone extension of size 2s, {delta_text} (dashed). Its straight edge is the {side} one; at the middle column it covers"
            ),
            "the pixel on that edge fully (\"full\") and half of the other (\"half\"). eb / et are its end pixels end_bottom / end_top.",
            "  1. edge line = line(start, 2·edge): the whole line along the straight edge",
            "  2. first cone = cone(start, lo): first half, ends in full and half",
            "  3. second cone = cone(start+full, lo): continues the first cone from the full pixel",
            "  4. narrow line = line(start+half, edge): continues the first cone from the half pixel",
            "Each piece is attenuated only by the piece it continues (T1→full, T1→half: the first cone's two end transmittances):",
            "  L = ½ · [ L(edge line) + L(first cone) + T1→full · L(second cone) + T1→half · L(narrow line) ]",
            &format!("  T towards {edge_end} = ½ · [ T(edge line) + T1→full · T(second cone → {edge_end}) ]"),
            &format!("  T towards {other_end} = ½ · [ T1→full · T(second cone → {other_end}) + T1→half · T(narrow line) ]"),
            "Stored as transmittance = mean of the two and spread = half their difference.",
        ],
    );
    svg.save(if even { "cone_even" } else { "cone_odd" });
}

fn cone_points(f: Frustum, min: f32, max: f32) -> Vec<Vec2> {
    f.polygon(min, max).vertices
}

// the cone being merged: its edge rays, its focus, and the stretch before its first extension
fn draw_merged_cone(svg: &mut Svg, c: ConeFrustum, reach: f32) {
    let f = c.focus.as_vec2();
    svg.poly(&cone_points(Frustum::Cone(c), 0.0, 1.0), GREY, 0.15, "none", false);
    svg.seg(f, f + c.lower_dir().as_vec2() * reach, GREY, 1.4, true);
    svg.seg(f, f + c.upper_dir().as_vec2() * reach, GREY, 1.4, true);
    svg.dot(f, INK);
}

// a cone of twice the size that is merged: from where it starts reading (t = 1) to t = 2, and its focus
fn draw_far_cone(svg: &mut Svg, far: Frustum, color: &str, name: &str, show_focus: bool) {
    let points = cone_points(far, 1.0, 2.0);
    svg.poly(&points, color, 0.3, color, false);
    if show_focus {
        let focus = far.focus();
        svg.seg(focus, points[0], color, 1.0, true);
        svg.seg(focus, points[3], color, 1.0, true);
        svg.dot(focus, color);
    }
    svg.label((points[1] + points[2]) / 2.0 + Vec2::new(0.2, 0.0), name, "start", color);
}

const MERGE_SIZE: i32 = 2;
const MERGE_INDEX: i32 = 1;

fn merge_even_diagram() {
    let mut svg = Svg::new(34.0);
    let c = ConeFrustum { focus: IVec2::ZERO, size: MERGE_SIZE, index: MERGE_INDEX };
    assert_eq!(Frustum::Cone(c).parity(), 0);
    let below = svg.panel(16.0, 10.0, IVec2::new(0, -1), IVec2::new(10, 9), "Even merge (focus.x / size is even), size s = 2, index i = 1");
    draw_merged_cone(&mut svg, c, 4.4);
    let start = c.focus + c.lower_dir();
    let near = ConeExtension { start, end_bottom: start + c.lower_dir() };
    let far = merge_even(Frustum::Cone(c));
    draw_far_cone(&mut svg, far[0].0, GREEN, "far 2i", false);
    draw_far_cone(&mut svg, far[1].0, PURPLE, "far 2i+1", false);
    svg.extension(Extension::Cone(near), BLUE, 0.4);
    svg.mark(start, "start", INK);
    svg.mark(near.end_bottom, "eb", INK);
    svg.mark(near.end_bottom + IVec2::Y, "et", INK);
    svg.label(c.focus.as_vec2() + Vec2::new(0.3, -0.45), "focus F", "start", INK);
    svg.label(centre(&Extension::Cone(near).polygon().vertices) + Vec2::new(-0.2, 1.3), "near", "end", BLUE);
    svg.lines(
        16.0,
        below,
        &[
            "near = cone extension from start = F + (s, i) to end_bottom = start + (s, i): the cone's second s columns.",
            "The two far cones have the same focus and size 2s; they take over at the near extension's two end pixels (eb, et):",
            "  L = L(near) + T(near → eb) · L(far 2i) + T(near → et) · L(far 2i+1)",
            "Grey: the cone's first s columns, which it never reads: its extensions start s columns ahead of the focus",
            "(for the size-1 cones of a probe, that is the probe's own pixel, read by probe_pixel).",
        ],
    );
    svg.save("merge_even");
}

fn merge_odd_short_diagram() {
    let mut svg = Svg::new(34.0);
    let c = ConeFrustum { focus: IVec2::new(MERGE_SIZE, 0), size: MERGE_SIZE, index: MERGE_INDEX };
    assert_eq!(Frustum::Cone(c).parity(), 1);
    let below = svg.panel(16.0, 10.0, IVec2::new(-3, -3), IVec2::new(10, 7), "Odd merge, short half (focus.x / size is odd), s = 2, i = 1");
    draw_merged_cone(&mut svg, c, 3.2);
    let far = merge_odd(Frustum::Cone(c), false, true);
    draw_far_cone(&mut svg, far[0].0, GREEN, "far 2i", true);
    draw_far_cone(&mut svg, far[1].0, PURPLE, "far 2i+1", true);
    svg.mark(c.focus + c.lower_dir(), "start", INK);
    svg.label(c.focus.as_vec2() + Vec2::new(0.35, -0.9), "focus F", "start", INK);
    svg.label(far[0].0.focus() + Vec2::new(-0.3, 0.0), "F − (s, i)", "end", GREEN);
    svg.label(far[1].0.focus() + Vec2::new(-0.3, 0.0), "F − (s, i+1)", "end", PURPLE);
    svg.lines(
        16.0,
        below,
        &[
            "The far cones of size 2s are focused one step behind F, along the cone's lower and upper edge. Both start reading",
            "at the cone's start pixel, so there is no near extension and nothing to attenuate by:",
            "  short = L(far 2i) + L(far 2i+1)",
        ],
    );
    svg.save("merge_odd_short");
}

fn merge_odd_long_diagram() {
    let mut svg = Svg::new(30.0);
    let c = ConeFrustum { focus: IVec2::new(MERGE_SIZE, 0), size: MERGE_SIZE, index: MERGE_INDEX };
    assert_eq!(Frustum::Cone(c).parity(), 1);
    let start = c.focus + c.lower_dir();
    let lower_end = start + c.lower_dir() * 2;
    let middle_end = start + c.lower_dir() + c.upper_dir();
    let bottom = cone(start, lower_end - start);
    let top = cone(start, middle_end - start);
    let middle = line(start, middle_end - start);

    let below = svg.panel(16.0, 10.0, IVec2::new(1, -1), IVec2::new(14, 11), "Odd merge, long half, s = 2, i = 1");
    draw_merged_cone(&mut svg, c, 5.4);
    let far = merge_odd(Frustum::Cone(c), true, true);
    draw_far_cone(&mut svg, far[0].0, GREEN, "far 2i", true);
    draw_far_cone(&mut svg, far[1].0, PURPLE, "far 2i+1", true);
    svg.poly(&cone_points(Frustum::Cone(c), 1.0, 3.0), BLUE, 0.3, BLUE, false);
    svg.mark(start, "start", INK);
    svg.mark(lower_end, "eb", INK);
    svg.mark(middle_end + IVec2::Y, "et", INK);
    svg.label(c.focus.as_vec2() + Vec2::new(0.3, -0.45), "focus F", "start", INK);
    svg.label(far[0].0.focus() + Vec2::new(0.3, -0.5), "F + (s, i)", "start", GREEN);
    svg.label(far[1].0.focus() + Vec2::new(-0.25, 0.4), "F + (s, i+1)", "end", PURPLE);
    svg.label(Vec2::new(6.5, 3.35), "near", "middle", BLUE);

    let mut right_bottom = 0.0;
    let pieces = [("bottom  (+1)", bottom, BLUE), ("top  (+1)", top, BLUE), ("middle  (−2)", middle, RED)];
    for (k, (title, piece, color)) in pieces.into_iter().enumerate() {
        let left = 440.0 + k as f32 * 200.0;
        right_bottom = svg.panel(left, 10.0, IVec2::new(4, 1), IVec2::new(10, 7), title);
        svg.extension(piece, color, 0.4);
        svg.outline(&cone_points(Frustum::Cone(c), 1.0, 3.0));
        svg.mark(start, "", INK);
        svg.mark(lower_end, "eb", INK);
        svg.mark(middle_end + IVec2::Y, "et", INK);
    }
    svg.lines(
        440.0,
        right_bottom + 4.0,
        &[
            "The near part spans 2s columns from start = F + (s, i) and is the sum of three",
            "extensions of size 2s, each resampled into the cone:",
            "  bottom = cone(start, 2·(s, i))                 along the lower edge",
            "  top = cone(start, (2s, 2i+1))                   along the upper edge",
            "  middle = line(start, (2s, 2i+1))               where the two overlap",
            "  L(near) = L(bottom) + L(top) − 2 · L(middle)",
            "The far cones of size 2s are focused one step ahead of F, at start and above it.",
            "They take over at bottom's lower end pixel (eb) and top's upper end pixel (et):",
            "  long = L(near) + T(bottom → eb) · L(far 2i) + T(top → et) · L(far 2i+1)",
            "",
            "Odd merge = ½ · short + ½ · long",
        ],
    );
    svg.grow(0.0, below);
    svg.save("merge_odd_long");
}

#[test]
fn ext_diagrams() {
    leaf();
    line_even();
    line_odd();
    cone_halves(true);
    cone_halves(false);
    merge_even_diagram();
    merge_odd_short_diagram();
    merge_odd_long_diagram();
}
