use super::{
    Constraint, Entity, P, Result, Sketch, add, cross, dot, fresh, insert_point, length, mul, sub,
    unit,
};
use std::f64::consts::TAU;

const EPS: f64 = 1e-9;

#[cfg(test)]
mod boundary_tests {
    use super::*;
    use crate::profile::Segment;
    #[test]
    fn shared_ends_are_allowed_and_overlaps_are_refused() {
        let line = |a, b| Segment::Line {
            start: [a, 0.],
            end: [b, 0.],
        };
        assert!(!boundaries_cross(&line(0., 2.), &line(2., 5.), true));
        assert!(boundaries_cross(&line(0., 2.), &line(2., 5.), false));
        assert!(boundaries_cross(&line(0., 2.), &line(3., 1.), true));
        let arc = |angle, sweep| Segment::Arc {
            centre: [0.; 2],
            radius: 2.,
            angle,
            sweep,
        };
        let pi = std::f64::consts::PI;
        assert!(!boundaries_cross(&arc(0., pi), &arc(0., -pi), true));
        assert!(boundaries_cross(&arc(0., pi), &arc(0., -pi), false));
        assert!(boundaries_cross(&arc(-0.5, 1.), &arc(0.6, -1.2), true));
        assert!(!boundaries_cross(&arc(-0.5, 1.), &arc(2., 1.), false));
    }
    #[test]
    fn long_edges_can_cross_at_a_small_angle() {
        let horizontal = Segment::Line {
            start: [0., 0.],
            end: [1e6, 0.],
        };
        let diagonal = Segment::Line {
            start: [0., -0.0001],
            end: [1e6, 0.0001],
        };
        assert!(boundaries_cross(&horizontal, &diagonal, false));
    }
}

#[derive(Clone, Copy)]
enum Curve {
    Line {
        origin: P,
        delta: P,
    },
    Round {
        centre: P,
        radius: f64,
        start: f64,
        sweep: f64,
    },
}

fn angle(v: P) -> f64 {
    v[1].atan2(v[0])
}
fn turn(z: f64) -> f64 {
    z.rem_euclid(TAU)
}

impl Curve {
    fn profile(edge: &crate::profile::Segment) -> Self {
        use crate::profile::Segment;
        match *edge {
            Segment::Line { start, end } => Self::Line {
                origin: start,
                delta: sub(end, start),
            },
            Segment::Circle { centre, radius } => Self::Round {
                centre,
                radius,
                start: 0.,
                sweep: TAU,
            },
            Segment::Arc {
                centre,
                radius,
                angle,
                sweep,
            } => Self::Round {
                centre,
                radius,
                start: if sweep < 0. { angle + sweep } else { angle },
                sweep: sweep.abs(),
            },
        }
    }

    fn from(s: &Sketch, e: &Entity) -> Self {
        match e {
            Entity::Line { a, b } => Self::Line {
                origin: s.points[a],
                delta: sub(s.points[b], s.points[a]),
            },
            Entity::Circle { centre, radius } => Self::Round {
                centre: s.points[centre],
                radius: *radius,
                start: 0.,
                sweep: TAU,
            },
            Entity::Arc { centre, start, end } => {
                let c = s.points[centre];
                let u = sub(s.points[start], c);
                let v = sub(s.points[end], c);
                Self::Round {
                    centre: c,
                    radius: length(u),
                    start: angle(u),
                    sweep: turn(angle(v) - angle(u)),
                }
            }
        }
    }
    fn at(self, t: f64) -> P {
        match self {
            Self::Line { origin, delta } => add(origin, mul(delta, t)),
            Self::Round {
                centre,
                radius,
                start,
                sweep,
            } => {
                let a = start + t * sweep;
                add(centre, [radius * a.cos(), radius * a.sin()])
            }
        }
    }
    fn parameter(self, p: P) -> f64 {
        match self {
            Self::Line { origin, delta } => dot(sub(p, origin), delta) / dot(delta, delta),
            Self::Round {
                centre,
                start,
                sweep,
                ..
            } => {
                let t = turn(angle(sub(p, centre)) - start);
                if TAU - t < EPS { 0. } else { t / sweep }
            }
        }
    }
}

fn intersections(a: Curve, b: Curve) -> Vec<P> {
    match (a, b) {
        (
            Curve::Line {
                origin: p,
                delta: u,
            },
            Curve::Line {
                origin: q,
                delta: v,
            },
        ) => {
            let det = cross(u, v);
            if det.abs() <= 32. * f64::EPSILON * length(u) * length(v) {
                Vec::new()
            } else {
                vec![add(p, mul(u, cross(sub(q, p), v) / det))]
            }
        }
        (Curve::Round { centre, radius, .. }, Curve::Line { origin, delta })
        | (Curve::Line { origin, delta }, Curve::Round { centre, radius, .. }) => {
            let Ok(axis) = unit(delta) else {
                return vec![];
            };
            let relative = sub(centre, origin);
            let nearest = dot(relative, axis);
            let normal = cross(relative, axis);
            let h2 = (radius - normal.abs()) * (radius + normal.abs());
            if h2 < -EPS * radius.max(1.) {
                return vec![];
            }
            let h = h2.max(0.).sqrt();
            [nearest - h, nearest + h]
                .map(|t| add(origin, mul(axis, t)))
                .to_vec()
        }
        (
            Curve::Round {
                centre: p,
                radius: r,
                ..
            },
            Curve::Round {
                centre: q,
                radius: s,
                ..
            },
        ) => {
            let delta = sub(q, p);
            let dist = length(delta);
            if dist <= EPS || dist > r + s + EPS || dist < (r - s).abs() - EPS {
                return vec![];
            }
            let d = mul(delta, 1. / dist);
            let along = (dist + (r - s) * (r + s) / dist) * 0.5;
            let offset = ((r - along) * (r + along)).max(0.).sqrt();
            let centre = add(p, mul(d, along));
            let normal = mul([-d[1], d[0]], offset);
            vec![add(centre, normal), sub(centre, normal)]
        }
    }
}

pub(crate) fn boundaries_cross(
    left: &crate::profile::Segment,
    right: &crate::profile::Segment,
    joined: bool,
) -> bool {
    let a = Curve::profile(left);
    let b = Curve::profile(right);
    let contact = |overlap: f64, tol: f64| overlap > tol || (!joined && overlap >= -tol);
    let tol = 1e-8;
    match (a, b) {
        (
            Curve::Line {
                origin: p,
                delta: u,
            },
            Curve::Line {
                origin: q,
                delta: v,
            },
        ) if cross(u, v).abs() <= 32. * f64::EPSILON * length(u) * length(v) => {
            let Ok(axis) = unit(u) else {
                return true;
            };
            if cross(axis, sub(q, p)).abs() > tol {
                return false;
            }
            let from = dot(sub(q, p), axis);
            let to = from + dot(v, axis);
            return contact(length(u).min(from.max(to)) - 0_f64.max(from.min(to)), tol);
        }
        (
            Curve::Round {
                centre: p,
                radius: r,
                start: sa,
                sweep: da,
            },
            Curve::Round {
                centre: q,
                radius: s,
                start: sb,
                sweep: db,
            },
        ) if length(sub(p, q)) <= tol && (r - s).abs() <= tol => {
            let start = turn(sb - sa);
            return [-TAU, 0., TAU].into_iter().any(|shift| {
                let lo = start + shift;
                contact(da.min(lo + db) - 0_f64.max(lo), tol / r)
            });
        }
        _ => {}
    }
    let on = |curve: Curve, p: P| {
        let t = curve.parameter(p);
        (0. ..=1.).contains(&t)
            || length(sub(curve.at(t.clamp(0., 1.)), p)) <= tol
            || length(sub(curve.at(0.), p)) <= tol
    };
    let end = |curve: Curve, p: P| {
        length(sub(curve.at(0.), p)) <= tol || length(sub(curve.at(1.), p)) <= tol
    };
    intersections(a, b)
        .into_iter()
        .any(|p| on(a, p) && on(b, p) && !(joined && end(a, p) && end(b, p)))
}

struct Hit {
    t: f64,
    other: String,
}

fn crossings(s: &Sketch, id: &str, source: Curve) -> Vec<Hit> {
    let mut hits = Vec::new();
    for (other, e) in &s.entities {
        if other == id {
            continue;
        }
        let curve = Curve::from(s, e);
        for p in intersections(source, curve) {
            let t = curve.parameter(p);
            if (-EPS..=1. + EPS).contains(&t) {
                hits.push(Hit {
                    t: source.parameter(p),
                    other: other.clone(),
                });
            }
        }
    }
    hits.sort_by(|a, b| a.t.total_cmp(&b.t).then_with(|| a.other.cmp(&b.other)));
    hits.dedup_by(|a, b| (a.t - b.t).abs() < EPS);
    hits
}

fn attach<N>(s: &mut Sketch<N>, point: &str, target: &str) {
    s.constraints
        .push(if matches!(s.entities[target], Entity::Line { .. }) {
            Constraint::PointOnLine {
                point: point.into(),
                line: target.into(),
            }
        } else {
            Constraint::PointOnCircle {
                point: point.into(),
                circle: target.into(),
            }
        });
}

pub(super) fn extend<N: Clone + From<f64>>(
    s: &mut Sketch<N>,
    geo: &Sketch,
    line: &str,
    end: &str,
) -> Result<()> {
    let Some(Entity::Line { a, b }) = geo.entities.get(line) else {
        return Err("Extend needs a straight line.".into());
    };
    let other = if end == a {
        b
    } else if end == b {
        a
    } else {
        return Err(format!("Point {end} is not an end of line {line}."));
    };
    let here = geo.points[end];
    let ray = Curve::Line {
        origin: here,
        delta: unit(sub(here, geo.points[other]))?,
    };
    let hit = crossings(geo, line, ray)
        .into_iter()
        .find(|h| h.t > EPS)
        .ok_or("There is no intersection beyond the end of the line.")?;
    let target = ray.at(hit.t);
    let shared = geo
        .entities
        .iter()
        .any(|(id, e)| id != line && super::point_refs(e).contains(&end));
    let dest = if shared {
        let name = insert_point(s, &format!("{end}_extended"), target);
        let Some(Entity::Line { a, b }) = s.entities.get_mut(line) else {
            unreachable!()
        };
        if a == end {
            a.clone_from(&name);
        } else {
            b.clone_from(&name);
        }
        name
    } else {
        s.points.insert(end.into(), target.map(N::from));
        end.into()
    };
    attach(s, &dest, &hit.other);
    Ok(())
}

pub(super) fn trim<N: Clone + From<f64>>(
    s: &mut Sketch<N>,
    geo: &Sketch,
    id: &str,
    near: P,
) -> Result<()> {
    if !near.into_iter().all(f64::is_finite) {
        return Err("The trim point must be finite.".into());
    }
    let original = s
        .entities
        .get(id)
        .cloned()
        .ok_or_else(|| format!("Unknown sketch entity: {id}"))?;
    let source = Curve::from(geo, &geo.entities[id]);
    let closed = matches!(original, Entity::Circle { .. });
    let hits: Vec<_> = crossings(geo, id, source)
        .into_iter()
        .filter(|h| {
            if closed {
                true
            } else {
                h.t > EPS && h.t < 1. - EPS
            }
        })
        .collect();
    if hits.is_empty() || (closed && hits.len() < 2) {
        return Err("There are too few intersections to trim this entity.".into());
    }
    let t = source.parameter(near).clamp(0., 1.);
    let right = hits.partition_point(|h| h.t <= t);
    let mut cut = |h: &Hit| {
        let p = insert_point(s, &format!("{id}_cut"), source.at(h.t));
        attach(s, &p, &h.other);
        p
    };
    let mut pieces = Vec::new();
    if closed {
        let before = if right == 0 {
            hits.len() - 1
        } else {
            right - 1
        };
        let after = right % hits.len();
        // The kept arc runs counter-clockwise around the rest of the circle.
        pieces.push((cut(&hits[after]), cut(&hits[before])));
    } else {
        let ends = match &original {
            Entity::Line { a, b } => (a, b),
            Entity::Arc { start, end, .. } => (start, end),
            _ => unreachable!(),
        };
        if right > 0 {
            pieces.push((ends.0.clone(), cut(&hits[right - 1])));
        }
        if right < hits.len() {
            pieces.push((cut(&hits[right]), ends.1.clone()));
        }
    }
    for (i, (a, b)) in pieces.into_iter().enumerate() {
        let part = if i == 0 {
            id.into()
        } else {
            fresh(s, &format!("{id}_trim"))
        };
        let e = match &original {
            Entity::Line { .. } => Entity::Line { a, b },
            Entity::Circle { centre, .. } | Entity::Arc { centre, .. } => Entity::Arc {
                centre: centre.clone(),
                start: a,
                end: b,
            },
        };
        if i > 0 {
            if s.construction.iter().any(|k| k == id) {
                s.construction.push(part.clone());
            }
            if matches!(e, Entity::Line { .. }) {
                s.constraints.push(Constraint::Collinear {
                    a: id.into(),
                    b: part.clone(),
                });
            }
        }
        s.entities.insert(part, e);
    }
    Ok(())
}
