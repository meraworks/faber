use faber_ports::{Mesh, Result};
use glam::DVec3;
use std::collections::{HashMap, HashSet};

pub(crate) const WELD_MM: f64 = 1e-6;
pub struct Weld {
    pub mesh: Mesh,
    pub map: Vec<u32>,
}

/// Weld referenced vertices to the tolerance grid. Keep the first position in each cell.
pub fn try_weld(mesh: &Mesh, tolerance_mm: f64) -> Result<Weld> {
    let (vertices, map) = mesh.welded_vertices(tolerance_mm)?;
    let triangles = mesh
        .triangles
        .iter()
        .map(|t| t.map(|i| map[i as usize]))
        .collect();
    Ok(Weld {
        mesh: Mesh {
            vertices,
            triangles,
            normals: Vec::new(),
        },
        map,
    })
}
pub fn weld(mesh: &Mesh, tolerance_mm: f64) -> Weld {
    try_weld(mesh, tolerance_mm).unwrap_or_else(|_| Weld {
        mesh: mesh.clone(),
        map: (0..mesh.vertices.len() as u32).collect(),
    })
}
pub(crate) fn points(mesh: &Mesh, t: [u32; 3]) -> [DVec3; 3] {
    t.map(|i| DVec3::from(mesh.vertices[i as usize]))
}
pub(crate) fn cross(mesh: &Mesh, t: [u32; 3]) -> DVec3 {
    let [a, b, c] = points(mesh, t);
    (b - a).cross(c - a)
}
pub(crate) fn volume(mesh: &Mesh, ids: &[usize]) -> f64 {
    if ids.is_empty() {
        return 0.;
    }
    let origin = points(mesh, mesh.triangles[ids[0]])[0];
    ids.iter()
        .map(|&i| {
            let [a, b, c] = points(mesh, mesh.triangles[i]).map(|p| p - origin);
            a.dot(b.cross(c)) / 6.
        })
        .sum()
}
pub(crate) fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}
pub(crate) fn sides(t: [u32; 3]) -> [(u32, u32); 3] {
    [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]
}
pub(crate) struct Shell {
    pub faces: Vec<usize>,
    pub closed: bool,
    pub orientable: bool,
}
pub(crate) struct Topology {
    pub edges: HashMap<(u32, u32), Vec<(usize, bool)>>,
    pub shell_of: Vec<usize>,
    pub shells: Vec<Shell>,
    pub flip: Vec<bool>,
    pub drop: Vec<bool>,
    pub degenerate: usize,
    pub duplicate: usize,
    pub open: usize,
    pub non_manifold: usize,
}
impl Topology {
    /// The triangles at a manifold vertex form one connected fan.
    pub fn split_vertices(&self, mesh: &Mesh) -> usize {
        let mut incident = vec![(0usize, 0usize); mesh.vertices.len()];
        for (face, triangle) in mesh.triangles.iter().enumerate() {
            if self.drop[face] {
                continue;
            }
            for &v in triangle {
                let entry = &mut incident[v as usize];
                entry.0 = face;
                entry.1 += 1;
            }
        }
        let mut visited = vec![usize::MAX; mesh.triangles.len()];
        let mut queue = Vec::new();
        let mut split = 0;
        for (vertex, (start, count)) in incident.into_iter().enumerate() {
            if count < 2 {
                continue;
            }
            queue.clear();
            queue.push(start);
            visited[start] = vertex;
            let mut i = 0;
            while i < queue.len() {
                let face = queue[i];
                i += 1;
                for other in mesh.triangles[face] {
                    if other as usize == vertex {
                        continue;
                    }
                    if let [a, b] = self.edges[&key(vertex as u32, other)].as_slice() {
                        let next = if a.0 == face { b.0 } else { a.0 };
                        if visited[next] != vertex {
                            visited[next] = vertex;
                            queue.push(next);
                        }
                    }
                }
            }
            split += usize::from(queue.len() != count);
        }
        split
    }
    pub fn new(mesh: &Mesh) -> Self {
        let n = mesh.triangles.len();
        let mut drop = vec![false; n];
        let mut triangles = HashSet::new();
        let mut edges = HashMap::<_, Vec<_>>::new();
        let (mut degenerate, mut duplicate) = (0, 0);
        for (i, &t) in mesh.triangles.iter().enumerate() {
            if cross(mesh, t).length_squared() == 0. {
                drop[i] = true;
                degenerate += 1;
                continue;
            }
            let mut sorted = t;
            sorted.sort_unstable();
            if !triangles.insert(sorted) {
                drop[i] = true;
                duplicate += 1;
                continue;
            }
            for (a, b) in sides(t) {
                edges.entry(key(a, b)).or_default().push((i, a < b));
            }
        }
        let open = edges.values().filter(|e| e.len() == 1).count();
        let non_manifold = edges.values().filter(|e| e.len() > 2).count();
        let mut shell_of = vec![usize::MAX; n];
        let mut flip = vec![false; n];
        let mut shells = Vec::new();
        for start in 0..n {
            if drop[start] || shell_of[start] != usize::MAX {
                continue;
            }
            let id = shells.len();
            let mut shell = Shell {
                faces: vec![start],
                closed: true,
                orientable: true,
            };
            shell_of[start] = id;
            let mut next = 0;
            while next < shell.faces.len() {
                let i = shell.faces[next];
                next += 1;
                for (a, b) in sides(mesh.triangles[i]) {
                    let neighbours = &edges[&key(a, b)];
                    let [first, second] = neighbours.as_slice() else {
                        shell.closed = false;
                        continue;
                    };
                    let (j, forward) = if first.0 == i { *second } else { *first };
                    let turn = flip[i] ^ (forward == (a < b));
                    if shell_of[j] == usize::MAX {
                        shell_of[j] = id;
                        flip[j] = turn;
                        shell.faces.push(j);
                    } else if flip[j] != turn {
                        shell.orientable = false;
                    }
                }
            }
            let turns = shell.faces.iter().filter(|&&i| flip[i]).count();
            if turns > shell.faces.len() / 2 {
                for &i in &shell.faces {
                    flip[i] = !flip[i];
                }
            }
            shells.push(shell);
        }
        Self {
            edges,
            shell_of,
            shells,
            flip,
            drop,
            degenerate,
            duplicate,
            open,
            non_manifold,
        }
    }
}

/// Remove vertices made unused by a topology change. Keep every surviving position.
pub(crate) fn compact(mesh: &mut Mesh) {
    let mut used = vec![false; mesh.vertices.len()];
    for &i in mesh.triangles.iter().flatten() {
        used[i as usize] = true;
    }
    let mut ids = vec![0; used.len()];
    let mut next = 0;
    for (i, b) in used.iter().enumerate() {
        if *b {
            ids[i] = next;
            next += 1;
        }
    }
    let mut i = 0;
    mesh.vertices.retain(|_| {
        i += 1;
        used[i - 1]
    });
    for t in &mut mesh.triangles {
        *t = t.map(|v| ids[v as usize]);
    }
    mesh.normals.clear();
}
