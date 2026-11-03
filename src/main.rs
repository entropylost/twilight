use std::cell::RefCell;
use std::rc::Rc;

use glam::{Affine2, Vec2};
use log::debug;
use twilight::render::{DEFAULT_FRUSTUM_START, MergeMode, RenderSettings, TransmissionKind};
use twilight::scene::{Emitter, Scene};
use twilight::shape::Rect;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::window;

use crate::draw::{DrawContext, lumaa};
use crate::input::MouseButton;

pub mod draw;
pub mod input;

fn run() -> Option<()> {
    let document = window()?.document()?;

    let canvas = document
        .get_element_by_id("main")?
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .ok()?;
    canvas
        .add_event_listener_with_callback(
            "contextmenu",
            Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                event.prevent_default();
                event.stop_propagation();
            })
            .into_js_value()
            .unchecked_ref(),
        )
        .ok()?;

    let ctx = canvas
        .get_context("2d")
        .ok()??
        .dyn_into::<web_sys::CanvasRenderingContext2d>()
        .ok()?;

    let selector = document
        .get_element_by_id("transmission")?
        .dyn_into::<web_sys::HtmlSelectElement>()
        .ok()?;
    for &kind in TransmissionKind::ALL {
        let option = document
            .create_element("option")
            .ok()?
            .dyn_into::<web_sys::HtmlOptionElement>()
            .ok()?;
        option.set_text(kind.name());
        option.set_selected(kind == TransmissionKind::default());
        selector.add_with_html_option_element(&option).ok()?;
    }
    let merge_mode_selector = document
        .get_element_by_id("merge-mode")?
        .dyn_into::<web_sys::HtmlSelectElement>()
        .ok()?;
    for &mode in MergeMode::ALL {
        let option = document
            .create_element("option")
            .ok()?
            .dyn_into::<web_sys::HtmlOptionElement>()
            .ok()?;
        option.set_text(mode.name());
        option.set_selected(mode == MergeMode::default());
        merge_mode_selector.add_with_html_option_element(&option).ok()?;
    }
    let frustum_length_input = document
        .get_element_by_id("frustum-len")
        .unwrap()
        .dyn_into::<web_sys::HtmlInputElement>()
        .unwrap();
    let skip_linear_input = document
        .get_element_by_id("skip-linear")
        .unwrap()
        .dyn_into::<web_sys::HtmlInputElement>()
        .unwrap();
    let pretrace_input = document
        .get_element_by_id("pretrace")
        .unwrap()
        .dyn_into::<web_sys::HtmlInputElement>()
        .unwrap();

    let ctx = DrawContext::new(ctx);

    let mut ctx = ctx.transform(Affine2::from_scale(Vec2::splat(8.0)));

    let mut scene = Scene::example();
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

        let frustum_start = frustum_length_input
            .value()
            .parse::<f32>()
            .unwrap_or(DEFAULT_FRUSTUM_START);
        let skip_linear = skip_linear_input.checked();
        let pretrace = pretrace_input.checked();
        let merge_mode = MergeMode::ALL[merge_mode_selector.selected_index().max(0) as usize];

        let kind = TransmissionKind::ALL[selector.selected_index().max(0) as usize];
        let settings = RenderSettings {
            frustum_start,
            skip_linear,
            pretrace,
            merge_mode,
        };
        for (cell, radiance) in kind.render(&scene, settings) {
            let radiance = radiance.powf(1.0 / 2.2);
            ctx.rect(Rect::center_size(cell.as_vec2(), Vec2::splat(1.0)))
                .fill(lumaa(radiance, 1.0));
        }

        for emitter in &mut scene.emitters {
            ctx.responsive_point(&mut emitter.position);
            ctx.circle(emitter.position, 0.4).fill("red");
        }
        for occluder in &mut scene.occluders {
            ctx.polygon(&occluder.vertices).stroke("red");
            for vertex in &mut occluder.vertices {
                ctx.responsive_point(vertex);
            }
        }

        if ctx.mouse.just_pressed(MouseButton::Right) {
            let mouse_pos = ctx.mouse.position();
            let position = ctx.transform.inverse().transform_point2(mouse_pos);
            scene.emitters.push(Emitter {
                position,
                brightness: 3.0,
            });
        }

        ctx.finish();
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
