//! Section analyses. A saved plane cuts the solids in views and in numbers. It never changes a solid.

use crate::history;
use faber_model::{Part, Placement, Scalar, SectionAnalysis, SectionPlane, SplitKeep};
use faber_ports::{Body, Kernel, Mesh, Result, Shape};
use glam::{DMat3, DVec3};
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// Largest distance for a cap face to count as lying on the plane.
const ON_PLANE_MM: f64 = 1e-5;
/// Tessellation tolerance for the cap boundaries.
const TESSELLATION_MM: f64 = 0.01;
const MAX_NOTE_CHARS: usize = 500;

/// A saved section in world coordinates.
pub struct ResolvedSection {
    pub origin: [f64; 3],
    pub normal: [f64; 3],
    pub reverse: bool,
    pub visible: bool,
    /// Part ids to cut. Empty cuts all parts.
    pub parts: Vec<String>,
}

/// The exact cut of one part by a section plane.
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq)]
pub struct CutPart {
    pub id: String,
    pub area_mm2: f64,
    /// Total length of all boundaries of the cut, holes included.
    pub perimeter_mm: f64,
    /// Bounds of the cut in plane coordinates, `[min, max]`. Absent when the plane misses the part.
    pub bounds_plane_mm: Option<[[f64; 2]; 2]>,
    /// Separate cut regions.
    pub regions: usize,
    /// Holes in all regions.
    pub holes: usize,
    /// Approximate. The least distance from an outer boundary to a hole boundary.
    /// Absent when no region has a hole.
    pub min_wall_mm: Option<f64>,
}

#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq)]
pub struct SectionReport {
    pub origin_mm: [f64; 3],
    pub normal: [f64; 3],
    /// Plane axes for the 2D bounds, `[x, y]`.
    pub axes: [[f64; 3]; 2],
    pub parts: Vec<CutPart>,
}

/// Checks a section against the project data. Parts and features are checked later.
pub(crate) fn check_refs(
    data: &faber_model::ProjectData,
    id: &str,
    section: &SectionAnalysis<Scalar>,
) -> Result<()> {
    if !crate::valid_id(id) {
        return Err(format!("Section id is reserved, invalid or too long: {id}"));
    }
    if section.note.chars().count() > MAX_NOTE_CHARS {
        return Err(format!(
            "Section note has more than {MAX_NOTE_CHARS} characters"
        ));
    }
    for part in &section.parts {
        if !data.parts.contains_key(part) {
            return Err(format!("Section {id} names unknown part: {part}"));
        }
    }
    let plane_part = match &section.plane {
        SectionPlane::Plane { .. } => None,
        SectionPlane::Reference { part, .. } | SectionPlane::Face { part, .. } => Some(part),
    };
    if let Some(part) = plane_part
        && !data.parts.contains_key(part)
    {
        return Err(format!("Section {id} names unknown part: {part}"));
    }
    Ok(())
}

/// Removes a part from every section list. A plane that uses the part blocks the removal.
pub(crate) fn detach_part(data: &mut faber_model::ProjectData, id: &str) -> Result<()> {
    for (section_id, section) in &data.sections {
        let used = match &section.plane {
            SectionPlane::Plane { .. } => false,
            SectionPlane::Reference { part, .. } | SectionPlane::Face { part, .. } => part == id,
        };
        if used {
            return Err(format!(
                "Section {section_id} uses part {id}. Remove the section first."
            ));
        }
    }
    for section in data.sections.values_mut() {
        section.parts.retain(|part| part != id);
    }
    Ok(())
}

/// Resolves the plane of a section with all numbers in millimetres and degrees.
/// Returns the origin and the unit normal in world coordinates.
pub(crate) fn world_plane(
    kernel: &dyn Kernel,
    parts: &BTreeMap<String, Part<Scalar>>,
    values: &BTreeMap<String, f64>,
    bodies: &[Body],
    histories: &BTreeMap<String, history::Cache>,
    section: &SectionAnalysis<f64>,
) -> Result<(DVec3, DVec3)> {
    match &section.plane {
        SectionPlane::Plane { origin, normal } => {
            let normal = unit(DVec3::from(*normal))?;
            Ok((DVec3::from(*origin), normal))
        }
        SectionPlane::Reference {
            part,
            feature,
            offset,
        } => {
            let local = reference_placement(parts, histories, part, feature)?;
            let (_, z) = frame_of(&local);
            let origin = DVec3::from(local.translation) + z * *offset;
            let placed = part_placement(parts, values, part)?;
            to_world(&placed, origin, z)
        }
        SectionPlane::Face {
            part,
            feature,
            face,
            offset,
        } => match feature {
            None => {
                let body = bodies
                    .iter()
                    .find(|body| &body.id == part)
                    .ok_or_else(|| format!("Unknown part: {part}"))?;
                let shape = body.shape.as_ref().ok_or("Part has no exact shape")?;
                let placed = crate::select::plane(kernel, shape, face, *offset)?;
                let (_, z) = frame_of(&placed);
                Ok((DVec3::from(placed.translation), z))
            }
            Some(feature) => {
                let shape = feature_shape(parts, histories, part, feature)?;
                let local = crate::select::plane(kernel, shape, face, *offset)?;
                let (_, z) = frame_of(&local);
                let placed = part_placement(parts, values, part)?;
                to_world(&placed, DVec3::from(local.translation), z)
            }
        },
    }
}

/// Measures the exact cut of one solid by the plane. Coordinates are world coordinates.
pub(crate) fn measure(
    kernel: &dyn Kernel,
    id: &str,
    shape: &Shape,
    origin: DVec3,
    normal: DVec3,
) -> Result<CutPart> {
    let (x, y) = frame(normal);
    let mut cut = CutPart {
        id: id.into(),
        area_mm2: 0.,
        perimeter_mm: 0.,
        bounds_plane_mm: None,
        regions: 0,
        holes: 0,
        min_wall_mm: None,
    };
    let bounds = kernel.properties(shape)?.bounds;
    let (lo, hi) = (DVec3::from(bounds[0]), DVec3::from(bounds[1]));
    let mut distances = [0.; 8];
    for (i, distance) in distances.iter_mut().enumerate() {
        let corner = DVec3::new(
            if i & 1 == 0 { lo.x } else { hi.x },
            if i & 2 == 0 { lo.y } else { hi.y },
            if i & 4 == 0 { lo.z } else { hi.z },
        );
        *distance = normal.dot(corner - origin);
    }
    let min = distances.iter().copied().fold(f64::INFINITY, f64::min);
    let max = distances.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if min >= 0. || max <= 0. {
        return Ok(cut);
    }
    let positive = kernel.split(
        shape,
        origin.to_array(),
        normal.to_array(),
        SplitKeep::Positive,
    )?;
    for face in kernel.faces(&positive)? {
        let Some(face_normal) = face.normal else {
            continue;
        };
        let centre = DVec3::from(face.centre);
        let parallel = DVec3::from(face_normal).dot(normal).abs() > 1. - 1e-9;
        if !parallel || normal.dot(centre - origin).abs() > ON_PLANE_MM {
            continue;
        }
        cut.area_mm2 += face.area;
        let face_shape = kernel.face_shape(&positive, face.index)?;
        cut.perimeter_mm += kernel
            .edges(&face_shape)?
            .iter()
            .map(|e| e.length)
            .sum::<f64>();
        let mesh = kernel.tessellate(&face_shape, TESSELLATION_MM)?;
        let points: Vec<[f64; 2]> = mesh
            .vertices
            .iter()
            .map(|v| {
                let d = DVec3::from(*v) - origin;
                [d.dot(x), d.dot(y)]
            })
            .collect();
        for p in &points {
            cut.bounds_plane_mm = Some(match cut.bounds_plane_mm {
                None => [*p, *p],
                Some([lo, hi]) => [
                    [lo[0].min(p[0]), lo[1].min(p[1])],
                    [hi[0].max(p[0]), hi[1].max(p[1])],
                ],
            });
        }
        let loops = boundary_loops(&mesh);
        cut.regions += 1;
        cut.holes += loops.len().saturating_sub(1);
        if let Some(wall) = wall_thickness(&loops, &points) {
            cut.min_wall_mm = Some(cut.min_wall_mm.map_or(wall, |w| w.min(wall)));
        }
    }
    Ok(cut)
}

/// The unit axes of the plane for 2D coordinates. The rule matches the reference planes.
pub(crate) fn frame(normal: DVec3) -> (DVec3, DVec3) {
    let seed = if normal.dot(DVec3::X).abs() > 0.999999 {
        DVec3::Y
    } else {
        DVec3::X
    };
    let x = (seed - normal * seed.dot(normal)).normalize();
    (x, normal.cross(x))
}

fn unit(normal: DVec3) -> Result<DVec3> {
    if !normal.is_finite() || normal.length() < 1e-10 {
        return Err("Section normal needs a non-zero length".into());
    }
    Ok(normal.normalize())
}

fn rotation(placement: &Placement) -> DMat3 {
    let [x, y, z] = placement.rotation.map(f64::to_radians);
    DMat3::from_rotation_z(z) * DMat3::from_rotation_y(y) * DMat3::from_rotation_x(x)
}

/// Origin and unit Z axis of a placement. Both are in the frame of the placement's parent.
fn frame_of(placement: &Placement) -> (DMat3, DVec3) {
    let rotation = rotation(placement);
    (rotation, (rotation * DVec3::Z).normalize())
}

fn part_placement(
    parts: &BTreeMap<String, Part<Scalar>>,
    values: &BTreeMap<String, f64>,
    id: &str,
) -> Result<Placement> {
    let part = parts.get(id).ok_or_else(|| format!("Unknown part: {id}"))?;
    part.placement
        .map_numbers(&mut |n| crate::number(n, values))
}

/// Moves a part-frame origin and normal into world coordinates.
fn to_world(placed: &Placement, origin: DVec3, normal: DVec3) -> Result<(DVec3, DVec3)> {
    let rotation = rotation(placed);
    let world = rotation * origin + DVec3::from(placed.translation);
    Ok((world, unit(rotation * normal)?))
}

fn reference_placement(
    parts: &BTreeMap<String, Part<Scalar>>,
    histories: &BTreeMap<String, history::Cache>,
    part: &str,
    feature: &str,
) -> Result<Placement> {
    let recipe = parts
        .get(part)
        .ok_or_else(|| format!("Unknown part: {part}"))?;
    let faber_model::Geometry::History { features, .. } = &recipe.geometry else {
        return Err(format!("Part {part} has no feature history"));
    };
    if !features.iter().any(|entry| entry.id == feature) {
        return Err(format!("Unknown feature {feature} in part {part}"));
    }
    histories
        .get(part)
        .and_then(|cache| cache.placement(feature))
        .ok_or_else(|| format!("Feature {feature} in part {part} is not a reference plane"))
}

fn feature_shape<'a>(
    parts: &BTreeMap<String, Part<Scalar>>,
    histories: &'a BTreeMap<String, history::Cache>,
    part: &str,
    feature: &str,
) -> Result<&'a Shape> {
    let recipe = parts
        .get(part)
        .ok_or_else(|| format!("Unknown part: {part}"))?;
    let faber_model::Geometry::History { features, .. } = &recipe.geometry else {
        return Err(format!("Part {part} has no feature history"));
    };
    if !features.iter().any(|entry| entry.id == feature) {
        return Err(format!("Unknown feature {feature} in part {part}"));
    }
    histories
        .get(part)
        .and_then(|cache| cache.shape(feature))
        .ok_or_else(|| format!("Unknown feature {feature} in part {part}"))
}

/// Boundary loops of a cap face. Each loop is a list of mesh segments.
fn boundary_loops(mesh: &Mesh) -> Vec<Vec<[u32; 2]>> {
    let mut count: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    for t in &mesh.triangles {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    let segments: Vec<[u32; 2]> = count
        .iter()
        .filter(|(_, n)| **n == 1)
        .map(|((a, b), _)| [*a, *b])
        .collect();
    let mut parent: HashMap<u32, u32> = HashMap::new();
    fn find(parent: &mut HashMap<u32, u32>, v: u32) -> u32 {
        let p = *parent.entry(v).or_insert(v);
        if p == v {
            return v;
        }
        let root = find(parent, p);
        parent.insert(v, root);
        root
    }
    for [a, b] in &segments {
        let ra = find(&mut parent, *a);
        let rb = find(&mut parent, *b);
        if ra != rb {
            parent.insert(ra, rb);
        }
    }
    let mut groups: BTreeMap<u32, Vec<[u32; 2]>> = BTreeMap::new();
    for segment in segments {
        let root = find(&mut parent, segment[0]);
        groups.entry(root).or_default().push(segment);
    }
    groups.into_values().collect()
}

/// Least distance from the outer loop of a region to any other loop of the same region.
fn wall_thickness(loops: &[Vec<[u32; 2]>], points: &[[f64; 2]]) -> Option<f64> {
    if loops.len() < 2 {
        return None;
    }
    let extent = |loop_: &Vec<[u32; 2]>| {
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for [a, b] in loop_ {
            for i in [*a, *b] {
                let p = points[i as usize];
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
        (hi[0] - lo[0]).hypot(hi[1] - lo[1])
    };
    let outer =
        (0..loops.len()).max_by(|&a, &b| extent(&loops[a]).total_cmp(&extent(&loops[b])))?;
    let mut least: Option<f64> = None;
    for (index, other) in loops.iter().enumerate() {
        if index == outer {
            continue;
        }
        for [a0, a1] in &loops[outer] {
            for [b0, b1] in other {
                let d = segment_distance(
                    points[*a0 as usize],
                    points[*a1 as usize],
                    points[*b0 as usize],
                    points[*b1 as usize],
                );
                least = Some(least.map_or(d, |l| l.min(d)));
            }
        }
    }
    least
}

fn segment_distance(a0: [f64; 2], a1: [f64; 2], b0: [f64; 2], b1: [f64; 2]) -> f64 {
    point_segment(a0, b0, b1)
        .min(point_segment(a1, b0, b1))
        .min(point_segment(b0, a0, a1))
        .min(point_segment(b1, a0, a1))
}

fn point_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0. {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0., 1.)
    } else {
        0.
    };
    (p[0] - a[0] - t * dx).hypot(p[1] - a[1] - t * dy)
}

impl crate::Project {
    /// Resolves a saved section to its world plane and display flags.
    pub fn section(&self, id: &str) -> Result<ResolvedSection> {
        let (origin, normal, section) = self.resolve_section(id)?;
        Ok(ResolvedSection {
            origin: origin.to_array(),
            normal: normal.to_array(),
            reverse: section.reverse,
            visible: section.visible,
            parts: section.parts.clone(),
        })
    }

    /// Reads the exact cut of each cut part by a saved section.
    pub fn section_info(&self, id: &str) -> Result<SectionReport> {
        self.cached("section_info", id, || self.section_report(id))
    }

    fn section_report(&self, id: &str) -> Result<SectionReport> {
        let (origin, normal, section) = self.resolve_section(id)?;
        let (x, y) = frame(normal);
        let mut parts = Vec::new();
        for body in &self.bodies {
            if !section.parts.is_empty() && !section.parts.contains(&body.id) {
                continue;
            }
            let shape = body
                .shape
                .as_ref()
                .ok_or_else(|| format!("Part {} has no exact shape", body.id))?;
            parts.push(measure(
                self.kernel.as_ref(),
                &body.id,
                shape,
                origin,
                normal,
            )?);
        }
        Ok(SectionReport {
            origin_mm: origin.to_array(),
            normal: normal.to_array(),
            axes: [x.to_array(), y.to_array()],
            parts,
        })
    }

    fn resolve_section(&self, id: &str) -> Result<(DVec3, DVec3, &SectionAnalysis<Scalar>)> {
        let section = self
            .data
            .sections
            .get(id)
            .ok_or_else(|| format!("Unknown section: {id}"))?;
        let values = faber_expr::resolve(&self.data.params)?;
        let numbers = section.map_numbers(&mut |n| crate::number(n, &values))?;
        let (origin, normal) = world_plane(
            self.kernel.as_ref(),
            &self.data.parts,
            &values,
            &self.bodies,
            &self.histories,
            &numbers,
        )
        .map_err(|e| format!("Section {id}: {e}"))?;
        Ok((origin, normal, section))
    }
}
