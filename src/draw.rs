use glam::{Affine2, IVec2, Vec2};
use palette::luma::{Luma, Lumaa};
use palette::{Srgb, Srgba};
use twilight::shape::Rect;
use web_sys::CanvasRenderingContext2d;

use crate::input::{MouseButton, MouseState, register_input_handlers};

pub fn rgb(r: f32, g: f32, b: f32) -> Srgb {
    Srgb::new(r, g, b)
}
pub fn rgba(r: f32, g: f32, b: f32, a: f32) -> Srgba {
    Srgba::new(r, g, b, a)
}
pub fn luma(l: f32) -> Luma {
    Luma::new(l)
}
pub fn lumaa(l: f32, a: f32) -> Lumaa {
    Lumaa::new(l, a)
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanvasColor(String);

impl From<Srgb> for CanvasColor {
    fn from(value: Srgb) -> Self {
        let (r, g, b) = value.into_components();
        Self(format!("rgb({}, {}, {})", r * 255.0, g * 255.0, b * 255.0))
    }
}
impl From<Srgba> for CanvasColor {
    fn from(value: Srgba) -> Self {
        let (r, g, b, a) = value.into_components();
        Self(format!("rgba({}, {}, {}, {})", r * 255.0, g * 255.0, b * 255.0, a))
    }
}
impl From<Luma> for CanvasColor {
    fn from(value: Luma) -> Self {
        let l = value.into_components().0;
        Self(format!("rgb({}, {}, {})", l * 255.0, l * 255.0, l * 255.0))
    }
}
impl From<Lumaa> for CanvasColor {
    fn from(value: Lumaa) -> Self {
        let (l, a) = value.into_components();
        Self(format!("rgba({}, {}, {}, {})", l * 255.0, l * 255.0, l * 255.0, a))
    }
}
impl From<&str> for CanvasColor {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

#[derive(Debug)]
pub struct Path<'a> {
    context: &'a mut DrawContext,
}
impl Path<'_> {
    pub fn fill(self, color: impl Into<CanvasColor>) -> Self {
        self.context.ctx.set_fill_style_str(&color.into().0);
        self.context.ctx.fill();
        self
    }
    pub fn stroke(self, color: impl Into<CanvasColor>) -> Self {
        self.context.ctx.set_stroke_style_str(&color.into().0);
        self.context.ctx.stroke();
        self
    }
}

#[derive(Debug, Clone)]
pub struct DrawContext {
    ctx: CanvasRenderingContext2d,
    pub mouse: MouseState,
    pub transform: Affine2,
    active_point: Option<Vec2>,
}
impl DrawContext {
    pub fn new(ctx: CanvasRenderingContext2d) -> Self {
        let mouse = register_input_handlers(ctx.canvas().unwrap().into());
        Self {
            ctx,
            mouse,
            transform: Affine2::IDENTITY,
            active_point: None,
        }
    }
    pub fn polyline(&mut self, points: &[Vec2]) -> Path<'_> {
        self.ctx.begin_path();
        if let Some((first, rest)) = points.split_first() {
            self.ctx.move_to(first.x as f64, first.y as f64);
            for point in rest {
                self.ctx.line_to(point.x as f64, point.y as f64);
            }
        }
        Path { context: self }
    }
    pub fn polygon(&mut self, points: &[Vec2]) -> Path<'_> {
        self.ctx.begin_path();
        if let Some((first, rest)) = points.split_first() {
            self.ctx.move_to(first.x as f64, first.y as f64);
            for point in rest {
                self.ctx.line_to(point.x as f64, point.y as f64);
            }
            self.ctx.close_path();
        }
        Path { context: self }
    }
    pub fn circle(&mut self, center: Vec2, radius: f32) -> Path<'_> {
        self.ctx.begin_path();
        self.ctx
            .arc(
                center.x as f64,
                center.y as f64,
                radius as f64,
                0.0,
                std::f64::consts::TAU,
            )
            .unwrap();
        Path { context: self }
    }
    pub fn rect(&mut self, rect: Rect) -> Path<'_> {
        self.ctx.begin_path();
        self.ctx.rect(
            rect.min.x as f64,
            rect.min.y as f64,
            rect.size().x as f64,
            rect.size().y as f64,
        );
        Path { context: self }
    }

    pub fn transform(mut self, transform: Affine2) -> Self {
        self.transform *= transform;
        self.update_transform();
        self
    }

    pub fn update_transform(&mut self) {
        let transform = self.transform.as_daffine2();
        self.ctx
            .set_transform(
                transform.matrix2.x_axis.x,
                transform.matrix2.x_axis.y,
                transform.matrix2.y_axis.x,
                transform.matrix2.y_axis.y,
                transform.translation.x,
                transform.translation.y,
            )
            .unwrap();
    }

    pub fn responsive_point(&mut self, point: &mut Vec2) {
        let display_point = self.transform.transform_point2(*point);
        let mouse_pos = self.mouse.position();
        if self.mouse.just_pressed(MouseButton::Left) && display_point.distance(mouse_pos) < 10.0 {
            self.active_point = Some(display_point);
        }
        if self.active_point == Some(display_point) {
            let new_point = self.transform.inverse().transform_point2(mouse_pos);
            *point = new_point;
            self.active_point = Some(self.transform.transform_point2(*point));
        }
    }

    pub fn responsive_grid_point(&mut self, point: &mut IVec2) {
        let display_point = self.transform.transform_point2(point.as_vec2());
        let mouse_pos = self.mouse.position();
        if self.mouse.just_pressed(MouseButton::Left) && display_point.distance(mouse_pos) < 10.0 {
            self.active_point = Some(display_point);
        }
        if self.active_point == Some(display_point) {
            let new_point = self.transform.inverse().transform_point2(mouse_pos);
            *point = new_point.round().as_ivec2();
            self.active_point = Some(self.transform.transform_point2(point.as_vec2()));
        }
    }

    pub fn clear(&mut self) {
        let size = Vec2::new(
            self.ctx.canvas().unwrap().width() as f32,
            self.ctx.canvas().unwrap().height() as f32,
        );
        let start = self.transform.inverse().transform_point2(Vec2::ZERO).as_dvec2();
        let size = self.transform.inverse().transform_vector2(size).as_dvec2();
        self.ctx.clear_rect(start.x, start.y, size.x, size.y);
    }

    pub fn set_line_width(&mut self, width: f32) {
        self.ctx.set_line_width(width as f64);
    }

    pub fn finish(&mut self) {
        if self.mouse.released(MouseButton::Left) {
            self.active_point = None;
        }
        self.mouse.step_frame();
    }
    pub fn raw(&mut self) -> &mut CanvasRenderingContext2d {
        &mut self.ctx
    }
}
