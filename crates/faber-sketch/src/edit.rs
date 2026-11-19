//! Edits to points, curves and constraints in the sketch plane.

use crate::{Constraint, Entity, Result, Sketch};
use faber_model::Scalar;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[path = "edit_intersections.rs"]
mod intersections;
pub(crate) use intersections::boundaries_cross;

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "op",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(deserialize = "N: Deserialize<'de>")
)]
pub enum SketchOp<N = f64> {
    AddPoint {
        id: String,
        at: [N; 2],
    },
    AddEntity {
        id: String,
        entity: Entity<N>,
        #[serde(default)]
        construction: bool,
    },
    AddConstraint {
        constraint: Constraint<N>,
    },
    Remove {
        id: String,
    },
    Fillet {
        corner: String,
        radius: N,
    },
    Chamfer {
        corner: String,
        distance: N,
    },
    Trim {
        entity: String,
        near: [N; 2],
    },
    Extend {
        line: String,
        end: String,
    },
    Mirror {
        entities: Vec<String>,
        about: String,
    },
    Pattern {
        entities: Vec<String>,
        count: u32,
        step: [N; 2],
    },
}

pub fn apply(sketch: &Sketch, ops: &[SketchOp]) -> Result<Sketch> {
    apply_with(sketch, ops, &mut |n| Ok(*n))
}

pub fn apply_parametric(
    sketch: &Sketch<Scalar>,
    ops: &[SketchOp<Scalar>],
    resolve: &mut impl FnMut(&Scalar) -> Result<f64>,
) -> Result<Sketch<Scalar>> {
    apply_with(sketch, ops, resolve)
}

fn point_refs<N>(e: &Entity<N>) -> Vec<&str> {
    match e {
        Entity::Line { a, b } => vec![a, b],
        Entity::Circle { centre, .. } => vec![centre],
        Entity::Arc { centre, start, end } => vec![centre, start, end],
    }
}

fn refs_mut<N>(c: &mut Constraint<N>) -> Vec<&mut String> {
    use Constraint::*;
    match c {
        Coincident { a, b }
        | Parallel { a, b }
        | Perpendicular { a, b }
        | Collinear { a, b }
        | Tangent { a, b }
        | Equal { a, b }
        | Concentric { a, b }
        | Distance { a, b, .. }
        | DistanceX { a, b, .. }
        | DistanceY { a, b, .. }
        | Angle { a, b, .. } => vec![a, b],
        Symmetric { a, b, about } => vec![a, b, about],
        Fix { point } => vec![point],
        PointOnLine { point, line }
        | Midpoint { point, line }
        | PointLineDistance { point, line, .. } => vec![point, line],
        PointOnCircle { point, circle } => vec![point, circle],
        Horizontal { line } | Vertical { line } | Length { line, .. } => vec![line],
        Radius { entity, .. } | Diameter { entity, .. } => vec![entity],
    }
}

fn available<N>(s: &Sketch<N>, id: &str) -> bool {
    !s.points.contains_key(id) && !s.entities.contains_key(id)
}

fn check_name<N>(s: &Sketch<N>, id: &str) -> Result<()> {
    if id.trim().is_empty() || !available(s, id) {
        return Err(format!("The sketch name '{id}' is empty or in use."));
    }
    Ok(())
}

fn fresh<N>(s: &Sketch<N>, base: &str) -> String {
    if available(s, base) {
        return base.into();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|id| available(s, id))
        .unwrap()
}

fn insert_point<N: From<f64>>(s: &mut Sketch<N>, base: &str, at: [f64; 2]) -> String {
    let id = fresh(s, base);
    s.points.insert(id.clone(), at.map(N::from));
    id
}

trait EditNumber: Clone + From<f64> {
    fn multiple(&self, count: u32) -> Self;
    fn anchor(&self, before: f64, solved: f64) -> Self;
}
impl EditNumber for f64 {
    fn anchor(&self, _before: f64, solved: f64) -> Self {
        solved
    }

    fn multiple(&self, count: u32) -> Self {
        *self * f64::from(count)
    }
}
impl EditNumber for Scalar {
    fn anchor(&self, before: f64, solved: f64) -> Self {
        match self {
            Self::Expression(_) if (solved - before).abs() <= 1e-9 => self.clone(),
            Self::Expression(expr) => Self::Expression(format!("({expr}) + ({})", solved - before)),
            Self::Number(_) => Self::Number(solved),
        }
    }

    fn multiple(&self, count: u32) -> Self {
        match self {
            Self::Number(n) => Self::Number(n.multiple(count)),
            Self::Expression(e) if count != 1 => Self::Expression(format!("({e}) * {count}")),
            _ => self.clone(),
        }
    }
}

fn apply_with<N: EditNumber>(
    sketch: &Sketch<N>,
    ops: &[SketchOp<N>],
    resolve: &mut impl FnMut(&N) -> Result<f64>,
) -> Result<Sketch<N>> {
    if ops.len() > 1024 {
        return Err("A sketch edit accepts at most 1024 operations.".into());
    }
    let mut s = sketch.clone();
    for op in ops {
        match op {
            SketchOp::AddPoint { id, at } => {
                check_name(&s, id)?;
                s.points.insert(id.clone(), at.clone());
            }
            SketchOp::AddEntity {
                id,
                entity,
                construction,
            } => {
                check_name(&s, id)?;
                s.entities.insert(id.clone(), entity.clone());
                if *construction {
                    s.construction.push(id.clone());
                }
            }
            SketchOp::AddConstraint { constraint } => {
                if let Constraint::Fix { point } = constraint {
                    let numeric = s.map_numbers(resolve)?;
                    let solved = crate::solve(&numeric)?;
                    if solved.status == crate::Status::Over {
                        return Err(
                            "Resolve the conflicting constraints before fixing a point.".into()
                        );
                    }
                    let target = solved
                        .points
                        .get(point)
                        .ok_or_else(|| format!("Unknown sketch point: {point}."))?;
                    let before = numeric.points[point];
                    let seed = s
                        .points
                        .get_mut(point)
                        .expect("The solved point has a start position");
                    for i in 0..2 {
                        seed[i] = seed[i].anchor(before[i], target[i]);
                    }
                }
                s.constraints.push(constraint.clone());
            }
            SketchOp::Remove { id } => {
                if let Some(index) = id
                    .strip_prefix("constraint:")
                    .and_then(|i| i.parse::<usize>().ok())
                {
                    if index >= s.constraints.len() {
                        return Err(format!("Unknown sketch constraint: {index}"));
                    }
                    s.constraints.remove(index);
                    continue;
                }
                if s.entities.remove(id).is_none() {
                    if s.entities
                        .values()
                        .any(|e| point_refs(e).contains(&id.as_str()))
                    {
                        return Err(format!("A sketch entity still uses point {id}."));
                    }
                    if s.points.remove(id).is_none() {
                        return Err(format!("Unknown sketch item: {id}"));
                    }
                }
                s.construction.retain(|key| key != id);
                s.constraints
                    .retain_mut(|c| !refs_mut(c).iter().any(|key| key.as_str() == id));
            }
            _ => {
                let numeric = s.map_numbers(resolve)?;
                let solution = crate::solve(&numeric)?;
                if solution.status == crate::Status::Over {
                    return Err(
                        "Resolve the conflicting constraints before editing the sketch geometry."
                            .into(),
                    );
                }
                let mut current = numeric;
                current.points = solution.points;
                for (id, r) in solution.radii {
                    if let Some(Entity::Circle { radius, .. }) = current.entities.get_mut(&id) {
                        *radius = r;
                    }
                }
                match op {
                    SketchOp::Fillet { corner, radius } => {
                        cut_corner(&mut s, &current, corner, radius, resolve(radius)?, true)?
                    }
                    SketchOp::Chamfer { corner, distance } => cut_corner(
                        &mut s,
                        &current,
                        corner,
                        distance,
                        resolve(distance)?,
                        false,
                    )?,
                    SketchOp::Mirror { entities, about } => {
                        copies(&mut s, &current, entities, CopyKind::Mirror(about))?
                    }
                    SketchOp::Pattern {
                        entities,
                        count,
                        step,
                    } => copies(
                        &mut s,
                        &current,
                        entities,
                        CopyKind::Pattern(*count, [resolve(&step[0])?, resolve(&step[1])?], step),
                    )?,
                    SketchOp::Trim { entity, near } => intersections::trim(
                        &mut s,
                        &current,
                        entity,
                        [resolve(&near[0])?, resolve(&near[1])?],
                    )?,
                    SketchOp::Extend { line, end } => {
                        intersections::extend(&mut s, &current, line, end)?
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
    crate::solve(&s.map_numbers(resolve)?)?;
    Ok(s)
}

type P = [f64; 2];
fn add(a: P, b: P) -> P {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: P, b: P) -> P {
    [a[0] - b[0], a[1] - b[1]]
}
fn mul(a: P, k: f64) -> P {
    [a[0] * k, a[1] * k]
}
fn dot(a: P, b: P) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: P, b: P) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn length(a: P) -> f64 {
    a[0].hypot(a[1])
}
fn unit(a: P) -> Result<P> {
    let d = length(a);
    if d <= 1e-12 || !d.is_finite() {
        Err("The sketch direction has zero or invalid length.".into())
    } else {
        Ok(mul(a, 1. / d))
    }
}

fn cut_corner<N: Clone + From<f64>>(
    s: &mut Sketch<N>,
    geo: &Sketch,
    corner: &str,
    param: &N,
    value: f64,
    round: bool,
) -> Result<()> {
    if !value.is_finite() || value <= 0. {
        return Err("The corner size must be finite and positive.".into());
    }
    let origin = *geo
        .points
        .get(corner)
        .ok_or_else(|| format!("Unknown corner point: {corner}"))?;
    let mut joint = BTreeSet::from([corner]);
    loop {
        let count = joint.len();
        for c in &geo.constraints {
            if let Constraint::Coincident { a, b } = c
                && (joint.contains(a.as_str()) || joint.contains(b.as_str()))
            {
                joint.extend([a.as_str(), b.as_str()]);
            }
        }
        if joint.len() == count {
            break;
        }
    }
    let mut sides = Vec::new();
    for (id, e) in &geo.entities {
        if geo.construction.contains(id) {
            continue;
        }
        if let Entity::Line { a, b } = e {
            if joint.contains(a.as_str()) || joint.contains(b.as_str()) {
                let other = if joint.contains(a.as_str()) { b } else { a };
                sides.push((id.clone(), sub(geo.points[other], origin)));
            }
        } else if let Entity::Arc { start, end, .. } = e
            && (joint.contains(start.as_str()) || joint.contains(end.as_str()))
        {
            return Err("Choose a corner of two straight lines.".into());
        }
    }
    let [(id_a, delta_a), (id_b, delta_b)]: [(String, P); 2] = sides
        .try_into()
        .map_err(|_| "Choose a corner of two straight lines.")?;
    let (u, v) = (unit(delta_a)?, unit(delta_b)?);
    let half = cross(u, v).abs().atan2(dot(u, v)) * 0.5;
    if half.sin() < 1e-9 || half.cos() < 1e-9 {
        return Err("The corner lines must meet at an angle.".into());
    }
    let along = if round { value / half.tan() } else { value };
    if along >= length(delta_a) - 1e-9 || along >= length(delta_b) - 1e-9 {
        return Err("The corner size exceeds a line length.".into());
    }
    let positions = [add(origin, mul(u, along)), add(origin, mul(v, along))];
    let mut ends = Vec::new();
    for (id, position) in [id_a.clone(), id_b.clone()].into_iter().zip(positions) {
        let guide = fresh(s, &format!("{id}_guide"));
        s.entities.insert(guide.clone(), s.entities[&id].clone());
        s.construction.push(guide.clone());
        for c in &mut s.constraints {
            for target in refs_mut(c) {
                if *target == id {
                    target.clone_from(&guide);
                }
            }
        }
        let tip = insert_point(s, &format!("{corner}_end"), position);
        let Some(Entity::Line { a, b }) = s.entities.get_mut(&id) else {
            unreachable!()
        };
        if joint.contains(a.as_str()) {
            a.clone_from(&tip);
        } else {
            b.clone_from(&tip);
        }
        s.constraints.push(Constraint::PointOnLine {
            point: tip.clone(),
            line: guide,
        });
        if !round {
            s.constraints.push(Constraint::Distance {
                a: corner.into(),
                b: tip.clone(),
                value: param.clone(),
            });
        }
        ends.push(tip);
    }
    let mut ends = ends.into_iter();
    let (mut a, mut b) = (ends.next().unwrap(), ends.next().unwrap());
    if !round {
        let id = fresh(s, &format!("{corner}_chamfer"));
        s.entities.insert(id, Entity::Line { a, b });
        return Ok(());
    }
    let centre = add(origin, mul(unit(add(u, v))?, value / half.sin()));
    if cross(sub(positions[0], centre), sub(positions[1], centre)) < 0. {
        std::mem::swap(&mut a, &mut b);
    }
    let centre = insert_point(s, &format!("{corner}_centre"), centre);
    let id = fresh(s, &format!("{corner}_fillet"));
    s.entities.insert(
        id.clone(),
        Entity::Arc {
            centre,
            start: a,
            end: b,
        },
    );
    for line in [id_a, id_b] {
        s.constraints.push(Constraint::Tangent {
            a: line,
            b: id.clone(),
        });
    }
    s.constraints.push(Constraint::Radius {
        entity: id,
        value: param.clone(),
    });
    Ok(())
}

enum CopyKind<'a, N> {
    Mirror(&'a str),
    Pattern(u32, P, &'a [N; 2]),
}

fn copies<N: EditNumber>(
    s: &mut Sketch<N>,
    geo: &Sketch,
    entities: &[String],
    kind: CopyKind<'_, N>,
) -> Result<()> {
    let mut ids = BTreeSet::new();
    let mut points = BTreeSet::new();
    for id in entities {
        if !ids.insert(id) {
            return Err(format!("Duplicate sketch entity: {id}"));
        }
        points.extend(point_refs(
            geo.entities
                .get(id)
                .ok_or_else(|| format!("Unknown sketch entity: {id}"))?,
        ));
    }
    if ids.is_empty() {
        return Err("Choose at least one sketch entity.".into());
    }
    let (count, axis) = match kind {
        CopyKind::Mirror(id) => {
            let Some(Entity::Line { a, b }) = geo.entities.get(id) else {
                return Err("Use a straight line for the mirror axis.".into());
            };
            (
                2,
                Some((geo.points[a], unit(sub(geo.points[b], geo.points[a]))?)),
            )
        }
        CopyKind::Pattern(count, step, _) => {
            if !(1..=1000).contains(&count)
                || !step.into_iter().all(f64::is_finite)
                || (count > 1 && length(step) <= 1e-12)
            {
                return Err(
                    "Use 1 to 1000 pattern members. Copies need a finite non-zero step.".into(),
                );
            }
            (count, None)
        }
    };
    if (points.len() + ids.len()).saturating_mul(count as usize - 1) > 10000 {
        return Err("The sketch pattern exceeds 10000 new items.".into());
    }
    for n in 1..count {
        let mut new_points = BTreeMap::new();
        for &p in &points {
            let at = geo.points[p];
            let target = match kind {
                CopyKind::Mirror(_) => {
                    let (origin, d) = axis.unwrap();
                    sub(mul(add(origin, mul(d, dot(sub(at, origin), d))), 2.), at)
                }
                CopyKind::Pattern(_, step, _) => add(at, mul(step, f64::from(n))),
            };
            let clone = insert_point(s, &format!("{p}_copy{n}"), target);
            match kind {
                CopyKind::Mirror(about) => s.constraints.push(Constraint::Symmetric {
                    a: p.into(),
                    b: clone.clone(),
                    about: about.into(),
                }),
                CopyKind::Pattern(_, _, step) => {
                    s.constraints.push(Constraint::DistanceX {
                        a: p.into(),
                        b: clone.clone(),
                        value: step[0].multiple(n),
                    });
                    s.constraints.push(Constraint::DistanceY {
                        a: p.into(),
                        b: clone.clone(),
                        value: step[1].multiple(n),
                    });
                }
            }
            new_points.insert(p, clone);
        }
        for &id in &ids {
            let new_id = fresh(s, &format!("{id}_copy{n}"));
            let get = |p: &String| new_points[p.as_str()].clone();
            let item = match &s.entities[id] {
                Entity::Line { a, b } => Entity::Line {
                    a: get(a),
                    b: get(b),
                },
                Entity::Arc { centre, start, end } => {
                    let (a, b) = if axis.is_some() {
                        (end, start)
                    } else {
                        (start, end)
                    };
                    Entity::Arc {
                        centre: get(centre),
                        start: get(a),
                        end: get(b),
                    }
                }
                Entity::Circle { centre, radius } => {
                    s.constraints.push(Constraint::Equal {
                        a: id.clone(),
                        b: new_id.clone(),
                    });
                    Entity::Circle {
                        centre: get(centre),
                        radius: radius.clone(),
                    }
                }
            };
            if s.construction.contains(id) {
                s.construction.push(new_id.clone());
            }
            s.entities.insert(new_id, item);
        }
    }
    Ok(())
}
