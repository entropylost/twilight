//! Renders the example scene to a PNG without a browser.

use std::path::PathBuf;
use std::process::exit;

use clap::Parser;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use image::{GrayImage, Luma};
use twilight::render::{DEFAULT_FRUSTUM_START, MergeMode, RadianceKind, RenderSettings};
use twilight::scene::Scene;

const SIZE: u32 = 128;

#[derive(Parser)]
#[command(about = "Renders the example scene to a PNG without a browser")]
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
    #[arg(short, long, default_value_t = 1)]
    scale: u32,

    #[arg(short = 'p', long)]
    pretrace: bool,

    /// Which odd-frustum merges to combine
    #[arg(
        short,
        long,
        default_value = MergeMode::default().name(),
        value_parser = PossibleValuesParser::new(MergeMode::ALL.iter().map(|mode| mode.name()))
            .map(|name| name.parse::<MergeMode>().unwrap()),
    )]
    merge_mode: MergeMode,
}

fn main() {
    let Args {
        output,
        radiance,
        frustum_start,
        skip_linear,
        scale,
        pretrace,
        merge_mode,
    } = Args::parse();

    let settings = RenderSettings {
        frustum_start,
        skip_linear,
        pretrace,
        merge_mode,
    };
    let radiance = radiance.render(&Scene::example(), settings);

    let image = GrayImage::from_fn(SIZE * scale, SIZE * scale, |x, y| {
        let cell = glam::UVec2::new(x / scale, y / scale).as_ivec2();
        let value = radiance.get(&cell).copied().unwrap_or(0.0);
        Luma([(value.clamp(0.0, 1.0) * 255.0).round() as u8])
    });
    image.save(&output).unwrap_or_else(|err| {
        eprintln!("Failed to write {}: {err}", output.display());
        exit(1);
    });
    println!("Wrote {}", output.display());
}
