//! Check material overlap between wall prisms and bend sectors.
use crate::{Bend, Wall};
use glam::{DQuat, DVec3};
const EPS: f64 = 1e-7;

#[derive(Clone, Copy)]
struct Bounds {
    low: DVec3,
    high: DVec3,
}
impl Bounds {
    fn empty() -> Self {
        Self {
            low: DVec3::splat(f64::INFINITY),
            high: DVec3::splat(f64::NEG_INFINITY),
        }
    }
    fn add(&mut self, p: DVec3) {
        self.low = self.low.min(p);
        self.high = self.high.max(p);
    }
    fn overlaps(self, b: Self) -> bool {
        (self.high.min(b.high) - self.low.max(b.low)).min_element() > EPS
    }
}

// A convex prism with a triangular or quadrilateral base.
struct Prism {
    points: Vec<DVec3>,
    edges: Vec<DVec3>,
    normals: Vec<DVec3>,
    bounds: Bounds,
}
impl Prism {
    fn new(base: &[DVec3], axis: DVec3) -> Self {
        let mut points = base.to_vec();
        points.extend(base.iter().map(|p| *p + axis));
        let mut bounds = Bounds::empty();
        for &p in &points {
            bounds.add(p);
        }
        let mut edges: Vec<_> = base
            .iter()
            .zip(base.iter().cycle().skip(1))
            .map(|(a, b)| (*b - *a).normalize_or_zero())
            .collect();
        let up = axis.normalize_or_zero();
        let mut normals: Vec<_> = edges
            .iter()
            .map(|v| v.cross(up).normalize_or_zero())
            .collect();
        edges.push(up);
        normals.push(up);
        Self {
            points,
            edges,
            normals,
            bounds,
        }
    }
    fn penetrates(&self, b: &Self) -> bool {
        let apart = |axis: DVec3| {
            let n = axis.length();
            if n < 1e-10 {
                return false;
            }
            let axis = axis / n;
            let origin = self.points[0];
            let project = |points: &[DVec3]| {
                points
                    .iter()
                    .map(|v| axis.dot(*v - origin))
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| {
                        (lo.min(x), hi.max(x))
                    })
            };
            let (lo, hi) = project(&self.points);
            let (low, high) = project(&b.points);
            hi.min(high) - lo.max(low) <= EPS
        };
        !self.normals.iter().chain(&b.normals).copied().any(apart)
            && !self
                .edges
                .iter()
                .any(|u| b.edges.iter().any(|v| apart(u.cross(*v))))
    }
}
struct Solid<'a> {
    id: &'a str,
    prisms: Vec<Prism>,
    bounds: Bounds,
}
impl<'a> Solid<'a> {
    fn new(id: &'a str, mut prisms: Vec<Prism>) -> Self {
        let mut bounds = Bounds::empty();
        for p in &prisms {
            bounds.add(p.bounds.low);
            bounds.add(p.bounds.high);
        }
        prisms.sort_unstable_by(|a, b| a.bounds.low.x.total_cmp(&b.bounds.low.x));
        Self { id, prisms, bounds }
    }
}
fn wall_solid(w: &Wall, t: f64) -> Result<Solid<'_>, String> {
    let mut points = w.outline.clone();
    let mut holes = Vec::new();
    for h in &w.holes {
        holes.push(points.len());
        points.extend_from_slice(h);
    }
    let indices = earcutr::earcut(points.as_flattened(), &holes, 2)
        .map_err(|_| format!("Cannot triangulate wall {}.", w.id))?;
    let axis = w.frame.transform_vector3(DVec3::Z) * t;
    let world: Vec<_> = points
        .iter()
        .map(|p| w.frame.transform_point3(DVec3::new(p[0], p[1], 0.)))
        .collect();
    let prisms = indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|tri| Prism::new(&[world[tri[0]], world[tri[1]], world[tri[2]]], axis))
        .collect();
    Ok(Solid::new(&w.id, prisms))
}
fn sectors(b: &Bend) -> usize {
    let angle = b.angle_deg.to_radians();
    let origin = DVec3::from(b.line[0]);
    let r = (DVec3::from(b.section[0]) - origin)
        .length()
        .max((DVec3::from(b.section[3]) - origin).length());
    let step = (2. * (1. - (0.002 / r).min(0.5)).acos()).min(std::f64::consts::PI / 24.);
    (angle.abs() / step).ceil().max(1.) as usize
}
fn bend_solid(b: &Bend) -> Solid<'_> {
    let origin = DVec3::from(b.line[0]);
    let axis = DVec3::from(b.line[1]) - origin;
    let bottom = DVec3::from(b.section[0]) - origin;
    let top = DVec3::from(b.section[3]) - origin;
    let angle = b.angle_deg.to_radians();
    let count = sectors(b);
    let mut low = origin + bottom;
    let mut high = origin + top;
    let mut prisms = Vec::with_capacity(count);
    for i in 1..=count {
        let rotate = DQuat::from_axis_angle(axis.normalize(), angle * i as f64 / count as f64);
        let next_low = origin + rotate * bottom;
        let next_high = origin + rotate * top;
        prisms.push(Prism::new(&[low, next_low, next_high, high], axis));
        low = next_low;
        high = next_high;
    }
    Solid::new(&b.flange, prisms)
}
pub(crate) fn check(walls: &[Wall], bends: &[Bend], t: f64) -> Result<(), String> {
    if bends.is_empty() {
        return Ok(());
    }
    let wedge_count = walls
        .iter()
        .map(|w| w.outline.len() + w.holes.iter().map(|h| h.len() + 2).sum::<usize>())
        .sum::<usize>()
        + bends.iter().map(sectors).sum::<usize>();
    if wedge_count > 180_000 {
        return Err(format!(
            "The folded sheet needs {wedge_count} collision cells. The limit is 180000. Simplify the outline."
        ));
    }
    let mut solids = walls
        .iter()
        .map(|w| wall_solid(w, t))
        .collect::<Result<Vec<_>, _>>()?;
    solids.extend(bends.iter().filter(|b| b.angle_deg != 0.).map(bend_solid));
    let mut comparisons = 0usize;
    for (i, a) in solids.iter().enumerate() {
        for b in &solids[..i] {
            if a.id == b.id || !a.bounds.overlaps(b.bounds) {
                continue;
            }
            for p in &a.prisms {
                let end = b
                    .prisms
                    .partition_point(|q| q.bounds.low.x < p.bounds.high.x - EPS);
                for q in &b.prisms[..end] {
                    comparisons += 1;
                    if comparisons > 20_000_000 {
                        return Err("Too many sheet faces to check for overlap. Simplify the sheet outline.".into());
                    }
                    if p.bounds.overlaps(q.bounds) && p.penetrates(q) {
                        return Err(format!(
                            "Wall or bend {} overlaps {} in the folded sheet.",
                            b.id, a.id
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
