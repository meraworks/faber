//! Restore source circles and arcs after the polygon union.
use super::{Bounds, Ring, area, perimeter};
use faber_geometry::sketch_loops::{rounded, slot};
use faber_model::{SketchLoop, SketchSegment};
use faber_ports::vector::Path2;
use glam::{DAffine2, DVec2};
use std::{
    collections::HashMap,
    f64::consts::{PI, TAU},
};

#[derive(Clone, Copy)]
struct Chord {
    from: DVec2,
    to: DVec2,
    curve: usize,
}
#[derive(Clone, Copy)]
struct Circle {
    centre: DVec2,
    radius: f64,
}
pub(super) struct Sources {
    tolerance: f64,
    circles: Vec<Circle>,
    chords: HashMap<(i64, i64), Vec<Chord>>,
}
impl Sources {
    pub(super) fn new(span: f64) -> Self {
        // The union scales coordinates to a 30-bit integer grid.
        Self {
            tolerance: (span * 1e-8).max(1e-10),
            circles: vec![],
            chords: HashMap::new(),
        }
    }
    fn key(&self, p: DVec2) -> (i64, i64) {
        (
            (p.x / self.tolerance).floor() as i64,
            (p.y / self.tolerance).floor() as i64,
        )
    }
    fn add(&mut self, centre: DVec2, radius: f64, points: Ring, frame: DAffine2) {
        let curve = self.circles.len();
        self.circles.push(Circle {
            centre: frame.transform_point2(centre),
            radius,
        });
        for pair in points.windows(2) {
            let [from, to] = [pair[0], pair[1]].map(|p| frame.transform_point2(p.into()));
            let key = self.key((from + to) * 0.5);
            self.chords
                .entry(key)
                .or_default()
                .push(Chord { from, to, curve });
        }
    }
    pub(super) fn collect(&mut self, profile: &SketchLoop, frame: DAffine2) -> Result<(), String> {
        if let Some(source) = rounded::lower(profile)? {
            return self.collect(&source, frame);
        }
        match profile {
            SketchLoop::Circle { centre, radius } => {
                let mut samples = crate::sample_loop(profile)?;
                samples.push(samples[0]);
                self.add((*centre).into(), *radius, samples, frame);
            }
            SketchLoop::Slot {
                centre,
                length,
                width,
                rotation,
            } => {
                self.collect(&slot::boundary(*centre, *length, *width, *rotation)?, frame)?;
            }
            SketchLoop::Path { start, segments } => {
                let mut a = DVec2::from(*start);
                for segment in segments {
                    match segment {
                        SketchSegment::Arc { via, end } => {
                            let b = DVec2::from(*end);
                            let d = DVec2::from(*via) - a;
                            let e = b - a;
                            let centre = a
                                + (e.perp() * d.length_squared() - d.perp() * e.length_squared())
                                    / (-2. * d.perp_dot(e));
                            let mut points = vec![a.to_array()];
                            crate::tree::arc(&mut points, a, (*via).into(), b)?;
                            self.add(centre, centre.distance(a), points, frame);
                            a = b;
                        }
                        SketchSegment::Line { end } | SketchSegment::Bezier { end, .. } => {
                            a = (*end).into()
                        }
                        SketchSegment::Spline { .. } => {
                            return Err(
                                "The flat pattern cannot develop an interpolated spline.".into()
                            );
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn find(&self, a: DVec2, b: DVec2) -> Option<Chord> {
        let (x, y) = self.key((a + b) * 0.5);
        let eps = self.tolerance * self.tolerance;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(chords) = self.chords.get(&(x + dx, y + dy)) {
                    for &c in chords {
                        if a.distance_squared(c.from) <= eps && b.distance_squared(c.to) <= eps {
                            return Some(c);
                        }
                        if a.distance_squared(c.to) <= eps && b.distance_squared(c.from) <= eps {
                            return Some(Chord {
                                from: c.to,
                                to: c.from,
                                curve: c.curve,
                            });
                        }
                    }
                }
            }
        }
        None
    }
    pub(super) fn trace(&self, ring: &Ring, paths: &mut Vec<Path2>) -> (f64, f64, Bounds) {
        let n = ring.len();
        let mut points = ring.clone();
        let mut curve = vec![None; n];
        for i in 0..n {
            let next = (i + 1) % n;
            if let Some(chord) = self.find(ring[i].into(), ring[next].into()) {
                points[i] = chord.from.to_array();
                points[next] = chord.to.to_array();
                curve[i] = Some(chord.curve);
            }
        }
        let mut bounds = Bounds::of(&points);
        let mut signed_area = area(&points);
        let mut length = perimeter(&points);
        let Some(first) = (0..n).find(|&i| curve[i] != curve[(i + n - 1) % n]) else {
            if let Some(id) = curve[0] {
                let Circle { centre, radius } = self.circles[id];
                paths.push(Path2::Circle {
                    centre: centre.to_array(),
                    radius,
                });
                return (
                    PI * radius * radius,
                    TAU * radius,
                    Bounds {
                        low: centre - radius,
                        high: centre + radius,
                    },
                );
            }
            paths.push(Path2::Polyline {
                points,
                closed: true,
            });
            return (signed_area.abs(), length, bounds);
        };
        let mut i = 0;
        while i < n {
            let k = (first + i) % n;
            let count = (i + 1..n)
                .take_while(|j| curve[(first + *j) % n] == curve[k])
                .count()
                + 1;
            let run: Vec<_> = (0..=count).map(|j| points[(k + j) % n]).collect();
            if let Some(id) = curve[k] {
                let circle = self.circles[id];
                let mut sweep = 0.;
                for pair in run.windows(2) {
                    let a = DVec2::from(pair[0]) - circle.centre;
                    let b = DVec2::from(pair[1]) - circle.centre;
                    let angle = a.perp_dot(b).atan2(a.dot(b));
                    signed_area += 0.5 * circle.radius.powi(2) * (angle - angle.sin());
                    length += circle.radius * angle.abs() - a.distance(b);
                    sweep += angle;
                }
                let ends = [run[0], *run.last().unwrap()]
                    .map(|p| (DVec2::from(p) - circle.centre).to_angle());
                let [start, end] = if sweep > 0. { ends } else { [ends[1], ends[0]] };
                for j in 0..4 {
                    let a = f64::from(j) * std::f64::consts::FRAC_PI_2;
                    if (a - start).rem_euclid(TAU) <= sweep.abs() {
                        let p = circle.centre + circle.radius * DVec2::from_angle(a);
                        bounds.low = bounds.low.min(p);
                        bounds.high = bounds.high.max(p);
                    }
                }
                paths.push(Path2::Arc {
                    centre: circle.centre.to_array(),
                    radius: circle.radius,
                    start_deg: start.to_degrees(),
                    end_deg: end.to_degrees(),
                });
            } else {
                paths.push(Path2::Polyline {
                    points: run,
                    closed: false,
                });
            }
            i += count;
        }
        (signed_area.abs(), length, bounds)
    }
}
