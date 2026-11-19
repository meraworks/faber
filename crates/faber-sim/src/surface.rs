//! Extract the surface between samples at cell centres.
use crate::Grid;
use faber_ports::Mesh;
use glam::DVec3;
use std::collections::HashMap;

// Faces in outward order, with corner numbers x + 2y + 4z.
const FACES: [[usize; 4]; 6] = [
    [0, 4, 6, 2],
    [1, 3, 7, 5],
    [0, 1, 5, 4],
    [2, 6, 7, 3],
    [0, 2, 3, 1],
    [4, 5, 7, 6],
];

/// March cubes around the cell centres, with one exterior sample layer.
pub fn iso_surface(grid: &Grid, values: &[f32], level: f32) -> Mesh {
    let mut mesh = extract(grid, values, level);
    set_normals(&mut mesh);
    mesh
}

fn extract(grid: &Grid, values: &[f32], level: f32) -> Mesh {
    let mut mesh = Mesh::default();
    if !level.is_finite()
        || values.len() != grid.fill.len()
        || values.iter().any(|v| !v.is_finite())
        || grid.n.contains(&0)
        || grid.n.iter().try_fold(1usize, |a, &b| a.checked_mul(b)) != Some(values.len())
        || !(grid.h > 0. && grid.h.is_finite())
        || values.len() > crate::CELL_LIMIT
    {
        return mesh;
    }
    let n = grid.n.map(|s| s + 2);
    let stride = [1, n[0], n[0] * n[1]];
    let mut edge_ids = HashMap::<(usize, usize), u32>::new();
    let origin = DVec3::from(grid.origin);
    for z in 0..n[2] - 1 {
        for y in 0..n[1] - 1 {
            for x in 0..n[0] - 1 {
                let sample = |corner: usize| {
                    let p = [x + (corner & 1), y + ((corner >> 1) & 1), z + (corner >> 2)];
                    let q: [usize; 3] = std::array::from_fn(|a| p[a].clamp(1, grid.n[a]) - 1);
                    let v = f64::from(values[q[0] + grid.n[0] * (q[1] + grid.n[1] * q[2])])
                        - f64::from(level);
                    let outside = (0..3).any(|a| p[a] == 0 || p[a] > grid.n[a]);
                    let value = if outside {
                        -v.abs().max(1e-12)
                    } else if v == 0. {
                        -1e-12
                    } else {
                        v
                    };
                    (
                        p[0] + stride[1] * p[1] + stride[2] * p[2],
                        origin + grid.h * (DVec3::from(p.map(|i| i as f64)) - DVec3::splat(0.5)),
                        value,
                    )
                };
                let s = std::array::from_fn::<_, 8, _>(sample);
                if s.iter().all(|v| v.2 > 0.) || s.iter().all(|v| v.2 < 0.) {
                    continue;
                }
                let mut edge = |a: usize, b: usize| {
                    let (i, p, va) = s[a];
                    let (j, q, vb) = s[b];
                    *edge_ids.entry((i.min(j), i.max(j))).or_insert_with(|| {
                        let v = mesh.vertices.len() as u32;
                        mesh.vertices.push(
                            p.lerp(q, (va / (va - vb)).clamp(1e-10, 1. - 1e-10))
                                .to_array(),
                        );
                        v
                    })
                };
                let mut links = Vec::with_capacity(12);
                for face in FACES {
                    let mut crossings = Vec::with_capacity(4);
                    for i in 0..4 {
                        let a = face[i];
                        let b = face[(i + 1) % 4];
                        if (s[a].2 > 0.) != (s[b].2 > 0.) {
                            crossings.push((edge(a, b), s[a].2 < 0.));
                        }
                    }
                    let mut connect = |a: (u32, bool), b: (u32, bool)| {
                        links.push(if a.1 { (a.0, b.0) } else { (b.0, a.0) })
                    };
                    match *crossings.as_slice() {
                        [a, b] => connect(a, b),
                        [a, b, c, d] => {
                            // Pair the intersections around the corners opposite to the face centre.
                            let centre = face.iter().map(|&i| s[i].2).sum::<f64>() > 0.;
                            if centre == (s[face[0]].2 > 0.) {
                                connect(a, b);
                                connect(c, d);
                            } else {
                                connect(b, c);
                                connect(d, a);
                            }
                        }
                        _ => {}
                    }
                }
                while let Some((first, mut next)) = links.pop() {
                    let mut ring = vec![first];
                    while next != first {
                        ring.push(next);
                        let Some(index) = links.iter().position(|&(a, _)| a == next) else {
                            break;
                        };
                        next = links.swap_remove(index).1;
                    }
                    if ring.len() == 3 {
                        mesh.triangles.push([ring[0], ring[1], ring[2]]);
                    } else if ring.len() > 3 {
                        let sum: DVec3 = ring
                            .iter()
                            .map(|&i| DVec3::from(mesh.vertices[i as usize]))
                            .sum();
                        let centre = mesh.vertices.len() as u32;
                        mesh.vertices.push((sum / ring.len() as f64).to_array());
                        for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                            mesh.triangles.push([*a, *b, centre]);
                        }
                    }
                }
            }
        }
    }
    mesh
}

pub(crate) fn finish_mesh(grid: &Grid, density: &[f32], keep: &[bool]) -> (Mesh, usize) {
    let mut mesh = extract(grid, density, 0.5);
    let mut neighbours = vec![Vec::new(); mesh.vertices.len()];
    for &[a, b, c] in &mesh.triangles {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            neighbours[u as usize].push(v as usize);
            neighbours[v as usize].push(u as usize);
        }
    }
    let mut seen = vec![false; neighbours.len()];
    let mut pieces = 0;
    for i in 0..seen.len() {
        if seen[i] {
            continue;
        }
        pieces += 1;
        let mut stack = vec![i];
        seen[i] = true;
        while let Some(u) = stack.pop() {
            for &v in &neighbours[u] {
                if !std::mem::replace(&mut seen[v], true) {
                    stack.push(v);
                }
            }
        }
    }
    let locked: Vec<_> = mesh
        .vertices
        .iter()
        .map(|&p| {
            let p = (DVec3::from(p) - DVec3::from(grid.origin)) / grid.h;
            let low = (p - DVec3::splat(1e-8)).floor().as_ivec3();
            let high = (p + DVec3::splat(1e-8)).floor().as_ivec3();
            (low.z..=high.z).any(|z| {
                (low.y..=high.y).any(|y| {
                    (low.x..=high.x).any(|x| {
                        let xyz = [x, y, z].map(|v| v as usize);
                        (0..3).all(|a| xyz[a] < grid.n[a])
                            && keep[xyz[0] + grid.n[0] * (xyz[1] + grid.n[1] * xyz[2])]
                    })
                })
            })
        })
        .collect();
    // A reverse Laplace step limits the shrinkage of narrow members.
    for step in [0.32, -0.33] {
        let old = &mesh.vertices;
        mesh.vertices = old
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                if locked[i] || neighbours[i].is_empty() {
                    v
                } else {
                    let mean = neighbours[i]
                        .iter()
                        .map(|&j| DVec3::from(old[j]))
                        .sum::<DVec3>()
                        / neighbours[i].len() as f64;
                    DVec3::from(v).lerp(mean, step).to_array()
                }
            })
            .collect();
    }
    set_normals(&mut mesh);
    (mesh, pieces)
}

fn set_normals(mesh: &mut Mesh) {
    let mut normals = vec![DVec3::ZERO; mesh.vertices.len()];
    for &t in &mesh.triangles {
        let [a, b, c] = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
        let n = (b - a).cross(c - a);
        for i in t {
            normals[i as usize] += n;
        }
    }
    mesh.normals = normals
        .into_iter()
        .map(|n| n.normalize_or_zero().to_array())
        .collect();
}
