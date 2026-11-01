//! Renders the example scene to a PNG without a browser.

use std::path::PathBuf;
use std::process::exit;

use clap::Parser;
use clap::builder::{PossibleValuesParser, TypedValueParser};
use image::{GrayImage, Luma};
use twilight::render::{DEFAULT_FRUSTRUM_START, RenderSettings, TransmissionKind};
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
        default_value = TransmissionKind::default().name(),
        value_parser = PossibleValuesParser::new(TransmissionKind::ALL.iter().map(|kind| kind.name()))
            .map(|name| name.parse::<TransmissionKind>().unwrap()),
    )]
    transmission: TransmissionKind,

    /// Frustrum length
    #[arg(short, long, default_value_t = DEFAULT_FRUSTRUM_START)]
    frustrum_start: f32,

    /// Skip the linear frustrum when merging odd frustrums
    #[arg(short = 'l', long)]
    skip_linear: bool,

    /// Pixels per cell
    #[arg(short, long, default_value_t = 1)]
    scale: u32,
}

fn main() {
    let Args {
        output,
        transmission,
        frustrum_start,
        skip_linear,
        scale,
    } = Args::parse();

    let settings = RenderSettings {
        frustrum_start,
        skip_linear,
    };
    let radiance = transmission.render(&Scene::example(), settings);

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
