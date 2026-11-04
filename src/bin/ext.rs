use std::{cell::RefCell, collections::HashMap, rc::Rc};

use glam::{Affine2, IVec2, Vec2};
use log::debug;
use palette::WithAlpha;
use twilight::{
    draw::{DrawContext, rgb},
    extension::{ConeExtension, Extension, ExtensionType, draw_extension},
    shape::Rect,
};
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::window;

fn run() -> Option<()> {
    let document = window()?.document()?;

    let canvas = document
        .get_element_by_id("main")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;

    let ctx = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .ok()?;

    let ctx = DrawContext::new(ctx);

    let mut ctx =
        ctx.transform(Affine2::from_scale(Vec2::splat(32.0)) * Affine2::from_translation(Vec2::new(10.0, 15.0)));

    anim_loop(move || {
        let window_width = window().unwrap().inner_width().unwrap().as_f64().unwrap() as u32;
        let window_height = window().unwrap().inner_height().unwrap().as_f64().unwrap() as u32;
        if canvas.width() != window_width || canvas.height() != window_height {
            debug!("Resizing canvas to {window_width}x{window_height}");
            canvas.set_width(window_width);
            canvas.set_height(window_height);
            ctx.update_transform();
        }

        ctx.set_line_width(0.1);

        ctx.clear();

        for (lv, color) in [
            rgb(0.5, 0.5, 0.5),
            rgb(0.3, 0.3, 1.0),
            rgb(1.0, 0.3, 0.3),
            rgb(0.3, 0.7, 0.3),
        ]
        .into_iter()
        .enumerate()
        {
            let size = 1 << lv;
            let cones = (-size..size)
                .map(|i| {
                    let start = IVec2::new(size, i);
                    ConeExtension {
                        start,
                        end_bottom: start + IVec2::new(size, i),
                    }
                })
                .collect::<Vec<_>>();

            for cone in &cones {
                ctx.polygon(&cone.polygon().vertices)
                    .fill(color.with_alpha(if cone.index() % 2 == 0 { 0.3 } else { 0.15 }));
            }

            for cone in &cones {
                let mut pixels = HashMap::new();
                draw_extension(
                    Extension::Cone(*cone),
                    if cone.index() % 2 == 0 { 1.0 } else { 0.5 },
                    &mut pixels,
                );
                for (pixel, weight) in pixels {
                    ctx.rect(Rect::center_size(pixel.as_vec2() + 0.5, Vec2::splat(0.8)))
                        .fill(color.with_alpha(weight));
                }
            }
        }
    });

    Some(())
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    window()
        .unwrap()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("failed to register `requestAnimationFrame`");
}

fn anim_loop(mut f: impl FnMut() + 'static) {
    let closure_ref = Rc::new(RefCell::new(None));
    let closure_ref_2 = closure_ref.clone();

    *closure_ref.borrow_mut() = Some(Closure::new(move || {
        f();
        request_animation_frame(closure_ref_2.borrow().as_ref().unwrap());
    }));

    request_animation_frame(closure_ref.borrow().as_ref().unwrap());
}

fn main() {
    wasm_logger::init(wasm_logger::Config::default());
    console_error_panic_hook::set_once();
    run().unwrap();
}
