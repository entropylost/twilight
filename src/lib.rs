use glam::IVec2;

pub fn range2d(start: IVec2, end: IVec2) -> impl Iterator<Item = IVec2> {
    (start.x..end.x).flat_map(move |x| (start.y..end.y).map(move |y| IVec2::new(x, y)))
}

pub mod extension;
pub mod frustum;
pub mod render;
pub mod scene;
pub mod shape;
pub mod transmission;

#[cfg(target_arch = "wasm32")]
pub mod draw;
#[cfg(target_arch = "wasm32")]
pub mod input;
