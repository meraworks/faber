use crate::{Entity, Sketch, Solution};
use faber_model::{SketchLoop, SketchProfile, SketchSegment};
use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

type Point = [f64; 2];

#[derive(Clone, Debug)]
pub enum Segment {
    Line {
        start: Point,
        end: Point,
    },
    Arc {
        centre: Point,
        radius: f64,
        angle: f64,
        sweep: f64,
    },
    Circle {
        centre: Point,
        radius: f64,
    },
}

fn radial(c: Point, r: f64, a: f64) -> Point {
    [c[0] + r * a.cos(), c[1] + r * a.sin()]
}

fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

impl Segment {
    fn start(&self) -> Point {
        match *self {
            Self::Line { start, .. } => start,
            Self::Arc {
                centre,
                radius,
                angle,
                ..
            } => radial(centre, radius, angle),
            Self::Circle { centre, radius } => radial(centre, radius, 0.),
        }
    }

    fn reversed(&self) -> Self {
        match *self {
            Self::Line { start, end } => Self::Line {
                start: end,
                end: start,
            },
            Self::Arc {
                centre,
                radius,
                angle,
                sweep,
            } => Self::Arc {
                centre,
                radius,
                angle: angle + sweep,
                sweep: -sweep,
            },
            Self::Circle { .. } => self.clone(),
        }
    }

    fn area(&self, origin: Point) -> f64 {
        let relative = |p: Point| [p[0] - origin[0], p[1] - origin[1]];
        match *self {
            Self::Line { start, end } => cross(relative(start), relative(end)) / 2.,
            Self::Arc {
                centre,
                radius,
                angle,
                sweep,
            } => {
                let a = radial([0.; 2], radius, angle);
                let b = radial([0.; 2], radius, angle + sweep);
                (cross(relative(centre), [b[0] - a[0], b[1] - a[1]]) + radius * radius * sweep) / 2.
            }
            Self::Circle { radius, .. } => PI * radius * radius,
        }
    }

    fn crossings(&self, p: Point) -> usize {
        match *self {
            Self::Circle { centre, radius } => {
                usize::from((p[0] - centre[0]).hypot(p[1] - centre[1]) < radius)
            }
            Self::Line { start: a, end: b } => usize::from(
                (a[1] > p[1]) != (b[1] > p[1])
                    && (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0] > p[0],
            ),
            Self::Arc {
                centre,
                radius,
                angle,
                sweep,
            } => {
                // At a horizontal tangent two adjacent intervals give even parity.
                let lo = angle.min(angle + sweep);
                let hi = angle.max(angle + sweep);
                let mut stops = vec![lo];
                for i in
                    ((lo - FRAC_PI_2) / PI).ceil() as i32..=((hi - FRAC_PI_2) / PI).floor() as i32
                {
                    let t = FRAC_PI_2 + f64::from(i) * PI;
                    if lo < t && t < hi {
                        stops.push(t);
                    }
                }
                stops.push(hi);
                let y = (p[1] - centre[1]) / radius;
                stops
                    .windows(2)
                    .filter(|t| {
                        if (t[0].sin() > y) == (t[1].sin() > y) {
                            return false;
                        }
                        let sign = ((t[0] + t[1]) * 0.5).cos().signum();
                        centre[0] + radius * sign * (1. - y * y).max(0.).sqrt() > p[0]
                    })
                    .count()
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct Loop {
    pub segments: Vec<Segment>,
    pub area: f64,
}

impl Loop {
    fn outline(&self) -> SketchLoop {
        if let [Segment::Circle { centre, radius }] = self.segments.as_slice() {
            return SketchLoop::Circle {
                centre: *centre,
                radius: *radius,
            };
        }
        let segments = self
            .segments
            .iter()
            .map(|segment| match *segment {
                Segment::Line { end, .. } => SketchSegment::Line { end },
                Segment::Arc {
                    centre,
                    radius,
                    angle,
                    sweep,
                } => SketchSegment::Arc {
                    via: radial(centre, radius, angle + sweep / 2.),
                    end: radial(centre, radius, angle + sweep),
                },
                Segment::Circle { .. } => unreachable!("A full circle forms its own loop"),
            })
            .collect();
        SketchLoop::Path {
            start: self.segments[0].start(),
            segments,
        }
    }

    fn contains(&self, p: Point) -> bool {
        self.segments.iter().map(|s| s.crossings(p)).sum::<usize>() % 2 == 1
    }
}

/// Join solved endpoints. Keep circular edges exact in the resulting profile.
pub fn loops(sketch: &Sketch, solution: &Solution) -> Result<Vec<Loop>, String> {
    match contours(sketch, solution)? {
        Contours::Closed(loops) => Ok(loops),
        Contours::Open(reason) => Err(reason),
    }
}

pub enum Contours {
    Closed(Vec<Loop>),
    Open(String),
}

/// An open sketch can be edited until a feature requires a closed region.
pub fn contours(sketch: &Sketch, solution: &Solution) -> Result<Contours, String> {
    let point = |name: &str| {
        solution
            .points
            .get(name)
            .copied()
            .ok_or_else(|| format!("Unknown sketch point {name}."))
    };
    let mut nodes: Vec<(String, Point)> = Vec::new();
    let mut locate = |name: &str| -> Result<usize, String> {
        let p = point(name)?;
        if let Some(i) = nodes
            .iter()
            .position(|(_, q)| (p[0] - q[0]).hypot(p[1] - q[1]) < 1e-7)
        {
            return Ok(i);
        }
        nodes.push((name.to_owned(), p));
        Ok(nodes.len() - 1)
    };
    let mut found = Vec::new();
    let mut edges = Vec::new();
    let mut adjacent = BTreeMap::<usize, Vec<usize>>::new();
    for (name, entity) in &sketch.entities {
        if sketch.construction.contains(name) {
            continue;
        }
        let (a, b, segment) = match entity {
            Entity::Circle { centre, .. } => {
                let radius = *solution
                    .radii
                    .get(name)
                    .ok_or_else(|| format!("Missing radius for {name}."))?;
                found.push(Loop {
                    segments: vec![Segment::Circle {
                        centre: point(centre)?,
                        radius,
                    }],
                    area: PI * radius * radius,
                });
                continue;
            }
            Entity::Line { a, b } => (
                a,
                b,
                Segment::Line {
                    start: point(a)?,
                    end: point(b)?,
                },
            ),
            Entity::Arc { centre, start, end } => {
                let c = point(centre)?;
                let a = point(start)?;
                let b = point(end)?;
                let angle = (a[1] - c[1]).atan2(a[0] - c[0]);
                let sweep = ((b[1] - c[1]).atan2(b[0] - c[0]) - angle).rem_euclid(TAU);
                (
                    start,
                    end,
                    Segment::Arc {
                        centre: c,
                        radius: (a[0] - c[0]).hypot(a[1] - c[1]),
                        angle,
                        sweep,
                    },
                )
            }
        };
        let a = locate(a)?;
        let b = locate(b)?;
        if a == b {
            return Err(format!("The ends of {name} coincide."));
        }
        adjacent.entry(a).or_default().push(edges.len());
        adjacent.entry(b).or_default().push(edges.len());
        edges.push((name, a, b, segment));
    }
    let mut used = BTreeSet::new();
    let mut open = None;
    for seed in 0..edges.len() {
        if used.contains(&seed) {
            continue;
        }
        let start = edges[seed].1;
        let mut stack = vec![start];
        let mut component = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut ends = Vec::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node) {
                continue;
            }
            let incident = &adjacent[&node];
            if incident.len() > 2 {
                return Err(format!(
                    "More than two profile edges meet at {}.",
                    nodes[node].0
                ));
            }
            if incident.len() == 1 {
                ends.push(nodes[node].0.as_str())
            }
            for &index in incident {
                let (_, a, b, _) = edges[index];
                component.insert(index);
                stack.push(if a == node { b } else { a });
            }
        }
        if !ends.is_empty() {
            ends.sort_unstable();
            let names = component
                .iter()
                .map(|&i| edges[i].0.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            open.get_or_insert_with(|| {
                format!(
                    "The entities {names} do not close a loop. The open ends are at {}.",
                    ends.join(" and ")
                )
            });
            used.extend(component);
            continue;
        }
        let mut at = start;
        let mut segments = Vec::new();
        while let Some(&i) = adjacent[&at].iter().find(|i| !used.contains(*i)) {
            used.insert(i);
            let (_, a, b, ref edge) = edges[i];
            segments.push(if at == a {
                edge.clone()
            } else {
                edge.reversed()
            });
            at = if at == a { b } else { a };
        }
        let area = segments.iter().map(|s| s.area(nodes[start].1)).sum::<f64>();
        if area.abs() < 1e-12 {
            return Err("A profile loop has zero area.".into());
        }
        if area < 0. {
            segments = segments.iter().rev().map(Segment::reversed).collect()
        }
        found.push(Loop {
            segments,
            area: area.abs(),
        });
    }
    check_boundaries(&found)?;
    if let Some(reason) = open {
        return Ok(Contours::Open(reason));
    }
    if found.is_empty() {
        return Ok(Contours::Open("The sketch has no closed profile.".into()));
    }
    Ok(Contours::Closed(found))
}

fn check_boundaries(loops: &[Loop]) -> Result<(), String> {
    let mut edges = Vec::new();
    for (owner, contour) in loops.iter().enumerate() {
        for (index, segment) in contour.segments.iter().enumerate() {
            let (lo, hi) = match *segment {
                Segment::Line { start, end } => (start[0].min(end[0]), start[0].max(end[0])),
                Segment::Arc { centre, radius, .. } | Segment::Circle { centre, radius } => {
                    (centre[0] - radius, centre[0] + radius)
                }
            };
            edges.push((lo, hi, owner, index, segment));
        }
    }
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (i, &(_, hi, owner, index, a)) in edges.iter().enumerate() {
        for &(lo, _, other, j, b) in &edges[i + 1..] {
            if lo > hi + 1e-7 {
                break;
            }
            let gap = index.abs_diff(j);
            let joined = owner == other && (gap == 1 || gap + 1 == loops[owner].segments.len());
            if crate::edit::boundaries_cross(a, b, joined) {
                return Err(
                    "Sketch boundaries cross, touch or overlap. Separate the loops.".into(),
                );
            }
        }
    }
    Ok(())
}

/// A loop inside a hole starts a separate region.
pub fn regions(loops: &[Loop]) -> Vec<SketchProfile> {
    let parents: Vec<_> = loops
        .iter()
        .map(|inner| {
            loops
                .iter()
                .enumerate()
                .filter(|(_, outer)| {
                    outer.area > inner.area && outer.contains(inner.segments[0].start())
                })
                .min_by(|(_, a), (_, b)| a.area.total_cmp(&b.area))
                .map(|(i, _)| i)
        })
        .collect();
    let mut result = Vec::new();
    for (i, outer) in loops.iter().enumerate() {
        let depth = std::iter::successors(parents[i], |&p| parents[p]).count();
        if depth % 2 == 0 {
            let holes = loops
                .iter()
                .enumerate()
                .filter(|(j, _)| parents[*j] == Some(i))
                .map(|(_, l)| l.outline())
                .collect();
            result.push(SketchProfile {
                outer: outer.outline(),
                holes,
            });
        }
    }
    result
}
