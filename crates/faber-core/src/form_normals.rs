//! Carry subdivision normals to the display triangles of a faceted solid.
use crate::{history, placement::rotation_matrix};
use faber_form::{Cage, subdivide};
use faber_geometry::bvh::Bvh;
use faber_model::{CageData, Geometry, ScaleFactor};
use faber_ports::{FaceMesh, FaceOrigins, Mesh, Result};
use glam::{DAffine3, DVec3};
use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(super) fn has_form(g: &Geometry) -> bool {
    match g {
        Geometry::Form { as_solid: true, .. } => true,
        Geometry::History { features, .. } => features
            .iter()
            .any(|f| matches!(f.geometry, Geometry::Form { as_solid: true, .. }) && !f.suppressed),
        _ => false,
    }
}
struct Source<'a> {
    id: Option<&'a str>,
    cage: &'a CageData,
    levels: u32,
    frame: DAffine3,
}
fn source<'a>(id: Option<&'a str>, g: &'a Geometry, frame: DAffine3) -> Option<Source<'a>> {
    if let Geometry::Form {
        cage,
        levels,
        as_solid: true,
        ..
    } = g
    {
        Some(Source {
            id,
            cage,
            levels: levels.unwrap_or(3),
            frame,
        })
    } else {
        None
    }
}
fn sources(g: &Geometry) -> Vec<Source<'_>> {
    let Geometry::History { features, output } = g else {
        return source(None, g, DAffine3::IDENTITY).into_iter().collect();
    };
    let by_id: BTreeMap<_, _> = features.iter().map(|f| (f.id.as_str(), f)).collect();
    let mut next = vec![(output.as_str(), DAffine3::IDENTITY)];
    let mut seen = BTreeSet::new();
    let mut result = vec![];
    while let Some((id, frame)) = next.pop() {
        if !seen.insert((id, frame.to_cols_array().map(f64::to_bits))) {
            continue;
        }
        let Some(f) = by_id.get(id) else { continue };
        if !f.suppressed {
            if let Some(s) = source(Some(id), &f.geometry, frame) {
                result.push(s);
                continue;
            }
            match &f.geometry {
                Geometry::Union { left, right }
                | Geometry::Difference { left, right }
                | Geometry::Intersection { left, right } => {
                    next.extend([(left.as_str(), frame), (right.as_str(), frame)]);
                    continue;
                }
                Geometry::Transform { source, placement } => {
                    next.push((
                        source,
                        frame
                            * DAffine3::from_mat3_translation(
                                rotation_matrix(placement.rotation),
                                placement.translation.into(),
                            ),
                    ));
                    continue;
                }
                Geometry::Scale {
                    source,
                    factor,
                    about,
                } => {
                    let scale = match factor {
                        ScaleFactor::Uniform(x) => DVec3::splat(*x),
                        ScaleFactor::Axes(v) => (*v).into(),
                    };
                    let centre = DVec3::from(*about);
                    next.push((
                        source,
                        frame
                            * DAffine3::from_translation(centre - scale * centre)
                            * DAffine3::from_scale(scale),
                    ));
                    continue;
                }
                _ => {}
            }
        }
        if let Some(input) = history::primary_input(&f.geometry) {
            next.push((input, frame));
        }
    }
    result
}

struct Samples {
    mesh: Mesh,
    bvh: OnceCell<Result<Bvh>>,
    by_corners: HashMap<[[i64; 3]; 3], u32>,
}
fn corner_key(points: [DVec3; 3]) -> [[i64; 3]; 3] {
    let mut key = points.map(|p| p.to_array().map(|v| (v * 1e7).round() as i64));
    key.sort_unstable();
    key
}

impl Samples {
    fn new(s: &Source<'_>) -> Result<Self> {
        let mut mesh = subdivide(&Cage::from_data(s.cage)?, s.levels)?;
        if s.frame != DAffine3::IDENTITY {
            let normals = s.frame.matrix3.inverse().transpose();
            for p in &mut mesh.vertices {
                *p = s.frame.transform_point3((*p).into()).to_array();
            }
            for n in &mut mesh.normals {
                *n = (normals * DVec3::from(*n)).normalize_or_zero().to_array();
            }
            if s.frame.matrix3.determinant() < 0. {
                for t in &mut mesh.triangles {
                    t.swap(1, 2)
                }
            }
        }
        let by_corners = mesh
            .triangles
            .iter()
            .enumerate()
            .map(|(i, tri)| {
                (
                    corner_key(tri.map(|j| DVec3::from(mesh.vertices[j as usize]))),
                    i as u32,
                )
            })
            .collect();
        Ok(Self {
            mesh,
            bvh: OnceCell::new(),
            by_corners,
        })
    }
    fn at(&self, tri: u32, p: DVec3, face_normal: DVec3) -> Option<DVec3> {
        let ids = *self.mesh.triangles.get(tri as usize)?;
        let [a, b, c] = ids.map(|i| DVec3::from(self.mesh.vertices[i as usize]));
        let ab = b - a;
        let ac = c - a;
        let ap = p - a;
        let cross = ab.cross(ac);
        let plane = cross.try_normalize()?;
        let sign = plane.dot(face_normal);
        if sign.abs() < 1. - 1e-8 || ap.dot(plane).abs() > 2e-6 {
            return None;
        }
        let area = cross.length_squared();
        let u = ap.cross(ac).dot(cross) / area;
        let v = ab.cross(ap).dot(cross) / area;
        let weights = [1. - u - v, u, v];
        if weights.iter().any(|&w| !(-1e-6..=1. + 1e-6).contains(&w)) {
            return None;
        }
        let n = std::iter::zip(ids, weights)
            .map(|(i, w)| DVec3::from(self.mesh.normals[i as usize]) * w)
            .sum::<DVec3>();
        Some(n.try_normalize()? * sign.signum())
    }
    fn triangle(&self, points: [DVec3; 3], native: [DVec3; 3]) -> Option<[DVec3; 3]> {
        let [a, b, c] = points;
        let n = (b - a).cross(c - a).try_normalize()?;
        if native.iter().any(|v| v.dot(n) < 1. - 1e-8) {
            return None;
        }
        // Most faces keep the three original corners through a Boolean.
        if let Some(&tri) = self.by_corners.get(&corner_key(points))
            && let [Some(x), Some(y), Some(z)] = points.map(|p| self.at(tri, p, n))
        {
            return Some([x, y, z]);
        }
        let bvh = self
            .bvh
            .get_or_init(|| Bvh::new(&self.mesh))
            .as_ref()
            .ok()?;
        let centre = a / 3. + b / 3. + c / 3.;
        let matched = bvh.closest(centre.to_array());
        if matched.t > 2e-6 {
            return None;
        }
        self.at(matched.triangle, centre, n)?;
        let mut result = [DVec3::ZERO; 3];
        for (i, p) in points.into_iter().enumerate() {
            result[i] = if let Some(v) = self.at(matched.triangle, p, n) {
                v
            } else {
                // A kernel can join coplanar facets. Search inside this triangle at a seam.
                let nearby = bvh.closest(p.lerp(centre, 1e-7).to_array());
                self.at(nearby.triangle, p, n)?
            };
        }
        Some(result)
    }
}

pub(super) fn transfer(
    g: &Geometry,
    origins: Option<&FaceOrigins>,
    surface: &mut FaceMesh,
) -> Result<()> {
    let mesh = &mut surface.mesh;
    if mesh.normals.len() != mesh.vertices.len() {
        return Ok(());
    }
    let mut transferred = vec![false; mesh.triangles.len()];
    let mut proposals = vec![None; mesh.vertices.len()];
    let mut conflict = vec![false; mesh.vertices.len()];
    for s in sources(g) {
        let samples = Samples::new(&s)?;
        for (k, t) in mesh.triangles.iter().enumerate() {
            if transferred[k] {
                continue;
            }
            if let (Some(id), Some(ids)) = (
                s.id,
                surface
                    .face_of_triangle
                    .get(k)
                    .and_then(|f| origins?.get(f)),
            ) && !ids.contains(id)
            {
                continue;
            }
            let p = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
            let native = t.map(|i| DVec3::from(mesh.normals[i as usize]));
            let Some(normals) = samples.triangle(p, native) else {
                continue;
            };
            transferred[k] = true;
            for (&i, v) in t.iter().zip(normals) {
                let i = i as usize;
                if proposals[i].is_some_and(|old: DVec3| old.dot(v) < 1. - 1e-8) {
                    conflict[i] = true
                } else {
                    proposals[i] = Some(v);
                }
            }
        }
    }
    // Preserve the original normal where a vertex also belongs to an unrelated face.
    for (t, &matched) in mesh.triangles.iter().zip(&transferred) {
        if !matched {
            for &i in t {
                conflict[i as usize] = true
            }
        }
    }
    for (i, v) in proposals.into_iter().enumerate() {
        if let Some(n) = v
            && !conflict[i]
        {
            mesh.normals[i] = n.to_array();
        }
    }
    Ok(())
}
