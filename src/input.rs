use std::{
    ops::Deref,
    rc::Rc,
    sync::{
        Mutex,
        atomic::{AtomicU32, Ordering},
    },
};

use glam::Vec2;
use wasm_bindgen::{JsCast, prelude::Closure};
use web_sys::{Element, MouseEvent};

#[derive(Debug, Clone)]
pub struct MouseState(Rc<MouseStateInner>);

impl Deref for MouseState {
    type Target = MouseStateInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct MouseStateInner {
    pos: Mutex<Vec2>,
    last_pressed_buttons: AtomicU32,
    pressed_buttons: AtomicU32,
}

impl MouseStateInner {
    pub fn position(&self) -> Vec2 {
        *self.pos.lock().unwrap()
    }
    pub fn pressed(&self, button: MouseButton) -> bool {
        self.pressed_buttons.load(Ordering::Relaxed) & (1 << button as u32) != 0
    }
    pub fn released(&self, button: MouseButton) -> bool {
        !self.pressed(button)
    }
    pub fn last_pressed(&self, button: MouseButton) -> bool {
        self.last_pressed_buttons.load(Ordering::Relaxed) & (1 << button as u32) != 0
    }
    pub fn just_pressed(&self, button: MouseButton) -> bool {
        self.pressed(button) && !self.last_pressed(button)
    }
    pub fn just_released(&self, button: MouseButton) -> bool {
        !self.pressed(button) && self.last_pressed(button)
    }
    pub fn step_frame(&self) {
        self.last_pressed_buttons.store(self.pressed_buttons.load(Ordering::Relaxed), Ordering::Relaxed);
    }
    fn apply_buttons(&self, buttons: u16) {
        self.pressed_buttons.store(buttons as u32, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    Back = 3,
    Forward = 4,
}

pub fn register_input_handlers(el: Element) -> MouseState {
    let state = MouseState(Rc::new(MouseStateInner {
        pos: Mutex::new(Vec2::ZERO),
        last_pressed_buttons: AtomicU32::new(0),
        pressed_buttons: AtomicU32::new(0),
    }));

    let closure = Closure::<dyn FnMut(MouseEvent)>::new({
        let state = state.clone();
        move |event: MouseEvent| {
            *state.pos.lock().unwrap() = Vec2::new(event.offset_x() as f32, event.offset_y() as f32);
            state.apply_buttons(event.buttons());
            event.prevent_default();
            event.stop_propagation();
        }
    })
    .into_js_value();

    let exit_closure = Closure::<dyn FnMut(MouseEvent)>::new({
        let state = state.clone();
        move |_event: MouseEvent| {
            state.apply_buttons(0);
        }
    })
    .into_js_value();
    let enter_closure = Closure::<dyn FnMut(MouseEvent)>::new({
        let state = state.clone();
        move |event: MouseEvent| {
            state.apply_buttons(event.buttons());
            state.last_pressed_buttons.store(event.buttons() as u32, Ordering::Relaxed);
        }
    })
    .into_js_value();

    el.add_event_listener_with_callback("mousemove", closure.unchecked_ref()).unwrap();
    el.add_event_listener_with_callback("mousedown", closure.unchecked_ref()).unwrap();
    el.add_event_listener_with_callback("mouseup", closure.unchecked_ref()).unwrap();
    el.add_event_listener_with_callback("mouseleave", exit_closure.unchecked_ref()).unwrap();
    el.add_event_listener_with_callback("mouseenter", enter_closure.unchecked_ref()).unwrap();

    el.add_event_listener_with_callback(
        "contextmenu",
        Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            event.prevent_default();
            event.stop_propagation();
        })
        .into_js_value()
        .unchecked_ref(),
    )
    .unwrap();

    state
}
