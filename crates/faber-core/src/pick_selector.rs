//! Make recipe selectors from one-based picks in the source solid.
use crate::Project;
use faber_model::{EdgeSelector, FaceSelector, Placement, ProjectData, Scalar};
use faber_ports::{Body, EdgeFace, EdgeInfo, EdgeLines, FaceInfo, Kernel, Result};
use glam::{DMat4, DVec3};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

const TOL: f64 = 1e-6;
const EDGE_ERROR: &str = "The picked edges cannot be described by one selector. Pick fewer edges.";
const FACE_ERROR: &str = "The picked faces cannot be described by one selector. Pick fewer faces.";

impl Project {
    /// Resolved recipe and solid in part coordinates before placement.
    pub fn local_geometry(
        &self,
        part: &str,
    ) -> Result<(&faber_model::Geometry, Option<&faber_ports::Shape>)> {
        let local = self
            .locals
            .get(part)
            .ok_or_else(|| format!("Unknown part: {part}"))?;
        Ok((&local.geometry, local.shape.as_ref()))
    }
    pub fn selected_local_edge(&self, part: &str, edge: &EdgeSelector) -> Result<u32> {
        let (_, shape) = self.local_geometry(part)?;
        let selected = crate::select::edges(
            self.kernel.as_ref(),
            shape.ok_or("Part has no exact shape.")?,
            edge,
        )?;
        match selected.as_slice() {
            [index] => Ok(*index),
            _ => Err("Select exactly one edge.".into()),
        }
    }
    pub fn selected_local_face(&self, part: &str, face: &FaceSelector) -> Result<u32> {
        let (_, shape) = self.local_geometry(part)?;
        let selected = crate::select::faces(
            self.kernel.as_ref(),
            shape.ok_or("Part has no exact shape.")?,
            face,
        )?;
        match selected.as_slice() {
            [index] => Ok(*index),
            _ => Err("Select exactly one face.".into()),
        }
    }
    pub fn unplaced_topology(&self, part: &str) -> Result<(Vec<EdgeInfo>, Vec<FaceInfo>)> {
        let local = self
            .locals
            .get(part)
            .ok_or_else(|| format!("Unknown part: {part}"))?;
        let shape = local.shape.as_ref().ok_or("Part has no exact shape")?;
        Ok((self.kernel.edges(shape)?, self.kernel.faces(shape)?))
    }
}

pub fn plane_at(centre: [f64; 3], normal: [f64; 3]) -> Result<Placement> {
    crate::select::plane_axes(centre, normal, 0.)
}

#[derive(Clone, Debug)]
pub struct PickTopology {
    pub edges: Vec<EdgeInfo>,
    pub faces: Vec<FaceInfo>,
    pub links: Vec<EdgeFace>,
    /// From assembled coordinates to recipe coordinates.
    pub inverse: DMat4,
}

impl PickTopology {
    pub fn read(kernel: &dyn Kernel, body: &Body, data: &ProjectData) -> Result<Self> {
        let shape = body.shape.as_ref().ok_or("Part has no exact shape")?;
        let part = data
            .parts
            .get(&body.id)
            .ok_or_else(|| format!("Unknown part: {}", body.id))?;
        let values = faber_expr::resolve(&data.params)?;
        let p = part
            .placement
            .map_numbers(&mut |s| crate::number(s, &values))?;
        let mut matrix = DMat4::from_mat3(crate::placement::rotation_matrix(p.rotation));
        matrix.w_axis = DVec3::from(p.translation).extend(1.);
        let inverse = matrix.inverse();
        let point = |p| inverse.transform_point3(DVec3::from(p)).to_array();
        let vector = |v| {
            inverse
                .transform_vector3(DVec3::from(v))
                .normalize()
                .to_array()
                .map(|x| if x.abs() < 1e-12 { 0. } else { x })
        };
        let mut edges = kernel.edges(shape)?;
        for edge in &mut edges {
            edge.centre = point(edge.centre);
            edge.direction = edge.direction.map(vector);
        }
        let mut faces = kernel.faces(shape)?;
        for face in &mut faces {
            face.centre = point(face.centre);
            face.normal = face.normal.map(vector);
            if let Some(c) = &mut face.cylinder {
                c.start = point(c.start);
                c.axis = vector(c.axis);
            }
        }
        Ok(Self {
            edges,
            faces,
            links: kernel.edge_faces(shape)?,
            inverse,
        })
    }

    pub fn edge_selector(&self, picked: &[u32]) -> Result<EdgeSelector<Scalar>> {
        let ids: BTreeSet<u32> = picked.iter().copied().collect();
        let Some(&start) = ids.first() else {
            return Err(EDGE_ERROR.into());
        };
        let mut adjacent = BTreeMap::<u32, BTreeSet<u32>>::new();
        for link in &self.links {
            adjacent.entry(link.face).or_default().insert(link.edge);
        }
        let faces: Vec<_> = adjacent
            .iter()
            .filter(|(_, edges)| edges.contains(&start) && ids.is_subset(edges))
            .collect();
        for (i, &(&a, ea)) in faces.iter().enumerate() {
            if ea == &ids
                && let Ok(face) = face_selector(&self.faces, &[a])
            {
                let mut s = edge_blank(ids.len()).map_numbers(&mut |v| Ok(Scalar::Number(*v)))?;
                s.of_face = Some(face);
                return Ok(s);
            }
            for &(&b, eb) in &faces[i + 1..] {
                if ea.intersection(eb).copied().eq(ids.iter().copied())
                    && let (Ok(first), Ok(second)) = (
                        face_selector(&self.faces, &[a]),
                        face_selector(&self.faces, &[b]),
                    )
                {
                    let mut s =
                        edge_blank(ids.len()).map_numbers(&mut |v| Ok(Scalar::Number(*v)))?;
                    s.between = Some([first, second]);
                    return Ok(s);
                }
            }
        }
        edge_selector(&self.edges, picked)
    }
}

fn edge_blank(expect: usize) -> EdgeSelector {
    EdgeSelector {
        between: None,
        of_face: None,
        parallel: None,
        near_centre: None,
        in_box: None,
        length: None,
        radius: None,
        tolerance: TOL,
        expect,
    }
}
fn face_blank(expect: usize) -> FaceSelector {
    FaceSelector {
        cylinder_radius: None,
        internal: None,
        created_by: None,
        normal: None,
        near_centre: None,
        in_box: None,
        area: None,
        tolerance: TOL,
        expect,
    }
}
fn bounds(points: impl Iterator<Item = [f64; 3]>) -> [[f64; 3]; 2] {
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for p in points {
        for (i, x) in p.into_iter().enumerate() {
            bounds[0][i] = bounds[0][i].min(x);
            bounds[1][i] = bounds[1][i].max(x);
        }
    }
    bounds
}

// A field can only narrow the result. Remove fields that exclude a picked item,
// then test the shortest combinations first.
fn exact_fields<S>(
    all: &BTreeSet<u32>,
    picked: &BTreeSet<u32>,
    base: S,
    fields: Vec<S>,
    matches: impl Fn(&S) -> BTreeSet<u32>,
    merge: impl Fn(&mut S, &S),
) -> Option<S>
where
    S: Clone,
{
    if all == picked {
        return Some(base);
    }
    let mut usable = Vec::new();
    for field in fields {
        let found = matches(&field);
        if found == *picked {
            return Some(field);
        }
        if found != *all && picked.is_subset(&found) {
            usable.push(field);
        }
    }
    let mut states = vec![(base, 0usize, all.clone())];
    let mut best = None;
    for field in usable {
        let mut additional = Vec::new();
        for (s, count, previous) in &states {
            let depth = count + 1;
            if best.as_ref().is_some_and(|(_, n)| depth >= *n) {
                continue;
            }
            let mut combined = s.clone();
            merge(&mut combined, &field);
            let candidates = matches(&combined);
            if candidates == *picked {
                best = Some((combined, depth));
            } else if candidates.is_subset(previous)
                && candidates != *previous
                && !states
                    .iter()
                    .chain(&additional)
                    .any(|(_, n, found)| *n <= depth && *found == candidates)
            {
                additional.push((combined, depth, candidates));
            }
        }
        states.extend(additional);
    }
    best.map(|(s, _)| s)
}

pub fn edge_selector(edges: &[EdgeInfo], picked: &[u32]) -> Result<EdgeSelector<Scalar>> {
    let ids: BTreeSet<_> = picked.iter().copied().collect();
    let all = edges.iter().map(|e| e.index).collect();
    let first = edges
        .iter()
        .find(|e| ids.contains(&e.index))
        .ok_or(EDGE_ERROR)?;
    let base = edge_blank(ids.len());
    let mut fields = Vec::new();
    if let Some(v) = first.direction {
        fields.push(EdgeSelector {
            parallel: Some(v),
            ..base.clone()
        });
    }
    if let Some(v) = first.radius {
        fields.push(EdgeSelector {
            radius: Some(v),
            ..base.clone()
        });
    }
    fields.push(EdgeSelector {
        length: Some(first.length),
        ..base.clone()
    });
    if ids.len() == 1 {
        fields.push(EdgeSelector {
            near_centre: Some(first.centre),
            ..base.clone()
        });
    }
    fields.push(EdgeSelector {
        in_box: Some(bounds(
            edges
                .iter()
                .filter(|e| ids.contains(&e.index))
                .map(|e| e.centre),
        )),
        ..base.clone()
    });
    let s = exact_fields(
        &all,
        &ids,
        base,
        fields,
        |s| {
            edges
                .iter()
                .filter(|e| crate::select::edge_failures(e, s).is_empty())
                .map(|e| e.index)
                .collect()
        },
        |a, b| {
            a.parallel = a.parallel.or(b.parallel);
            a.radius = a.radius.or(b.radius);
            a.length = a.length.or(b.length);
            a.near_centre = a.near_centre.or(b.near_centre);
            a.in_box = a.in_box.or(b.in_box);
        },
    )
    .ok_or(EDGE_ERROR)?;
    s.map_numbers(&mut |v| Ok(Scalar::Number(*v)))
}

pub fn face_selector(faces: &[FaceInfo], picked: &[u32]) -> Result<FaceSelector<Scalar>> {
    let ids: BTreeSet<_> = picked.iter().copied().collect();
    let all = faces.iter().map(|f| f.index).collect();
    let first = faces
        .iter()
        .find(|f| ids.contains(&f.index))
        .ok_or(FACE_ERROR)?;
    let base = face_blank(ids.len());
    let mut fields = Vec::new();
    if let Some(v) = first.normal {
        fields.push(FaceSelector {
            normal: Some(v),
            ..base.clone()
        });
    }
    for id in first.created_by.iter().flatten() {
        fields.push(FaceSelector {
            created_by: Some(id.clone()),
            ..base.clone()
        });
    }
    if let Some(c) = &first.cylinder {
        fields.push(FaceSelector {
            internal: Some(c.internal),
            ..base.clone()
        });
        fields.push(FaceSelector {
            cylinder_radius: Some(c.radius),
            ..base.clone()
        });
    }
    fields.push(FaceSelector {
        area: Some(first.area),
        ..base.clone()
    });
    if ids.len() == 1 {
        fields.push(FaceSelector {
            near_centre: Some(first.centre),
            ..base.clone()
        });
    }
    fields.push(FaceSelector {
        in_box: Some(bounds(
            faces
                .iter()
                .filter(|f| ids.contains(&f.index))
                .map(|f| f.centre),
        )),
        ..base.clone()
    });
    let origins: BTreeMap<_, BTreeSet<_>> = faces
        .iter()
        .filter_map(|f| {
            f.created_by
                .as_ref()
                .map(|ids| (f.index, ids.iter().cloned().collect()))
        })
        .collect();
    let s = exact_fields(
        &all,
        &ids,
        base,
        fields,
        |s| {
            faces
                .iter()
                .filter(|f| crate::select::face_failures(f, s, origins.get(&f.index)).is_empty())
                .map(|f| f.index)
                .collect()
        },
        |a, b| {
            a.normal = a.normal.or(b.normal);
            a.internal = a.internal.or(b.internal);
            a.cylinder_radius = a.cylinder_radius.or(b.cylinder_radius);
            a.near_centre = a.near_centre.or(b.near_centre);
            a.in_box = a.in_box.or(b.in_box);
            a.area = a.area.or(b.area);
            if b.created_by.is_some() {
                a.created_by.clone_from(&b.created_by);
            }
        },
    )
    .ok_or(FACE_ERROR)?;
    s.map_numbers(&mut |v| Ok(Scalar::Number(*v)))
}

#[derive(Clone, Copy)]
struct End {
    edge: u32,
    point: DVec3,
    tangent: DVec3,
}
fn ends(lines: &EdgeLines, edge: u32) -> Vec<End> {
    let Some(points) = lines.edge(edge) else {
        return Vec::new();
    };
    let one = |points: &mut dyn Iterator<Item = &[f64; 3]>| -> Option<End> {
        let point = DVec3::from(*points.next()?);
        let next = points
            .map(|p| DVec3::from(*p))
            .find(|p| p.distance_squared(point) > 1e-20)?;
        let chord = next - point;
        let mut tangent = chord.normalize();
        // Use the next chord to estimate the tangent at a curved endpoint.
        if let Some(third) = points
            .map(|p| DVec3::from(*p))
            .find(|p| p.distance_squared(next) > 1e-20)
        {
            let other = third - next;
            tangent = (tangent
                + (tangent - other.normalize())
                    * (chord.length() / (chord.length() + other.length())))
            .normalize_or_zero();
        }
        Some(End {
            edge,
            point,
            tangent,
        })
    };
    [one(&mut points.iter()), one(&mut points.iter().rev())]
        .into_iter()
        .flatten()
        .collect()
}

fn connected(
    lines: &EdgeLines,
    allowed: impl Iterator<Item = u32>,
    start: u32,
    tangent: bool,
) -> Vec<u32> {
    let mut cells = BTreeMap::<[i64; 3], Vec<End>>::new();
    let mut topology = BTreeMap::new();
    let cell = |p: DVec3| p.to_array().map(|x| (x / TOL).floor() as i64);
    for id in allowed {
        let list = ends(lines, id);
        for end in &list {
            cells.entry(cell(end.point)).or_default().push(*end);
        }
        if !list.is_empty() {
            topology.insert(id, list);
        }
    }
    if !topology.contains_key(&start) {
        return Vec::new();
    }
    let mut result = BTreeSet::from([start]);
    let mut pending = VecDeque::from([start]);
    while let Some(edge) = pending.pop_front() {
        for end in &topology[&edge] {
            let [x, y, z] = cell(end.point);
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for dz in -1..=1 {
                        for peer in cells.get(&[x + dx, y + dy, z + dz]).into_iter().flatten() {
                            if end.point.distance_squared(peer.point) <= TOL * TOL
                                && (!tangent || end.tangent.dot(peer.tangent) < -0.99999)
                                && result.insert(peer.edge)
                            {
                                pending.push_back(peer.edge);
                            }
                        }
                    }
                }
            }
        }
    }
    result.into_iter().collect()
}

pub fn tangent_chain(lines: &EdgeLines, start: u32) -> Vec<u32> {
    connected(lines, 1..=lines.ranges.len() as u32, start, true)
}

pub fn edge_loop(lines: &EdgeLines, links: &[EdgeFace], face: u32, near: u32) -> Vec<u32> {
    let members: BTreeSet<_> = links
        .iter()
        .filter(|l| l.face == face)
        .map(|l| l.edge)
        .collect();
    connected(lines, members.into_iter(), near, false)
}

/// A purchased solid can be measured and used as a sketch reference.
pub fn editable_part(id: &str, part: &faber_model::Part<Scalar>) -> Result<()> {
    fn purchased(g: &faber_model::Geometry<Scalar>) -> bool {
        use faber_model::Geometry;
        match g {
            Geometry::Imported { purchased, .. } => purchased.is_some(),
            Geometry::History { features, .. } => features.iter().any(|f| purchased(&f.geometry)),
            _ => false,
        }
    }
    if purchased(&part.geometry) {
        Err(format!("Part {id} is a purchased part. It cannot change."))
    } else {
        Ok(())
    }
}
