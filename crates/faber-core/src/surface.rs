//! Curvature, draft and line-of-sight access at the displayed surface.

use faber_geometry::bvh::Bvh;
pub use faber_ports::SurfaceKind as FaceKind;
use faber_ports::{CurvedMesh, FaceInfo, FaceMesh, Mesh, Result};
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
#[path = "surface_estimate.rs"]
mod estimate;
pub use estimate::{checked_estimate_curvature, estimate_curvature};

const MAX_TRIANGLES: usize = 2_000_000;
const MAX_DIRECTIONS: usize = 64;

#[derive(Clone, Debug, Serialize)]
pub struct Resolution {
    pub triangles: usize,
    pub vertices: usize,
}

fn resolution(mesh: &Mesh) -> Resolution {
    Resolution {
        triangles: mesh.triangles.len(),
        vertices: mesh.vertices.len(),
    }
}

fn check_size(mesh: &Mesh) -> Result<()> {
    let n = mesh.triangles.len();
    if n == 0 {
        return Err("The mesh has no triangle.".into());
    }
    if n > MAX_TRIANGLES {
        return Err(format!(
            "The mesh has {n} triangles. The surface analysis limit is {MAX_TRIANGLES}."
        ));
    }
    Ok(())
}

fn check_face_mesh(mesh: &FaceMesh) -> Result<()> {
    check_size(&mesh.mesh)?;
    if mesh.face_of_triangle.len() != mesh.mesh.triangles.len() {
        return Err("Supply a face index for each triangle.".into());
    }
    if mesh.mesh.vertices.iter().flatten().any(|v| !v.is_finite()) {
        return Err("Mesh coordinates must be finite.".into());
    }
    if mesh
        .mesh
        .triangles
        .iter()
        .flatten()
        .any(|&i| i as usize >= mesh.mesh.vertices.len())
    {
        return Err("A triangle refers to a missing vertex.".into());
    }
    Ok(())
}

struct Triangle {
    centre: DVec3,
    normal: DVec3,
    area: f64,
}

impl Triangle {
    fn ray_origin(&self) -> [f64; 3] {
        let shifted = self.centre + 1e-6 * self.normal;
        std::array::from_fn(|i| {
            // Keep the start outside the face when the offset is below one coordinate step.
            if shifted[i] == self.centre[i] && self.normal[i] > 0. {
                shifted[i].next_up()
            } else if shifted[i] == self.centre[i] && self.normal[i] < 0. {
                shifted[i].next_down()
            } else {
                shifted[i]
            }
        })
    }
}

fn triangle(mesh: &Mesh, indices: [u32; 3]) -> Result<Triangle> {
    let [a, b, c] = indices.map(|i| DVec3::from(mesh.vertices[i as usize]));
    let cross = (b - a).cross(c - a);
    let magnitude = cross.length();
    if !magnitude.is_finite() {
        return Err("A triangle exceeds the surface analysis coordinate limit.".into());
    }
    Ok(Triangle {
        centre: a + ((b - a) + (c - a)) / 3.,
        normal: if magnitude > 0. {
            cross / magnitude
        } else {
            DVec3::ZERO
        },
        area: magnitude * 0.5,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct FaceCurvature {
    pub face: u32,
    pub kind: FaceKind,
    pub min_convex_radius: Option<f64>,
    pub min_concave_radius: Option<f64>,
    pub area: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CurvatureReport {
    pub method: &'static str,
    pub resolution: Resolution,
    pub min_convex_radius_mm: Option<f64>,
    pub min_concave_radius_mm: Option<f64>,
    pub smallest_concave_faces: Vec<u32>,
    pub faces: Vec<FaceCurvature>,
    pub limits: Vec<String>,
}

impl CurvatureReport {
    pub fn tool_faces(&self, tool_radius_mm: f64) -> Result<Vec<FaceCurvature>> {
        if !tool_radius_mm.is_finite() || tool_radius_mm <= 0. {
            return Err("The tool radius must be finite and positive.".into());
        }
        Ok(self
            .faces
            .iter()
            .filter(|f| {
                f.min_concave_radius
                    .is_some_and(|r| r < tool_radius_mm && !equal_radius(r, tool_radius_mm))
            })
            .cloned()
            .collect())
    }
}

fn equal_radius(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.max(b)
}

fn include_minimum(current: &mut Option<f64>, radius: f64) {
    if radius.is_finite() && current.is_none_or(|r| radius < r) {
        *current = Some(radius);
    }
}

fn empty_curvature(mesh: &Mesh) -> CurvatureReport {
    CurvatureReport {
        method: "Principal curvature at surface vertices.",
        resolution: resolution(mesh),
        min_convex_radius_mm: None,
        min_concave_radius_mm: None,
        smallest_concave_faces: Vec::new(),
        faces: Vec::new(),
        limits: Vec::new(),
    }
}

pub fn checked_curvature_report(mesh: &CurvedMesh, faces: &[FaceInfo]) -> Result<CurvatureReport> {
    check_face_mesh(&mesh.faces)?;
    if mesh.curvature.len() != mesh.faces.mesh.vertices.len()
        || mesh.curvature.iter().flatten().any(|v| !v.is_finite())
    {
        return Err("Supply two finite curvatures for each vertex.".into());
    }
    let mut grouped = BTreeMap::new();
    for (&indices, &face) in mesh
        .faces
        .mesh
        .triangles
        .iter()
        .zip(&mesh.faces.face_of_triangle)
    {
        let area = triangle(&mesh.faces.mesh, indices)?.area;
        if area == 0. {
            continue;
        }
        let row = grouped.entry(face).or_insert_with(|| FaceCurvature {
            face,
            kind: mesh.kinds.get(&face).copied().unwrap_or(FaceKind::Freeform),
            min_convex_radius: None,
            min_concave_radius: None,
            area: 0.,
        });
        row.area += area;
        for i in indices {
            for k in mesh.curvature[i as usize] {
                if k > 0. {
                    include_minimum(&mut row.min_convex_radius, 1. / k);
                }
                if k < 0. {
                    include_minimum(&mut row.min_concave_radius, -1. / k);
                }
            }
        }
    }
    for f in faces {
        if let Some(row) = grouped.get_mut(&f.index) {
            row.area = f.area;
        }
    }
    let mut report = empty_curvature(&mesh.faces.mesh);
    report.faces = grouped.into_values().collect();
    report.min_convex_radius_mm = report
        .faces
        .iter()
        .filter_map(|r| r.min_convex_radius)
        .min_by(f64::total_cmp);
    report.min_concave_radius_mm = report
        .faces
        .iter()
        .filter_map(|r| r.min_concave_radius)
        .min_by(f64::total_cmp);
    if let Some(min) = report.min_concave_radius_mm {
        report.smallest_concave_faces.extend(
            report
                .faces
                .iter()
                .filter(|r| r.min_concave_radius.is_some_and(|k| equal_radius(k, min)))
                .map(|r| r.face),
        );
    }
    if faces.is_empty() && mesh.kinds.is_empty() {
        report.method = "Local curvature fit on welded mesh vertices.";
        report
            .limits
            .push("Curvature of a mesh part is an estimate.".into());
        report
            .limits
            .push("The result depends on triangle size, winding and sharp edges.".into());
    } else {
        report
            .limits
            .push("The smallest radius between sample points can be smaller.".into());
        report.limits.push("At a singular surface point the sample comes from the interior of an adjacent triangle.".into());
    }
    report
        .limits
        .push("The radius check does not test the complete tool or its path.".into());
    Ok(report)
}

pub fn curvature_report(mesh: &CurvedMesh, faces: &[FaceInfo]) -> CurvatureReport {
    checked_curvature_report(mesh, faces).unwrap_or_else(|e| {
        let mut report = empty_curvature(&mesh.faces.mesh);
        report.limits.push(e);
        report
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CurvatureMode {
    Mean,
    Gauss,
    MinRadius,
}

fn curvature_value([a, b]: [f64; 2], mode: CurvatureMode) -> Option<f64> {
    let v = match mode {
        CurvatureMode::Mean => a * 0.5 + b * 0.5,
        CurvatureMode::Gauss => a * b,
        CurvatureMode::MinRadius => 1. / a.abs().max(b.abs()),
    };
    v.is_finite().then_some(v)
}

pub fn curvature_range(mesh: &CurvedMesh, mode: CurvatureMode) -> [f64; 2] {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for &id in mesh.faces.mesh.triangles.iter().flatten() {
        if let Some(v) = mesh
            .curvature
            .get(id as usize)
            .and_then(|&c| curvature_value(c, mode))
        {
            low = low.min(v);
            high = high.max(v);
        }
    }
    if low.is_finite() {
        [low, high]
    } else {
        [0.; 2]
    }
}

const FLAT: [u8; 3] = [184, 190, 197];
const LOW: [u8; 3] = [49, 108, 190];
const HIGH: [u8; 3] = [224, 103, 52];
fn blend(start: [u8; 3], end: [u8; 3], f: f64) -> [u8; 3] {
    std::array::from_fn(|i| {
        (f64::from(start[i]) + (f64::from(end[i]) - f64::from(start[i])) * f.clamp(0., 1.)).round()
            as u8
    })
}

pub fn curvature_colour(v: f64, mode: CurvatureMode, [lo, hi]: [f64; 2]) -> [u8; 3] {
    if !v.is_finite() || v == 0. || !lo.is_finite() || !hi.is_finite() || lo > hi {
        return FLAT;
    }
    let v = v.clamp(lo, hi);
    if mode == CurvatureMode::MinRadius {
        return blend(HIGH, LOW, if hi > lo { (v - lo) / (hi - lo) } else { 0.5 });
    }
    if v < 0. {
        blend(FLAT, LOW, if lo < 0. { v / lo } else { 1. })
    } else {
        blend(FLAT, HIGH, if hi > 0. { v / hi } else { 1. })
    }
}

pub fn curvature_colours(
    mesh: &CurvedMesh,
    mode: CurvatureMode,
    range: Option<[f64; 2]>,
) -> Vec<[u8; 3]> {
    let extent = range
        .filter(|[a, b]| a.is_finite() && b.is_finite() && a <= b)
        .unwrap_or_else(|| curvature_range(mesh, mode));
    mesh.curvature
        .iter()
        .map(|&c| curvature_value(c, mode).map_or(FLAT, |v| curvature_colour(v, mode, extent)))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DraftClass {
    Positive,
    Negative,
    Vertical,
    Undercut,
    BelowMinimum,
}

pub fn draft_colour(class: DraftClass) -> [u8; 3] {
    match class {
        DraftClass::Positive => [63, 175, 99],
        DraftClass::Negative => [62, 125, 206],
        DraftClass::Vertical => [242, 217, 98],
        DraftClass::BelowMinimum => [249, 182, 48],
        DraftClass::Undercut => [217, 65, 66],
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DraftFace {
    pub face: u32,
    pub class: DraftClass,
    pub min_deg: f64,
    pub max_deg: f64,
    pub area: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DraftReport {
    pub method: &'static str,
    pub resolution: Resolution,
    pub faces: Vec<DraftFace>,
    pub vertical_mm2: f64,
    pub below_minimum_mm2: f64,
    pub undercut_mm2: f64,
    pub parting_z: Option<f64>,
    pub limits: Vec<String>,
    #[serde(skip)]
    pub triangle_classes: Vec<DraftClass>,
}

fn direction(values: [f64; 3], name: &str) -> Result<DVec3> {
    let v = DVec3::from(values);
    if !v.is_finite() {
        return Err(format!("The {name} direction must be finite."));
    }
    (v / v.abs().max_element())
        .try_normalize()
        .ok_or_else(|| format!("The {name} direction has zero length."))
}

fn priority(c: DraftClass) -> u8 {
    match c {
        DraftClass::Undercut => 4,
        DraftClass::BelowMinimum => 3,
        DraftClass::Vertical => 2,
        DraftClass::Negative => 1,
        DraftClass::Positive => 0,
    }
}

fn edges(ids: [u32; 3]) -> [[u32; 2]; 3] {
    let [a, b, c] = ids;
    [[a, b], [b, c], [c, a]].map(|[i, j]| [i.min(j), i.max(j)])
}

// Opposed draft must meet on one complete horizontal ring.
fn parting_plane(mesh: &Mesh, classes: &[DraftClass]) -> Option<f64> {
    let (points, ids) = mesh.welded_vertices(1e-8).ok()?;
    let mut boundary = HashSet::new();
    for (t, &class) in mesh.triangles.iter().zip(classes) {
        if class != DraftClass::Negative {
            continue;
        }
        for e in edges(t.map(|i| ids[i as usize])) {
            if !boundary.insert(e) {
                boundary.remove(&e);
            }
        }
    }
    let mut ring = HashMap::<u32, Vec<u32>>::new();
    for (t, &class) in mesh.triangles.iter().zip(classes) {
        if class != DraftClass::Positive {
            continue;
        }
        for [a, b] in edges(t.map(|i| ids[i as usize])) {
            if boundary.remove(&[a, b]) {
                ring.entry(a).or_default().push(b);
                ring.entry(b).or_default().push(a);
            }
        }
    }
    if !boundary.is_empty() || ring.len() < 3 || ring.values().any(|v| v.len() != 2) {
        return None;
    }
    let &start = ring.keys().min()?;
    let height = points[start as usize][2];
    let mut visited = 1;
    let mut previous = start;
    let mut current = ring[&start][0];
    while current != start {
        if (points[current as usize][2] - height).abs() > 1e-6 {
            return None;
        }
        let neighbours = &ring[&current];
        let next = if neighbours[0] == previous {
            neighbours[1]
        } else {
            neighbours[0]
        };
        previous = current;
        current = next;
        visited += 1;
    }
    (visited == ring.len()).then_some(height)
}

pub fn draft_report(mesh: &FaceMesh, pull: [f64; 3], min_deg: f64) -> Result<DraftReport> {
    check_size(&mesh.mesh)?;
    let bvh = Bvh::new(&mesh.mesh)?;
    draft_report_sampled(mesh, mesh, pull, min_deg, &bvh)
}

fn inclination(tri: &Triangle, axis: DVec3) -> f64 {
    tri.normal.dot(axis).clamp(-1., 1.).asin().to_degrees()
}

fn draft_class(angle: f64, min_deg: f64) -> DraftClass {
    if angle.abs() <= 0.01 + 1e-12 {
        DraftClass::Vertical
    } else if angle.abs() + 1e-12 < min_deg {
        DraftClass::BelowMinimum
    } else if angle < 0. {
        DraftClass::Negative
    } else {
        DraftClass::Positive
    }
}

pub fn draft_report_sampled(
    mesh: &FaceMesh,
    original: &FaceMesh,
    pull: [f64; 3],
    min_deg: f64,
    bvh: &Bvh,
) -> Result<DraftReport> {
    let axis = direction(pull, "pull")?;
    if !min_deg.is_finite() || !(0. ..=90.).contains(&min_deg) {
        return Err("The minimum draft angle must be between 0 and 90 degrees.".into());
    }
    check_face_mesh(mesh)?;
    let mut report = DraftReport {
        method: "Triangle normals and rays in the pull direction.",
        resolution: resolution(&mesh.mesh),
        faces: Vec::new(),
        triangle_classes: Vec::with_capacity(mesh.mesh.triangles.len()),
        vertical_mm2: 0.,
        undercut_mm2: 0.,
        below_minimum_mm2: 0.,
        parting_z: None,
        limits: vec![
            "The analysis uses one ray at each triangle centre. A face can contain more than one draft class.".into(),
            "A parting height requires a Z pull and a single horizontal ring between faces with opposed draft.".into(),
        ],
    };
    let mut faces = BTreeMap::new();
    for (&indices, &face) in mesh.mesh.triangles.iter().zip(&mesh.face_of_triangle) {
        let tri = triangle(&mesh.mesh, indices)?;
        let angle = inclination(&tri, axis);
        let mut class = draft_class(angle, min_deg);
        if class != DraftClass::Vertical
            && bvh
                .first_hit(tri.ray_origin(), (axis * angle.signum()).into())
                .is_some()
        {
            class = DraftClass::Undercut;
        }
        match class {
            DraftClass::Undercut => report.undercut_mm2 += tri.area,
            DraftClass::BelowMinimum => report.below_minimum_mm2 += tri.area,
            DraftClass::Vertical => report.vertical_mm2 += tri.area,
            _ => {}
        }
        report.triangle_classes.push(class);
        if tri.area == 0. {
            continue;
        }
        let row = faces.entry(face).or_insert(DraftFace {
            face,
            class,
            min_deg: angle,
            max_deg: angle,
            area: 0.,
        });
        row.area += tri.area;
        row.min_deg = row.min_deg.min(angle);
        row.max_deg = row.max_deg.max(angle);
        if priority(class) > priority(row.class) {
            row.class = class;
        }
    }
    report.faces = faces.into_values().collect();
    if axis.x.abs() <= 1e-12 && axis.y.abs() <= 1e-12 && report.undercut_mm2 == 0. {
        if std::ptr::eq(mesh, original) {
            report.parting_z = parting_plane(&mesh.mesh, &report.triangle_classes);
        } else {
            check_face_mesh(original)?;
            let classes = original
                .mesh
                .triangles
                .iter()
                .map(|&t| {
                    Ok(draft_class(
                        inclination(&triangle(&original.mesh, t)?, axis),
                        min_deg,
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            report.parting_z = parting_plane(&original.mesh, &classes);
        }
    }
    Ok(report)
}

#[derive(Clone, Debug, Serialize)]
pub struct AccessReport {
    pub method: &'static str,
    pub resolution: Resolution,
    pub reachable_mm2: f64,
    pub hidden_mm2: f64,
    pub hidden_faces: Vec<u32>,
    pub limits: Vec<String>,
    #[serde(skip)]
    pub reachable: Vec<bool>,
}

pub fn access_colour(reachable: bool) -> [u8; 3] {
    if reachable {
        draft_colour(DraftClass::Positive)
    } else {
        draft_colour(DraftClass::Undercut)
    }
}

fn empty_access(mesh: &Mesh) -> AccessReport {
    AccessReport {
        method: "Line-of-sight rays from triangle centres.",
        resolution: resolution(mesh),
        reachable_mm2: 0.,
        hidden_mm2: 0.,
        hidden_faces: Vec::new(),
        reachable: Vec::new(),
        limits: vec![
            "Side faces count as hidden for a direction along them.".into(),
            "A hidden face has at least one hidden triangle. The rays do not include tool width."
                .into(),
        ],
    }
}

pub fn checked_access_report(
    mesh: &FaceMesh,
    bvh: &Bvh,
    directions: &[[f64; 3]],
    scene: Option<&Bvh>,
) -> Result<AccessReport> {
    if directions.is_empty() || directions.len() > MAX_DIRECTIONS {
        return Err(format!("Supply 1 to {MAX_DIRECTIONS} access directions."));
    }
    let rays = directions
        .iter()
        .map(|&r| direction(r, "access"))
        .collect::<Result<Vec<_>>>()?;
    check_face_mesh(mesh)?;
    let mut report = empty_access(&mesh.mesh);
    let mut hidden = BTreeSet::new();
    report.reachable.reserve(mesh.mesh.triangles.len());
    for (&indices, &face) in mesh.mesh.triangles.iter().zip(&mesh.face_of_triangle) {
        let tri = triangle(&mesh.mesh, indices)?;
        let origin = tri.ray_origin();
        let reached = rays.iter().any(|&d| {
            tri.normal.dot(d) > 1e-12
                && bvh.first_hit(origin, d.into()).is_none()
                && scene.is_none_or(|s| s.first_hit(origin, d.into()).is_none())
        });
        report.reachable.push(reached);
        if reached {
            report.reachable_mm2 += tri.area;
        } else if tri.area > 0. {
            report.hidden_mm2 += tri.area;
            hidden.insert(face);
        }
    }
    report.hidden_faces = hidden.into_iter().collect();
    Ok(report)
}

pub fn access_report(
    mesh: &FaceMesh,
    bvh: &Bvh,
    directions: &[[f64; 3]],
    scene: Option<&Bvh>,
) -> AccessReport {
    checked_access_report(mesh, bvh, directions, scene).unwrap_or_else(|e| {
        let mut report = empty_access(&mesh.mesh);
        report.limits.push(e);
        report
    })
}
