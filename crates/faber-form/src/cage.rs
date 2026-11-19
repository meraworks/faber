use crate::CageData;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_FACES: usize = 400_000;

#[derive(Clone, Debug)]
pub(crate) struct Edge {
    pub vertices: [u32; 2],
    pub faces: [usize; 2],
    pub sharpness: f64,
}

#[derive(Clone, Debug)]
pub struct Cage {
    data: CageData,
    pub(crate) links: Vec<Edge>,
    pub(crate) face_edges: Vec<Vec<usize>>,
    pub(crate) vertex_edges: Vec<Vec<usize>>,
    pub(crate) vertex_faces: Vec<Vec<usize>>,
}

impl Cage {
    pub fn from_data(data: &CageData) -> Result<Self, String> {
        check_size(data)?;
        Self::build(data.clone())
    }

    pub(crate) fn build(data: CageData) -> Result<Self, String> {
        check_size(&data)?;
        for (i, p) in data.vertices.iter().enumerate() {
            if p.iter().any(|v| !v.is_finite()) {
                return Err(format!("Vertex {i} has a coordinate that is not finite."));
            }
        }
        let mut vertex_faces = vec![Vec::new(); data.vertices.len()];
        let mut uses = BTreeMap::<[u32; 2], Vec<(usize, usize, bool)>>::new();
        let mut face_edges = Vec::with_capacity(data.faces.len());
        for (i, face) in data.faces.iter().enumerate() {
            if !(3..=4).contains(&face.len()) {
                return Err(format!("Face {i} must have three or four vertices."));
            }
            for (j, &v) in face.iter().enumerate() {
                if face[..j].contains(&v) {
                    return Err(format!("Face {i} names vertex {v} two times."));
                }
                let vf = vertex_faces
                    .get_mut(v as usize)
                    .ok_or_else(|| format!("Face {i} names missing vertex {v}."))?;
                vf.push(i);
                let w = face[(j + 1) % face.len()];
                uses.entry([v.min(w), v.max(w)])
                    .or_default()
                    .push((i, j, v < w));
            }
            face_edges.push(vec![0; face.len()]);
        }
        if let Some(i) = vertex_faces.iter().position(Vec::is_empty) {
            return Err(format!("Vertex {i} is not used."));
        }
        let mut creases = BTreeMap::new();
        for c in &data.creases {
            let [a, b] = c.edge;
            let key = [a.min(b), a.max(b)];
            if !(0.0..=10.).contains(&c.sharpness) {
                return Err(format!("Crease on edge {a}-{b} must be between 0 and 10."));
            }
            if !uses.contains_key(&key) {
                return Err(format!("The crease names missing edge {a}-{b}."));
            }
            if creases.insert(key, c.sharpness).is_some() {
                return Err(format!("The crease on edge {a}-{b} occurs twice."));
            }
        }
        let mut links = Vec::with_capacity(uses.len());
        let mut vertex_edges = vec![Vec::new(); data.vertices.len()];
        for (vertices, uses) in uses {
            let [a, b] = vertices;
            if uses.len() == 1 {
                return Err(format!("Edge {a}-{b} has 1 face. The cage must be closed."));
            }
            let [left, right] = uses.as_slice() else {
                return Err(format!("Edge {a}-{b} has {} faces.", uses.len()));
            };
            if left.2 == right.2 {
                return Err(format!(
                    "The faces at edge {a}-{b} have inconsistent winding."
                ));
            }
            let id = links.len();
            for &(f, j, _) in [left, right] {
                face_edges[f][j] = id;
            }
            for v in [a, b] {
                vertex_edges[v as usize].push(id);
            }
            links.push(Edge {
                vertices,
                faces: [left.0, right.0],
                sharpness: creases.get(&vertices).copied().unwrap_or(0.),
            });
        }
        let cage = Self {
            data,
            links,
            face_edges,
            vertex_edges,
            vertex_faces,
        };
        for (v, faces) in cage.vertex_faces.iter().enumerate() {
            let mut reached = BTreeSet::new();
            cage.visit_fan(v, faces[0], &mut reached, false);
            if reached.len() != faces.len() {
                return Err(format!("Vertex {v} joins separate face fans."));
            }
        }
        for i in 0..cage.data.faces.len() {
            crate::edit::check_face_geometry(&cage.data, i)?;
        }
        Ok(cage)
    }

    pub(crate) fn visit_fan(
        &self,
        vertex: usize,
        start: usize,
        reached: &mut BTreeSet<usize>,
        split_crease: bool,
    ) {
        let mut stack = vec![start];
        while let Some(f) = stack.pop() {
            if !reached.insert(f) {
                continue;
            }
            let face = &self.data.faces[f];
            let j = face.iter().position(|&v| v as usize == vertex).unwrap();
            for k in [j, (j + face.len() - 1) % face.len()] {
                let e = &self.links[self.face_edges[f][k]];
                if !split_crease || e.sharpness < 1. {
                    stack.push(if e.faces[0] == f {
                        e.faces[1]
                    } else {
                        e.faces[0]
                    });
                }
            }
        }
    }
    pub(crate) fn vertices_mut(&mut self) -> &mut [[f64; 3]] {
        &mut self.data.vertices
    }
    pub fn data(&self) -> &CageData {
        &self.data
    }
    pub fn to_data(&self) -> CageData {
        self.data.clone()
    }
    pub fn edges(&self) -> Vec<[u32; 2]> {
        self.links.iter().map(|e| e.vertices).collect()
    }
    pub fn euler(&self) -> i64 {
        self.data.vertices.len() as i64 - self.links.len() as i64 + self.data.faces.len() as i64
    }
}

fn check_size(data: &CageData) -> Result<(), String> {
    if data.faces.is_empty() {
        return Err("The cage has no faces.".into());
    }
    if data.faces.len() > MAX_FACES
        || data.vertices.len() > MAX_FACES * 4
        || data.creases.len() > MAX_FACES * 2
    {
        return Err(format!(
            "The cage exceeds the limit of {MAX_FACES} faces or their vertices and edges."
        ));
    }
    if let Some(i) = data.faces.iter().position(|f| !(3..=4).contains(&f.len())) {
        return Err(format!("Face {i} must have three or four vertices."));
    }
    Ok(())
}
