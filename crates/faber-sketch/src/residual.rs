use crate::{Constraint, Entity, Result, Sketch};
use std::collections::{BTreeMap, BTreeSet};

/// A value and its non-zero partial derivatives.
#[derive(Clone)]
pub(super) struct Jet {
    pub f: f64,
    pub d: Vec<(usize, f64)>,
}

impl Jet {
    fn variable(f: f64, index: usize) -> Self {
        Self {
            f,
            d: vec![(index, 1.)],
        }
    }
    fn combine(&self, other: &Self, value: f64, left: f64, right: f64) -> Self {
        let mut d: Vec<_> = self.d.iter().map(|&(i, v)| (i, v * left)).collect();
        for &(j, v) in &other.d {
            if let Some((_, entry)) = d.iter_mut().find(|(i, _)| *i == j) {
                *entry += v * right;
            } else {
                d.push((j, v * right));
            }
        }
        d.retain(|(_, v)| *v != 0.);
        Self { f: value, d }
    }
    fn add(&self, b: &Self) -> Self {
        self.combine(b, self.f + b.f, 1., 1.)
    }
    fn sub(&self, b: &Self) -> Self {
        self.combine(b, self.f - b.f, 1., -1.)
    }
    fn mul(&self, b: &Self) -> Self {
        self.combine(b, self.f * b.f, b.f, self.f)
    }
    fn div(&self, b: &Self) -> Self {
        self.combine(b, self.f / b.f, 1. / b.f, -(self.f / b.f) / b.f)
    }
    fn scale(mut self, k: f64) -> Self {
        self.f *= k;
        for (_, d) in &mut self.d {
            *d *= k;
        }
        self
    }
    fn minus(mut self, k: f64) -> Self {
        self.f -= k;
        self
    }
}
type Point = [Jet; 2];
fn difference(a: &Point, b: &Point) -> Point {
    [a[0].sub(&b[0]), a[1].sub(&b[1])]
}
fn dot(a: &Point, b: &Point) -> Jet {
    a[0].mul(&b[0]).add(&a[1].mul(&b[1]))
}
fn cross(a: &Point, b: &Point) -> Jet {
    a[0].mul(&b[1]).sub(&a[1].mul(&b[0]))
}
fn angle_error(a: &Point, b: &Point, target: f64) -> Result<Jet> {
    let (a, b) = (unit(a)?, unit(b)?);
    let (y, x) = (cross(&a, &b), dot(&a, &b));
    let length = x.f * x.f + y.f * y.f;
    let delta = (y.f.atan2(x.f) - target + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI;
    Ok(y.combine(&x, delta, x.f / length, -y.f / length))
}
fn norm(v: &Point) -> Jet {
    let [a, b] = v;
    let n = a.f.hypot(b.f);
    if n == 0. {
        a.combine(b, n, 1., 0.)
    } else {
        a.combine(b, n, a.f / n, b.f / n)
    }
}
fn unit(v: &Point) -> Result<Point> {
    let n = norm(v);
    if n.f <= 1e-12 {
        return Err("A line has zero length.".into());
    }
    Ok([v[0].div(&n), v[1].div(&n)])
}
fn positive(value: f64, name: &str) -> Result<()> {
    if value.is_finite() && value > 0. {
        Ok(())
    } else {
        Err(format!("{name} is {value} mm. It must be above 0."))
    }
}
fn finite(v: f64) -> Result<()> {
    if v.is_finite() {
        Ok(())
    } else {
        Err("Sketch coordinates and dimensions must be finite.".into())
    }
}
pub(super) struct Row {
    pub value: Jet,
    pub constraint: Option<usize>,
}
pub(super) struct Problem<'a> {
    pub sketch: &'a Sketch,
    pub start: Vec<f64>,
    pub points: BTreeMap<&'a str, usize>,
    pub circles: BTreeMap<&'a str, usize>,
}
impl<'a> Problem<'a> {
    pub fn new(s: &'a Sketch) -> Result<Self> {
        if s.points.len() > 320 || s.entities.len() > 1000 || s.constraints.len() > 1000 {
            return Err(
                "The solver accepts up to 320 points, 1000 entities and 1000 constraints.".into(),
            );
        }
        let mut start = vec![];
        let mut points = BTreeMap::new();
        let mut circles = BTreeMap::new();
        for (id, xy) in &s.points {
            if id.trim().is_empty() {
                return Err("A point needs a name.".into());
            }
            finite(xy[0])?;
            finite(xy[1])?;
            points.insert(id.as_str(), start.len());
            start.extend(xy);
        }
        let point = |id: &str| {
            s.points
                .get(id)
                .ok_or_else(|| format!("Unknown sketch point: {id}."))
        };
        let separate = |a: &str, b: &str| -> Result<()> {
            let a = point(a)?;
            let b = point(b)?;
            if (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-12 {
                return Err("An entity has zero length or radius.".into());
            }
            Ok(())
        };
        for (id, e) in &s.entities {
            if id.trim().is_empty() {
                return Err("An entity needs a name.".into());
            }
            match e {
                Entity::Line { a, b } => separate(a, b)?,
                Entity::Circle { centre, radius } => {
                    point(centre)?;
                    positive(*radius, &format!("Radius of circle {id}"))?;
                    circles.insert(id.as_str(), start.len());
                    start.push(*radius);
                }
                Entity::Arc { centre, start, end } => {
                    separate(centre, start)?;
                    separate(centre, end)?;
                    separate(start, end)?;
                }
            }
        }
        if start.len() > 640 {
            return Err("The solver accepts up to 640 scalar variables.".into());
        }
        let mut construction = BTreeSet::new();
        for id in &s.construction {
            if !s.entities.contains_key(id) {
                return Err(format!("Unknown construction entity: {id}."));
            }
            if !construction.insert(id) {
                return Err(format!("Duplicate construction entity: {id}."));
            }
        }
        Ok(Self {
            sketch: s,
            start,
            points,
            circles,
        })
    }
    pub fn index(&self, id: &str) -> Result<usize> {
        self.points
            .get(id)
            .copied()
            .ok_or_else(|| format!("Unknown sketch point: {id}."))
    }
    fn point(&self, id: &str, x: &[f64]) -> Result<Point> {
        let i = self.index(id)?;
        Ok([Jet::variable(x[i], i), Jet::variable(x[i + 1], i + 1)])
    }
    fn entity(&self, id: &str) -> Result<&Entity> {
        self.sketch
            .entities
            .get(id)
            .ok_or_else(|| format!("Unknown sketch entity: {id}."))
    }
    fn line(&self, id: &str, x: &[f64]) -> Result<(Point, Point)> {
        let Entity::Line { a, b } = self.entity(id)? else {
            return Err(format!("Entity {id} must be a line."));
        };
        let a = self.point(a, x)?;
        let diff = difference(&self.point(b, x)?, &a);
        Ok((a, diff))
    }
    fn round(&self, id: &str, x: &[f64]) -> Result<(Point, Jet)> {
        match self.entity(id)? {
            Entity::Circle { centre, .. } => {
                let i = self.circles[id];
                positive(x[i], &format!("Radius of circle {id}"))?;
                Ok((self.point(centre, x)?, Jet::variable(x[i], i)))
            }
            Entity::Arc { centre, start, .. } => {
                let p = self.point(centre, x)?;
                let r = norm(&difference(&self.point(start, x)?, &p));
                positive(r.f, &format!("Radius of arc {id}"))?;
                Ok((p, r))
            }
            _ => Err(format!("Entity {id} must be a circle or arc.")),
        }
    }
    pub fn radii(&self, x: &[f64]) -> Result<BTreeMap<String, f64>> {
        self.sketch
            .entities
            .iter()
            .filter(|(_, e)| !matches!(e, Entity::Line { .. }))
            .map(|(id, _)| Ok((id.clone(), self.round(id, x)?.1.f)))
            .collect()
    }
    pub fn validate_result(&self, x: &[f64]) -> Result<()> {
        for (id, e) in &self.sketch.entities {
            let (a, b) = match e {
                Entity::Line { a, b } => (a, b),
                Entity::Arc { start, end, .. } => (start, end),
                Entity::Circle { .. } => continue,
            };
            if norm(&difference(&self.point(a, x)?, &self.point(b, x)?)).f <= 1e-10 {
                return Err(format!("Entity {id} has zero length in the solved sketch."));
            }
        }
        Ok(())
    }
    pub fn rows(&self, x: &[f64], skip: Option<usize>) -> Result<Vec<Row>> {
        let mut rows = vec![];
        let groups = self.coincident_groups(skip);
        for e in self.sketch.entities.values() {
            if let Entity::Arc { centre, start, end } = e {
                let c = self.point(centre, x)?;
                let a = self.point(start, x)?;
                let b = self.point(end, x)?;
                rows.push(Row {
                    value: norm(&difference(&a, &c)).sub(&norm(&difference(&b, &c))),
                    constraint: None,
                });
            }
        }
        for (i, c) in self.sketch.constraints.iter().enumerate() {
            if Some(i) != skip {
                let values = self
                    .equations(c, x, &groups)
                    .map_err(|e| format!("Constraint {i}: {e}"))?;
                rows.extend(values.into_iter().map(|value| Row {
                    value,
                    constraint: Some(i),
                }));
            }
        }
        if rows
            .iter()
            .any(|r| !r.value.f.is_finite() || r.value.d.iter().any(|(_, v)| !v.is_finite()))
        {
            return Err("Sketch equations exceed the numeric range.".into());
        }
        Ok(rows)
    }
    fn coincident_groups(&self, skip: Option<usize>) -> Vec<usize> {
        let mut groups: Vec<_> = (0..self.points.len()).collect();
        fn root(groups: &[usize], mut i: usize) -> usize {
            while groups[i] != i {
                i = groups[i];
            }
            i
        }
        for (index, constraint) in self.sketch.constraints.iter().enumerate() {
            if Some(index) != skip
                && let Constraint::Coincident { a, b } = constraint
                && let (Some(&i), Some(&j)) =
                    (self.points.get(a.as_str()), self.points.get(b.as_str()))
            {
                let (u, v) = (root(&groups, i / 2), root(&groups, j / 2));
                groups[u.max(v)] = u.min(v);
            }
        }
        for i in 0..groups.len() {
            groups[i] = root(&groups, i);
        }
        groups
    }
    fn equations(&self, c: &Constraint, x: &[f64], groups: &[usize]) -> Result<Vec<Jet>> {
        use Constraint::*;
        let p = |id: &str| self.point(id, x);
        let l = |id: &str| self.line(id, x);
        let r = |id: &str| self.round(id, x);
        let offset = |pt: &Point, line: &str| -> Result<Jet> {
            let (a, v) = l(line)?;
            Ok(cross(&unit(&v)?, &difference(pt, &a)))
        };
        let rows = match c {
            Coincident { a, b } => difference(&p(a)?, &p(b)?).to_vec(),
            Fix { point } => {
                let i = self.index(point)?;
                let [a, b] = p(point)?;
                vec![a.minus(self.start[i]), b.minus(self.start[i + 1])]
            }
            PointOnLine { point, line } => vec![offset(&p(point)?, line)?],
            PointOnCircle { point, circle } => {
                let (centre, rad) = r(circle)?;
                let v = difference(&p(point)?, &centre);
                let length = norm(&v);
                let mut out = vec![length.sub(&rad)];
                if let Entity::Arc { start, end, .. } = self.entity(circle)? {
                    let mut trim = Jet {
                        f: 0.,
                        d: Vec::new(),
                    };
                    if length.f > 1e-12 {
                        let a = difference(&p(start)?, &centre);
                        let b = difference(&p(end)?, &centre);
                        let from = angle_error(&a, &v, 0.)?;
                        let sweep = angle_error(&a, &b, 0.)?.f.rem_euclid(std::f64::consts::TAU);
                        if from.f.abs() > 1e-13
                            && from.f.rem_euclid(std::f64::consts::TAU) > sweep + 1e-13
                        {
                            let to = angle_error(&b, &v, 0.)?;
                            trim = if from.f.abs() < to.f.abs() { from } else { to }.mul(&rad);
                        }
                    }
                    out.push(trim);
                }
                out
            }
            Midpoint { point, line } => {
                let (a, d) = l(line)?;
                let e = difference(&p(point)?, &a);
                vec![
                    e[0].sub(&d[0].clone().scale(0.5)),
                    e[1].sub(&d[1].clone().scale(0.5)),
                ]
            }
            Horizontal { line } => vec![l(line)?.1[1].clone()],
            Vertical { line } => vec![l(line)?.1[0].clone()],
            Parallel { a, b } | Perpendicular { a, b } | Collinear { a, b } => {
                let (pa, va) = l(a)?;
                let (pb, vb) = l(b)?;
                let u = unit(&va)?;
                let (sa, sb) = (self.line(a, &self.start)?.1, self.line(b, &self.start)?.1);
                let target = if matches!(c, Perpendicular { .. }) {
                    if cross(&sa, &sb).f < 0. {
                        -std::f64::consts::FRAC_PI_2
                    } else {
                        std::f64::consts::FRAC_PI_2
                    }
                } else if dot(&sa, &sb).f < 0. {
                    std::f64::consts::PI
                } else {
                    0.
                };
                let mut out =
                    vec![angle_error(&va, &vb, target)?.scale((norm(&sa).f + norm(&sb).f) * 0.5)];
                if matches!(c, Collinear { .. }) {
                    out.push(cross(&u, &difference(&pb, &pa)));
                }
                out
            }
            Angle { a, b, degrees } => {
                finite(*degrees)?;
                let v = l(a)?.1;
                let w = l(b)?.1;
                vec![
                    angle_error(&v, &w, degrees.to_radians())?
                        .mul(&norm(&v).add(&norm(&w)).scale(0.5)),
                ]
            }
            Tangent { a, b } => {
                let (a_line, b_line) = (
                    matches!(self.entity(a)?, Entity::Line { .. }),
                    matches!(self.entity(b)?, Entity::Line { .. }),
                );
                if a_line && b_line {
                    return Err("Tangency needs a circle or arc.".into());
                }
                if a_line || b_line {
                    let (line, round) = if a_line { (a, b) } else { (b, a) };
                    let (centre, rad) = r(round)?;
                    if let (Entity::Line { a, b }, Entity::Arc { start, end, .. }) =
                        (self.entity(line)?, self.entity(round)?)
                        && let Some(shared) = [start, end].into_iter().find(|e| {
                            let group = groups[self.points[e.as_str()] / 2];
                            [a, b]
                                .iter()
                                .any(|point| groups[self.points[point.as_str()] / 2] == group)
                        })
                    {
                        // At a shared end the tangent is perpendicular to its radius.
                        return Ok(vec![dot(
                            &unit(&l(line)?.1)?,
                            &difference(&p(shared)?, &centre),
                        )]);
                    }
                    let signed = offset(&centre, line)?;
                    let (origin, v) = self.line(line, &self.start)?;
                    let (base, _) = self.round(round, &self.start)?;
                    let side = cross(&unit(&v)?, &difference(&base, &origin)).f;
                    vec![signed.scale(if side < 0. { -1. } else { 1. }).sub(&rad)]
                } else {
                    let (p, ra) = r(a)?;
                    let (q, rb) = r(b)?;
                    let (p0, r0) = self.round(a, &self.start)?;
                    let (q0, s0) = self.round(b, &self.start)?;
                    if let (
                        Entity::Arc { start, end, .. },
                        Entity::Arc {
                            start: other_start,
                            end: other_end,
                            ..
                        },
                    ) = (self.entity(a)?, self.entity(b)?)
                        && let Some(id) = [start, end].into_iter().find(|id| {
                            let group = groups[self.points[id.as_str()] / 2];
                            [other_start, other_end]
                                .iter()
                                .any(|e| groups[self.points[e.as_str()] / 2] == group)
                        })
                    {
                        let contact = self.point(id, x)?;
                        let base = self.point(id, &self.start)?;
                        let angle = if dot(&difference(&base, &p0), &difference(&base, &q0)).f < 0.
                        {
                            std::f64::consts::PI
                        } else {
                            0.
                        };
                        return Ok(vec![
                            angle_error(
                                &difference(&contact, &p),
                                &difference(&contact, &q),
                                angle,
                            )?
                            .scale((r0.f + s0.f) * 0.5),
                        ]);
                    }
                    let d = norm(&difference(&p, &q));
                    let d0 = norm(&difference(&p0, &q0)).f;
                    let sum = (d0 - r0.f - s0.f).abs();
                    let diff = (d0 - (r0.f - s0.f).abs()).abs();
                    let expected = if sum <= diff {
                        ra.add(&rb)
                    } else if r0.f >= s0.f {
                        ra.sub(&rb)
                    } else {
                        rb.sub(&ra)
                    };
                    vec![d.sub(&expected)]
                }
            }
            Equal { a, b } => {
                if matches!(self.entity(a)?, Entity::Line { .. }) {
                    vec![norm(&l(a)?.1).sub(&norm(&l(b)?.1))]
                } else {
                    vec![r(a)?.1.sub(&r(b)?.1)]
                }
            }
            Concentric { a, b } => difference(&r(a)?.0, &r(b)?.0).to_vec(),
            Symmetric { a, b, about } => {
                let (origin, v) = l(about)?;
                let axis = unit(&v)?;
                let (p, q) = (p(a)?, p(b)?);
                let d = difference(&p, &origin);
                let perp = cross(&axis, &d);
                vec![
                    p[0].add(&axis[1].mul(&perp).scale(2.)).sub(&q[0]),
                    p[1].sub(&axis[0].mul(&perp).scale(2.)).sub(&q[1]),
                ]
            }
            Distance { a, b, value } => {
                positive(*value, "Distance")?;
                vec![norm(&difference(&p(b)?, &p(a)?)).minus(*value)]
            }
            DistanceX { a, b, value } | DistanceY { a, b, value } => {
                finite(*value)?;
                let j = usize::from(matches!(c, DistanceY { .. }));
                vec![p(b)?[j].sub(&p(a)?[j]).minus(*value)]
            }
            PointLineDistance { point, line, value } => {
                positive(*value, "Distance")?;
                let signed = offset(&p(point)?, line)?;
                let (origin, v) = self.line(line, &self.start)?;
                let initial = cross(
                    &unit(&v)?,
                    &difference(&self.point(point, &self.start)?, &origin),
                )
                .f;
                vec![
                    signed
                        .scale(if initial < 0. { -1. } else { 1. })
                        .minus(*value),
                ]
            }
            Radius { entity, value } | Diameter { entity, value } => {
                positive(*value, "Circular dimension")?;
                let k = if matches!(c, Diameter { .. }) { 2. } else { 1. };
                vec![r(entity)?.1.scale(k).minus(*value)]
            }
            Length { line, value } => {
                positive(*value, "Line length")?;
                vec![norm(&l(line)?.1).minus(*value)]
            }
        };
        Ok(rows)
    }
}
#[cfg(test)]
#[path = "residual_tests.rs"]
mod tests;
