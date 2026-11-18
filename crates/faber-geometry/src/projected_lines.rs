//! Merge coincident straight edges in an orthographic view.
use faber_ports::drawing::{Curve2, Projection2};
use glam::DVec2;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct ProjectedLines(BTreeMap<[i64; 3], LineGroup>);

impl ProjectedLines {
    pub fn add(&mut self, a: [f64; 2], b: [f64; 2], hidden: bool) {
        line(&mut self.0, a, b, usize::from(hidden));
    }

    pub fn append_to(self, projection: &mut Projection2) {
        for group in self.0.into_values() {
            group.finish(projection);
        }
    }
}

fn quant(n: f64) -> i64 {
    (n * 1e7).round() as i64
}
struct LineGroup {
    axis: DVec2,
    origin: DVec2,
    spans: [Vec<[f64; 2]>; 2],
}
fn line(groups: &mut BTreeMap<[i64; 3], LineGroup>, a: [f64; 2], b: [f64; 2], style: usize) {
    let (a, b) = (DVec2::from(a), DVec2::from(b));
    let Some(mut axis) = (b - a).try_normalize() else {
        return;
    };
    if axis.x < -1e-9 || (axis.x.abs() <= 1e-9 && axis.y < 0.) {
        axis = -axis;
    }
    let normal = axis.perp();
    let distance = normal.dot(a);
    let group = groups
        .entry([quant(axis.x), quant(axis.y), quant(distance)])
        .or_insert_with(|| LineGroup {
            axis,
            origin: normal * distance,
            spans: Default::default(),
        });
    let (t, u) = (group.axis.dot(a), group.axis.dot(b));
    if (t - u).abs() > 1e-8 {
        group.spans[style].push([t.min(u), t.max(u)]);
    }
}
fn merge(mut ranges: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    ranges.sort_by(|a, b| a[0].total_cmp(&b[0]));
    let mut merged: Vec<[f64; 2]> = vec![];
    for range in ranges {
        if let Some(prior) = merged.last_mut()
            && range[0] <= prior[1] + 1e-8
        {
            prior[1] = prior[1].max(range[1]);
        } else {
            merged.push(range);
        }
    }
    merged
}
impl LineGroup {
    fn finish(self, p: &mut Projection2) {
        let [visible, hidden] = self.spans;
        let curve = |a: f64, b: f64| {
            Curve2::Line([
                (self.origin + self.axis * a).into(),
                (self.origin + self.axis * b).into(),
            ])
        };
        let visible = merge(visible);
        p.visible.extend(visible.iter().map(|&[a, b]| curve(a, b)));
        for [mut a, b] in merge(hidden) {
            let first = visible.partition_point(|r| r[1] <= a);
            for &[v, w] in &visible[first..] {
                if v >= b {
                    break;
                }
                if v > a + 1e-8 {
                    p.hidden.push(curve(a, v));
                }
                a = a.max(w);
            }
            if b > a + 1e-8 {
                p.hidden.push(curve(a, b));
            }
        }
    }
}
pub fn bounds(p: &Projection2) -> [[f64; 2]; 2] {
    let mut low = DVec2::splat(f64::INFINITY);
    let mut high = -low;
    let mut point = |v: DVec2| {
        low = low.min(v);
        high = high.max(v);
    };
    for c in p.visible.iter().chain(&p.hidden) {
        match c {
            Curve2::Line(ends) => ends.iter().for_each(|&v| point(v.into())),
            Curve2::Polyline(points) => points.iter().for_each(|&v| point(v.into())),
            Curve2::Circle { centre, radius } => {
                point(DVec2::from(*centre) - *radius);
                point(DVec2::from(*centre) + *radius);
            }
            Curve2::Arc {
                centre,
                radius,
                start_deg,
                end_deg,
            } => {
                let mut angle = |a: f64| {
                    point(DVec2::from(*centre) + radius * DVec2::from_angle(a.to_radians()))
                };
                angle(*start_deg);
                angle(*end_deg);
                for a in [0., 90., 180., 270.] {
                    if (a - start_deg).rem_euclid(360.) <= end_deg - start_deg {
                        angle(a);
                    }
                }
            }
        }
    }
    if low.is_finite() {
        [low.into(), high.into()]
    } else {
        [[0.; 2]; 2]
    }
}
