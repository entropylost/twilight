#![allow(clippy::multiple_bound_locations)]

use super::*;
use crate::{
    extension::{
        ConeExtension, Extension, ExtensionType, Fluence, LineExtension, PixelScene, ViewDirection, get_extension,
    },
    radiance::{ResampleRadiance, ViewDir},
};

#[derive(Debug, Clone, Copy)]
pub struct RenderExtSettings {
    pub simplify_occlusion: bool,
}

// splits long cone into near cone, and far cone + a line (both ways). Also uses two more lines for occlusion.
fn calculate_long_near<L: Radiance, I: Radiance>(
    ext_cache: &mut HashMap<Extension, Fluence<L>>,
    cone: ConeFrustum,
    scene: &PixelScene,
    view_dir: ViewDirection,
    simplify_occlusion: bool,
) -> I
where
    I: ResampleRadiance<L>,
{
    let start = cone.focus + cone.lower_dir();
    let lower_start = start + cone.lower_dir();
    let upper_start = start + cone.upper_dir();

    let near_ext = ConeExtension { start, end_bottom: start + cone.lower_dir() };
    let near =
        get_extension(ext_cache, near_ext.into(), scene, view_dir).resample::<I>(near_ext.as_frustum(), cone.into());
    let transmissions = if simplify_occlusion {
        [near.transmittance; 2]
    } else {
        [lower_start, upper_start]
            .map(|end| get_extension(ext_cache, LineExtension { start, end }.into(), scene, view_dir).transmittance)
    };
    let far_variants = [
        (transmissions[0], LineExtension { start: lower_start, end: lower_start + cone.lower_dir() }.into()),
        (transmissions[1], ConeExtension { start: upper_start, end_bottom: upper_start + cone.lower_dir() }.into()),
        // other way
        (transmissions[0], ConeExtension { start: lower_start, end_bottom: lower_start + cone.lower_dir() }.into()),
        (transmissions[1], LineExtension { start: upper_start, end: upper_start + cone.upper_dir() }.into()),
    ]
    .map(|(transmission, ext)| {
        get_extension(ext_cache, ext, scene, view_dir).resample::<I>(ext.as_frustum(), cone.into()).radiance
            * transmission
    });
    let far = far_variants.into_iter().reduce(|a, b| a + b).unwrap();
    near.radiance + far * 0.5
}

pub fn get_radiance_recursive_ext<L: Radiance, I: Radiance>(
    ext_cache: &mut HashMap<Extension, Fluence<L>>,
    cache: &mut HashMap<Frustum, I>,
    frustum: Frustum,
    scene: &PixelScene,
    settings: RenderExtSettings,
) -> I
where
    I: ResampleRadiance<L>,
{
    if let Some(&cached) = cache.get(&frustum) {
        return cached;
    }
    let Frustum::Cone(cone) = frustum else {
        unimplemented!();
    };
    if frustum.size() > 128 {
        // debug!("frustum too large: {:?}", frustum);
        return I::default();
    }

    let RenderExtSettings { simplify_occlusion } = settings;

    let view_dir = if cone.index >= 0 { ViewDirection::Up } else { ViewDirection::Down };

    let start = cone.focus + cone.lower_dir();
    let transmittance_between = |ext_cache: &mut HashMap<Extension, Fluence<L>>, f: Frustum| {
        let Frustum::Cone(c) = f else {
            unreachable!();
        };
        get_extension(ext_cache, LineExtension { start, end: c.focus + c.lower_dir() }.into(), scene, view_dir)
            .transmittance
    };
    let result = if frustum.parity() == 0 {
        let near_ext = ConeExtension { start, end_bottom: start + cone.lower_dir() };
        let near =
            get_extension(ext_cache, near_ext.into(), scene, view_dir).resample::<I>(near_ext.as_frustum(), frustum);
        let far = merge_even(frustum).into_iter().map(|(f, _weight)| {
            get_radiance_recursive_ext(ext_cache, cache, f, scene, settings).reproject(f, frustum)
                * if simplify_occlusion { near.transmittance } else { transmittance_between(ext_cache, f) }
        });

        let far = far.reduce(|a, b| a + b).unwrap();
        near.radiance + far
    } else {
        let short = {
            let near_ext = LineExtension { start, end: start + cone.lower_dir() + cone.upper_dir() };
            let near = get_extension(ext_cache, near_ext.into(), scene, view_dir)
                .resample::<I>(near_ext.as_frustum(), frustum);
            let far = merge_odd(frustum, false, true).into_iter().map(|(f, _weight)| {
                get_radiance_recursive_ext(ext_cache, cache, f, scene, settings).reproject(f, frustum)
            });

            let far = far.reduce(|a, b| a + b).unwrap();
            near.radiance * (-1.0) + far
        };
        let long = {
            let near = calculate_long_near::<L, I>(ext_cache, cone, scene, view_dir, simplify_occlusion);
            let far = merge_odd(frustum, true, true).into_iter().map(|(f, _weight)| {
                get_radiance_recursive_ext(ext_cache, cache, f, scene, settings).reproject(f, frustum)
                    * transmittance_between(ext_cache, f)
            });

            let far = far.reduce(|a, b| a + b).unwrap();
            near + far
        };
        short * 0.5 + long * 0.5
    };
    if result.at_focus(frustum).is_nan() {
        panic!("Radiance is NaN for frustum: {:?}", frustum);
    }

    cache.insert(frustum, result);
    result
}

// A size-1 cone's extensions start a column ahead of its focus, so the pixel it crosses first, which touches the probe,
// is never traced. Returns that pixel's transmittance and the emission the probe sees in it.
fn probe_pixel<L: Radiance>(cone: ConeFrustum, scene: &PixelScene) -> Fluence<Constant> {
    let pixel = cone.focus + IVec2::new(0, cone.index);
    if let Some(&value) = scene.get(&pixel) {
        // the probe is on the pixel's surface, so the pixel fills the whole cone. A face of emittance e seen across an
        // angle a gives e * a / 2, as get_extension weights each face 1/2 and cos / distance is its angle.
        return Fluence { transmittance: 0.0, radiance: Constant { value: value * 0.5 * FRAC_PI_4 } };
    }
    // an empty size-1 extension, but only the next pixel's face is inside the cone; the side pixel is the next pixel
    // of a cone in the rotated frame. The next pixel is the cone's first extension, which can't see its own emission.
    let frustum = Frustum::Cone(cone);
    let next_pixel = pixel + IVec2::X;
    let emittance = scene.get(&next_pixel).copied().unwrap_or(0.0);
    let face = pixel.as_vec2() + Vec2::new(1.0, 0.5);
    let emission = L::new_directional(frustum, face, ViewDir::Right) * emittance * 0.5;
    // read the emitter directly; it is too close for the far part's carrier to interpolate
    Fluence { transmittance: 1.0, radiance: Constant { value: emission.at_focus(frustum) } }
}

// Returns the total radiance arriving at each cell in [1, 128)^2.
pub fn render_ext<L: Radiance, I: Radiance>(scene: &PixelScene, settings: RenderExtSettings) -> HashMap<IVec2, f32>
where
    I: ResampleRadiance<L>,
{
    let mut caches = HashMap::<_, (HashMap<Extension, Fluence<L>>, HashMap<Frustum, I>)>::new();

    let rotate_transform = Affine2::from_translation(Vec2::splat(64.0))
        * Affine2::from_angle(FRAC_PI_2)
        * Affine2::from_translation(Vec2::splat(-64.0));
    let transforms = [
        Affine2::IDENTITY,
        rotate_transform,
        rotate_transform * rotate_transform,
        rotate_transform * rotate_transform * rotate_transform,
    ];
    let scenes = transforms.map(|t| {
        scene
            .iter()
            .map(|(k, v)| (t.transform_point2(k.as_vec2() + 0.5).floor().as_ivec2(), *v))
            .collect::<PixelScene>()
    });

    let mut image = HashMap::new();
    for cell in range2d(IVec2::splat(1), IVec2::splat(128)) {
        let mut total_radiance = 0.0;
        for (k, (t, scene)) in transforms.iter().zip(&scenes).enumerate() {
            let probe = t.transform_point2(cell.as_vec2());
            assert!((probe - probe.round()).length() < 0.001, "{:?}", probe);
            let probe = probe.round();
            for i in [-1, 0] {
                let cone = ConeFrustum { focus: probe.as_ivec2(), size: 1, index: i };
                let frustum = Frustum::Cone(cone);

                let near = probe_pixel::<L>(cone, scene);
                if near.transmittance == 0.0 {
                    total_radiance += near.radiance.value;
                    continue;
                }
                let (ext_cache, cache) = caches.entry((k, i)).or_default();
                let far = get_radiance_recursive_ext(ext_cache, cache, frustum, scene, settings);
                let far = Fluence { transmittance: 1.0, radiance: Constant { value: far.at_focus(frustum) } };
                total_radiance += near.over(far).radiance.value;
            }
        }
        image.insert(cell, total_radiance);
    }
    image
}
