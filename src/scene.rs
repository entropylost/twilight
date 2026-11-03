use glam::{Affine2, Vec2};

use crate::{
    frustum::Frustum,
    shape::{LineSegment, Polygon},
    transmission::Transmission,
};

#[derive(Debug, Clone, Copy)]
pub struct Emitter {
    pub brightness: f32,
    pub position: Vec2,
}

#[derive(Debug, Clone)]
pub struct Scene {
    pub occluders: Vec<Polygon>,
    pub emitters: Vec<Emitter>,
}

impl Scene {
    // The scene shown on startup.
    pub fn example() -> Self {
        Self {
            occluders: vec![Polygon::new(&[
                Vec2::new(60.0, 60.0),
                Vec2::new(63.5, 70.0),
                Vec2::new(56.5, 80.0),
            ])],
            emitters: vec![
                Emitter {
                    brightness: 0.5,
                    position: Vec2::new(40.0, 65.0),
                },
                Emitter {
                    brightness: 1.0,
                    position: Vec2::new(58.6, 108.3),
                },
            ],
        }
    }
    pub fn transform(&self, transform: Affine2) -> Self {
        Self {
            occluders: self.occluders.iter().map(|p| p.transform(transform)).collect(),
            emitters: self
                .emitters
                .iter()
                .map(|emitter| Emitter {
                    brightness: emitter.brightness,
                    position: transform.transform_point2(emitter.position),
                })
                .collect(),
        }
    }
    // Returns true if the line segment is not blocked by any occluder.
    pub fn trace(&self, ray: LineSegment) -> bool {
        self.occluders.iter().all(|occluder| {
            // If the ray crosses no edge it is either entirely inside or entirely outside the occluder.
            occluder.iter_edges().all(|edge| ray.intersect(edge).is_none()) && !occluder.contains(ray.start)
        })
    }
    // Traces all lines bundle.at(t) for t in [0, 1] and returns the amount of the segment that is unblocked.
    // The result is measured in t, so it lies in [0, 1].
    pub fn trace_bundle(&self, bundle: RayBundle) -> f32 {
        // Whether a ray is blocked can only change at a t where an endpoint crosses an occluder edge, or where the ray
        // sweeps over an occluder vertex. Visibility is constant between consecutive such events, so tracing a single
        // ray per interval gives the exact answer.
        let mut events = vec![0.0, 1.0];
        for occluder in &self.occluders {
            for edge in occluder.iter_edges() {
                events.extend(bundle.start.intersect_t(edge).map(|(t, _)| t));
                events.extend(bundle.end.intersect_t(edge).map(|(t, _)| t));
            }
            // A twisted bundle can sweep over a vertex twice, so every crossing is an event.
            events.extend(occluder.vertices.iter().flat_map(|&vertex| bundle.offsets_at(vertex)));
        }
        events.sort_by(f32::total_cmp);

        events
            .windows(2)
            .filter(|interval| {
                let t = (interval[0] + interval[1]) * 0.5;
                self.trace(bundle.at(t))
            })
            .map(|interval| interval[1] - interval[0])
            .sum()
    }
    pub fn emitters_within(&self, area: &Polygon) -> impl Iterator<Item = &Emitter> {
        self.emitters
            .iter()
            .filter(move |emitter| area.contains_one_sided(emitter.position))
    }
    pub fn visible_emitters_within(&self, area: &Polygon, bundle: RayBundle) -> impl Iterator<Item = &Emitter> {
        self.emitters.iter().filter(move |emitter| {
            area.contains_one_sided(emitter.position)
                && self.trace(LineSegment::new(
                    bundle.at(bundle.offset_at(emitter.position).unwrap()).start,
                    emitter.position,
                ))
        })
    }
    pub fn emission_within<T: Transmission>(&self, source: Frustum, area: &Polygon, bundle: RayBundle) -> T {
        let mut emission = T::default();
        for emitter in self.visible_emitters_within(area, bundle) {
            emission = emission + T::new(source, emitter.position) * emitter.brightness;
        }
        emission
    }
}

// Roots of a * t^2 + b * t + c = 0. Missing roots (no real solutions, or a degenerate equation) come back as NaN or
// infinity. Uses the numerically stable form, which also handles a = 0 without a special case.
fn solve_quadratic(a: f32, b: f32, c: f32) -> [f32; 2] {
    let q = -0.5 * (b + b.signum() * (b * b - 4.0 * a * c).sqrt());
    [q / a, c / q]
}

// TODO: Should this support non-linear bundles? It makes stuff quite a bit messier.
#[derive(Debug, Clone, Copy)]
pub struct RayBundle {
    pub start: LineSegment,
    pub end: LineSegment,
}
impl RayBundle {
    pub fn new(start: LineSegment, end: LineSegment) -> Self {
        Self { start, end }
    }
    pub fn at(&self, t: f32) -> LineSegment {
        LineSegment::new(self.start.at(t), self.end.at(t))
    }
    pub fn as_polygon(&self) -> Polygon {
        Polygon::new(&[self.start.start, self.end.start, self.end.end, self.start.end])
    }
    // find t such that self.at(t) contains point.
    // If the bundle twists over itself and covers the point twice, returns the smaller t.
    pub fn offset_at(&self, point: Vec2) -> Option<f32> {
        self.offsets_at(point).next()
    }
    // All t (at most two, in increasing order) such that self.at(t) contains point.
    pub fn offsets_at(&self, point: Vec2) -> impl Iterator<Item = f32> + '_ {
        // With P(t) = start.at(t) = start.start + t * d and Q(t) - P(t) = w + t * k, the ray passes through the point
        // when (Q(t) - P(t)) × (point - P(t)) = 0. Writing point - P(t) = c - t * d, this expands to a quadratic in t.
        let d = self.start.direction();
        let w = self.end.start - self.start.start;
        let k = self.end.direction() - d;
        let c = point - self.start.start;
        let mut roots = solve_quadratic(-k.perp_dot(d), k.perp_dot(c) - w.perp_dot(d), w.perp_dot(c));
        roots.sort_by(f32::total_cmp);
        // The NaN / infinite roots of degenerate quadratics fail the range check.
        roots.into_iter().filter(move |t| {
            if !(0.0..=1.0).contains(t) {
                return false;
            }
            // The point is on the ray's line; check that it lies between the endpoints. A zero-length ray satisfies
            // the quadratic for every point, so it is rejected.
            let ray = self.at(*t);
            let dir = ray.direction();
            let s = (point - ray.start).dot(dir);
            dir != Vec2::ZERO && (0.0..=dir.length_squared()).contains(&s)
        })
    }
}
