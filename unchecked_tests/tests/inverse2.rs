use glam::{IVec2, Vec2};
use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use twilight::frustum::{ConeFrustum, Frustum, FrustumType, merge_even, merge_odd};
use twilight::radiance::{
    AveragedLinear, Constant, Gradient, Hessian, Inverse2, Quadratic, QuadraticProjected, Radiance,
};

// A chain of cone frustums from a size-1 probe down to size 64, each a random even / near-odd / far-odd child of the
// one before, as render's recursion would visit them.
fn random_chain(rng: &mut StdRng) -> Vec<Frustum> {
    let mut chain = vec![Frustum::Cone(ConeFrustum {
        focus: IVec2::new(0, 0),
        size: 1,
        index: rng.random_range(-1..1),
    })];
    while chain.last().unwrap().size() < 64 {
        let frustum = *chain.last().unwrap();
        let children = match rng.random_range(0..3) {
            0 => merge_even(frustum),
            1 => merge_odd(frustum, false, true),
            _ => merge_odd(frustum, true, true),
        };
        chain.push(children[rng.random_range(0..children.len())].0);
    }
    chain
}

// An emitter in the last frustum, between 2 and 4 of its sizes ahead (render's default frustum start is 2).
fn random_emitter(rng: &mut StdRng, frustum: Frustum) -> Vec2 {
    let segment = frustum.at(rng.random_range(2.0..4.0));
    segment.start + (segment.end - segment.start) * rng.random_range(0.0..1.0)
}

// The emitter's transmission built at the end of the chain and reprojected back to its start, against the exact
// 1 / distance from the start's focus: (mean, worst) relative error.
fn chain_error<I: Radiance>(cases: &[(Vec<Frustum>, Vec2)]) -> (f32, f32) {
    let mut sum = 0.0;
    let mut worst: f32 = 0.0;
    for (chain, emitter) in cases {
        let mut value = I::new(*chain.last().unwrap(), *emitter);
        for pair in chain.windows(2).rev() {
            value = value.reproject(pair[1], pair[0]);
        }
        let exact = 1.0 / (*emitter - chain[0].focus()).length();
        let error = ((value.at_focus(chain[0]) - exact) / exact).abs();
        sum += error;
        worst = worst.max(error);
    }
    (sum / cases.len() as f32, worst)
}

#[test]
fn inverse2_reprojection_matches_distance() {
    let mut rng = StdRng::seed_from_u64(7);
    let cases: Vec<_> = (0..2000)
        .map(|_| {
            let chain = random_chain(&mut rng);
            let emitter = random_emitter(&mut rng, *chain.last().unwrap());
            (chain, emitter)
        })
        .collect();

    let results = [
        ("Constant", chain_error::<Constant>(&cases)),
        ("Quadratic", chain_error::<Quadratic>(&cases)),
        ("QuadraticProjected", chain_error::<QuadraticProjected>(&cases)),
        ("Gradient", chain_error::<Gradient>(&cases)),
        ("Hessian", chain_error::<Hessian>(&cases)),
        ("AveragedLinear", chain_error::<AveragedLinear>(&cases)),
        ("Inverse2", chain_error::<Inverse2>(&cases)),
    ];
    for (name, (mean, worst)) in results {
        println!("{name:>20}: mean relative error {mean:.5}, worst {worst:.5}");
    }
    let (mean, worst) = results.last().unwrap().1;
    assert!(
        mean < 1e-3 && worst < 5e-3,
        "Inverse2 error too large: mean {mean}, worst {worst}"
    );
}

// Sums and scales act on the samples, so an Inverse2 of two emitters is the sum of theirs.
#[test]
fn inverse2_is_linear() {
    let frustum = Frustum::Cone(ConeFrustum {
        focus: IVec2::new(4, 2),
        size: 8,
        index: 3,
    });
    let a = Vec2::new(30.0, 10.0);
    let b = Vec2::new(26.0, 16.0);
    let sum = Inverse2::new(frustum, a) * 2.0 + Inverse2::new(frustum, b);
    let expected = [0, 1].map(|i| Inverse2::new(frustum, a).samples[i] * 2.0 + Inverse2::new(frustum, b).samples[i]);
    assert_eq!(sum.samples, expected);
}
