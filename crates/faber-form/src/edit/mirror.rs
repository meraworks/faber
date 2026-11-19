use super::*;
use std::collections::HashMap;

pub(super) struct Reflection {
    pub mask: u8,
    vertices: Vec<u32>,
}
impl Reflection {
    pub fn vertex(&self, id: u32) -> u32 {
        if self.mask == 0 {
            id
        } else {
            self.vertices[id as usize]
        }
    }
    pub fn reflect(&self, p: DVec3) -> DVec3 {
        reflect(p, self.mask)
    }
}
fn reflect(mut p: DVec3, mask: u8) -> DVec3 {
    for i in 0..3 {
        if mask & (1 << i) != 0 {
            p[i] = -p[i];
        }
    }
    p
}
fn face_key(mut f: Vec<u32>) -> Vec<u32> {
    f.sort_unstable();
    f
}
pub(super) struct Mirrors {
    pub maps: Vec<Reflection>,
    faces: HashMap<Vec<u32>, u32>,
}
impl Mirrors {
    pub fn new(d: &Data, planes: &[SymmetryPlane]) -> Result<Self> {
        let mut maps = vec![Reflection {
            mask: 0,
            vertices: vec![],
        }];
        if planes.is_empty() {
            return Ok(Self {
                maps,
                faces: HashMap::new(),
            });
        }
        let size = d
            .vertices
            .iter()
            .flatten()
            .fold(0.0_f64, |s, x| s.max(x.abs()));
        let tol = (size * 1e-10).max(1e-10);
        let cell = |p: DVec3| p.to_array().map(|x| (x / tol).round() as i64);
        let mut positions = HashMap::<_, Vec<u32>>::new();
        for (i, &p) in d.vertices.iter().enumerate() {
            positions.entry(cell(p.into())).or_default().push(i as u32);
        }
        let faces: HashMap<_, _> = d
            .faces
            .iter()
            .enumerate()
            .map(|(i, f)| (face_key(f.clone()), i as u32))
            .collect();
        let creases = crease_map(d);
        for plane in planes.iter().collect::<BTreeSet<_>>() {
            let axis = match plane {
                SymmetryPlane::X => 0,
                SymmetryPlane::Y => 1,
                SymmetryPlane::Z => 2,
            };
            let mask = 1 << axis;
            let mut image = Vec::with_capacity(d.vertices.len());
            for (i, &p) in d.vertices.iter().enumerate() {
                if p[axis].abs() <= tol {
                    image.push(i as u32);
                    continue;
                }
                let q = reflect(p.into(), mask);
                let key = cell(q);
                let at = |key| {
                    positions.get(&key).and_then(|ids| {
                        ids.iter()
                            .copied()
                            .find(|&id| point(d, id).distance(q) <= tol)
                    })
                };
                let mut hit = at(key);
                if hit.is_none() {
                    'near: for dx in -1..=1 {
                        for dy in -1..=1 {
                            for dz in -1..=1 {
                                if let Some(id) = at([key[0] + dx, key[1] + dy, key[2] + dz]) {
                                    hit = Some(id);
                                    break 'near;
                                }
                            }
                        }
                    }
                }
                image.push(
                    hit.ok_or_else(|| format!("Vertex {i} has no mirror on the {plane:?} plane."))?,
                );
            }
            if image
                .iter()
                .enumerate()
                .any(|(i, &j)| image[j as usize] != i as u32)
            {
                return Err(format!("The {plane:?} plane has an ambiguous vertex pair."));
            }
            for (i, f) in d.faces.iter().enumerate() {
                let key = face_key(f.iter().map(|&v| image[v as usize]).collect());
                if !faces.contains_key(&key) {
                    return Err(format!("Face {i} has no mirror on the {plane:?} plane."));
                }
            }
            for (&[a, b], &s) in &creases {
                if creases
                    .get(&edge(image[a as usize], image[b as usize]))
                    .copied()
                    .unwrap_or(0.)
                    != s
                {
                    return Err(format!(
                        "Crease {a}-{b} has no matching mirror on the {plane:?} plane."
                    ));
                }
            }
            let extra: Vec<_> = maps
                .iter()
                .map(|r| Reflection {
                    mask: r.mask | mask,
                    vertices: (0..image.len() as u32)
                        .map(|id| image[r.vertex(id) as usize])
                        .collect(),
                })
                .collect();
            maps.extend(extra);
        }
        Ok(Self { maps, faces })
    }
    pub fn face(&self, d: &Data, r: &Reflection, id: u32) -> Result<u32> {
        check_index(id, d.faces.len(), "face")?;
        if r.mask == 0 {
            return Ok(id);
        }
        let key = face_key(d.faces[id as usize].iter().map(|&v| r.vertex(v)).collect());
        self.faces
            .get(&key)
            .copied()
            .ok_or_else(|| format!("Face {id} has no matching mirror."))
    }
    pub fn faces(&self, d: &Data, ids: &[u32]) -> Result<BTreeSet<u32>> {
        if ids.is_empty() {
            return Err("Select at least one face.".into());
        }
        ids.iter()
            .flat_map(|&id| self.maps.iter().map(move |r| self.face(d, r, id)))
            .collect()
    }
}
