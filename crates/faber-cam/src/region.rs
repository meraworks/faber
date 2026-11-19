use faber_ports::Mesh;
use i_overlay::{
    core::{fill_rule::FillRule, overlay_rule::OverlayRule},
    float::{simplify::SimplifyShape, single::SingleFloatOverlay},
    mesh::{
        outline::offset::OutlineOffset,
        style::{LineJoin, OutlineStyle},
    },
};
use serde::{Deserialize, Serialize};

pub type Ring = Vec<[f64; 2]>;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: Ring,
    pub holes: Vec<Ring>,
}

pub(crate) fn area(r: &[[f64; 2]]) -> f64 {
    let Some(p) = r.first().copied() else {
        return 0.;
    };
    r.windows(2)
        .map(|w| (w[0][0] - p[0]) * (w[1][1] - p[1]) - (w[0][1] - p[1]) * (w[1][0] - p[0]))
        .sum::<f64>()
        * 0.5
}
fn from_shapes(shapes: Vec<Vec<Ring>>) -> Vec<Region> {
    shapes
        .into_iter()
        .filter_map(|shape| {
            let mut it = shape.into_iter();
            Some(Region {
                outer: it.next()?,
                holes: it.collect(),
            })
        })
        .collect()
}
fn loops(regions: &[Region]) -> Vec<Ring> {
    regions
        .iter()
        .flat_map(|r| std::iter::once((&r.outer, true)).chain(r.holes.iter().map(|h| (h, false))))
        .map(|(ring, outer)| {
            let mut r = ring.clone();
            if (area(&r) > 0.) != outer {
                r.reverse();
            }
            r
        })
        .collect()
}
pub fn regions_at(mesh: &Mesh, z: f64) -> Vec<Region> {
    if !z.is_finite()
        || mesh
            .triangles
            .iter()
            .flatten()
            .any(|i| *i as usize >= mesh.vertices.len())
    {
        return vec![];
    }
    let rings: Vec<Ring> = faber_meshtools::slice(mesh, [0., 0., z], [0., 0., 1.])
        .into_iter()
        .map(|r| r.into_iter().map(|[x, y, _]| [x, y]).collect())
        .collect();
    from_shapes(rings.simplify_shape(FillRule::EvenOdd))
}
pub fn offset(regions: &[Region], distance: f64) -> Vec<Region> {
    if !distance.is_finite() || regions.is_empty() {
        return vec![];
    }
    if distance == 0. {
        return regions.to_vec();
    }
    let angle = 2. * (1. - 0.005 / distance.abs()).clamp(-1., 1.).acos();
    let style = OutlineStyle::new(distance)
        .line_join(LineJoin::Round(angle.min(std::f64::consts::FRAC_PI_2)));
    from_shapes(loops(regions).outline(&style))
}
pub(crate) fn boolean(left: &[Region], right: &[Region], rule: OverlayRule) -> Vec<Region> {
    from_shapes(loops(left).overlay(&loops(right), rule, FillRule::NonZero))
}
pub(crate) fn silhouette(mesh: &Mesh) -> Vec<Region> {
    let mut rings = vec![];
    for t in &mesh.triangles {
        let Some(points) = t
            .iter()
            .map(|&i| mesh.vertices.get(i as usize).map(|p| [p[0], p[1]]))
            .collect::<Option<Ring>>()
        else {
            return vec![];
        };
        let signed = area(&points);
        if signed > 0. {
            rings.push(points);
        } else if signed < 0. {
            rings.push(points.into_iter().rev().collect());
        }
    }
    from_shapes(rings.simplify_shape(FillRule::NonZero))
}

pub(crate) fn horizontal_floor(mesh: &Mesh, z: f64) -> Vec<Region> {
    let mut triangles = Vec::new();
    let tolerance = 1e-8 + z.abs() * f64::EPSILON * 16.;
    for tri in &mesh.triangles {
        let [Some(a), Some(b), Some(c)] = tri.map(|i| mesh.vertices.get(i as usize)) else {
            return vec![];
        };
        if [a, b, c].iter().all(|p| (p[2] - z).abs() <= tolerance) {
            let points = vec![[a[0], a[1]], [b[0], b[1]], [c[0], c[1]]];
            if area(&points) > 0. {
                triangles.push(points);
            }
        }
    }
    from_shapes(triangles.simplify_shape(FillRule::NonZero))
}
pub(crate) fn contains(ring: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    for (&a, &b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}
pub(crate) fn boundary_distance(p: [f64; 2], ring: &[[f64; 2]]) -> f64 {
    let p = glam::DVec2::from(p);
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(&a, &b)| {
            let a = glam::DVec2::from(a);
            let d = glam::DVec2::from(b) - a;
            let t = if d.length_squared() > 0. {
                (p - a).dot(d) / d.length_squared()
            } else {
                0.
            };
            p.distance(a + t.clamp(0., 1.) * d)
        })
        .fold(f64::INFINITY, f64::min)
}
