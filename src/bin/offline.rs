//! Renders the example scene to a PNG without a browser.

use std::path::PathBuf;
use std::process::exit;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Parser, ValueEnum};
use glam::{IVec2, Vec2};
use image::{GrayImage, Luma};
use twilight::extension::PixelScene;
use twilight::radiance::*;
use twilight::render::ext::{RenderExtSettings, render_ext};
use twilight::render::pt::{RenderPtSettings, render_pt};
use twilight::render::{DEFAULT_FRUSTUM_START, MergeMode, RadianceKind, RenderSettings};
use twilight::scene::{Emitter, Scene};

const SIZE: u32 = 128;

#[derive(Parser)]
#[command(about = "Renders a scene to an image.")]
struct Args {
    /// Output path
    #[arg(short, long, default_value = "render.png")]
    output: PathBuf,

    /// Transmission model
    #[arg(
        short,
        long,
        default_value = RadianceKind::default().name(),
        value_parser = PossibleValuesParser::new(RadianceKind::ALL.iter().map(|kind| kind.name()))
            .map(|name| name.parse::<RadianceKind>().unwrap()),
    )]
    radiance: RadianceKind,

    /// Frustum length
    #[arg(short, long, default_value_t = DEFAULT_FRUSTUM_START)]
    frustum_start: f32,

    /// Skip the linear frustum when merging odd frustums
    #[arg(short = 'l', long)]
    skip_linear: bool,

    /// Pixels per cell
    #[arg(long, default_value_t = 1)]
    scale: u32,

    #[arg(short = 'p', long)]
    pretrace: bool,

    #[arg(short = 'e', long)]
    ext: bool,

    #[arg(short, long)]
    simplify_occlusion: bool,

    /// Path trace the pixel scene used by --ext
    #[arg(long)]
    pt: bool,

    /// Rays per probe when path tracing
    #[arg(short, long, default_value_t = 256)]
    directions: u32,

    /// Seed for the per-probe ray rotation when path tracing
    #[arg(long, default_value_t = 0)]
    seed: u64,

    /// Which odd-frustum merges to combine
    #[arg(
        short,
        long,
        default_value = MergeMode::default().name(),
        value_parser = PossibleValuesParser::new(MergeMode::ALL.iter().map(|mode| mode.name()))
            .map(|name| name.parse::<MergeMode>().unwrap()),
    )]
    merge_mode: MergeMode,

    /// Pixel scene used by --ext and --pt
    #[arg(long, value_enum, default_value_t = SceneKind::PixelShadow)]
    scene: SceneKind,
}

#[derive(Clone, Copy, ValueEnum)]
enum SceneKind {
    BlockShadow,
    PixelShadow,
    CircleLight,
}

impl SceneKind {
    fn build(self) -> PixelScene {
        match self {
            SceneKind::BlockShadow => block_shadow(),
            SceneKind::PixelShadow => pixel_shadow(),
            SceneKind::CircleLight => circle_light(),
        }
    }
}

fn block_shadow() -> PixelScene {
    let mut scene = PixelScene::new();

    let pos = IVec2::new(36, 32);
    for i in 0..3 {
        for j in 0..3 {
            scene.insert(pos + IVec2::new(i, j), 2.5);
        }
    }

    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }

    scene
}

fn pixel_shadow() -> PixelScene {
    let mut scene = PixelScene::new();

    let pos = IVec2::new(36, 32);
    scene.insert(pos, 5.0);

    for i in 0..50 {
        scene.insert(IVec2::new(i, 60), 0.0);
    }

    scene
}

fn circle_light() -> PixelScene {
    let mut scene = PixelScene::new();

    let pos = IVec2::new(128 / 2, 128 / 2);
    for i in -10..=10 {
        for j in -10..=10 {
            let offset = IVec2::new(i, j);
            if offset.length_squared() < 95 {
                scene.insert(pos + IVec2::new(i, j), 0.3);
            }
        }
    }

    scene
}

fn main() {
    let Args {
        output,
        radiance,
        frustum_start,
        skip_linear,
        scale,
        pretrace,
        ext,
        simplify_occlusion,
        pt,
        directions,
        seed,
        merge_mode,
        scene,
    } = Args::parse();

    let radiance = if pt {
        render_pt(&scene.build(), RenderPtSettings { directions, seed })
    } else if ext {
        render_ext::<DirectionalPosition, Inverse2>(&scene.build(), RenderExtSettings { simplify_occlusion })
    } else {
        let settings = RenderSettings { frustum_start, skip_linear, pretrace, merge_mode };
        radiance.render(
            &Scene { occluders: vec![], emitters: vec![Emitter { position: Vec2::new(36.5, 32.5), brightness: 1.0 }] },
            settings,
        )
    };

    let image = GrayImage::from_fn(SIZE * scale, SIZE * scale, |x, y| {
        let cell = glam::UVec2::new(x / scale, y / scale).as_ivec2();
        let value = radiance.get(&cell).copied().unwrap_or(0.0);
        let value = value.powf(1.0 / 2.2);
        Luma([(value.clamp(0.0, 1.0) * 255.0).round() as u8])
    });
    image.save(&output).unwrap_or_else(|err| {
        eprintln!("Failed to write {}: {err}", output.display());
        exit(1);
    });
    println!("Wrote {}", output.display());
}
