use crate::{Cage, CageData, Crease, MAX_FACES};
use faber_ports::Mesh;
use glam::DVec3;
use std::collections::BTreeSet;

fn face_centres(c: &Cage) -> Vec<DVec3> {
    let d = c.data();
    d.faces
        .iter()
        .map(|f| {
            f.iter()
                .map(|&i| DVec3::from(d.vertices[i as usize]) / (f.len() as f64))
                .sum()
        })
        .collect()
}
fn vertex_point(c: &Cage, centres: &[DVec3], i: usize) -> DVec3 {
    let vertices = &c.data().vertices;
    let p = DVec3::from(vertices[i]);
    let n = c.vertex_edges[i].len() as f64;
    let mut neighbours = DVec3::ZERO;
    let mut sharp = [(0., DVec3::ZERO); 3];
    for &id in &c.vertex_edges[i] {
        let e = &c.links[id];
        let [a, b] = e.vertices;
        let q = DVec3::from(vertices[if a as usize == i { b } else { a } as usize]);
        neighbours += q / n;
        let mut candidate = (e.sharpness, q);
        for ranked in &mut sharp {
            if candidate.0 > ranked.0 {
                std::mem::swap(ranked, &mut candidate);
            }
        }
    }
    let faces = c.vertex_faces[i]
        .iter()
        .map(|&f| centres[f] / n)
        .sum::<DVec3>();
    let smooth = faces / n + neighbours / n + p * ((n - 2.) / n);
    if sharp[1].0 == 0. {
        return smooth;
    }
    let crease = p * 0.75 + sharp[0].1 * 0.125 + sharp[1].1 * 0.125;
    smooth
        .lerp(crease, sharp[1].0.min(1.))
        .lerp(p, sharp[2].0.min(1.))
}
fn split(c: &Cage) -> Result<Cage, String> {
    let d = c.data();
    let centres = face_centres(c);
    let edge_index = d.vertices.len() as u32;
    let face_index = edge_index + c.links.len() as u32;
    let mut out = CageData {
        vertices: (0..d.vertices.len())
            .map(|i| vertex_point(c, &centres, i).to_array())
            .collect(),
        faces: Vec::with_capacity(c.links.len() * 2),
        creases: Vec::new(),
    };
    for (i, e) in c.links.iter().enumerate() {
        let [a, b] = e.vertices;
        let mid =
            DVec3::from(d.vertices[a as usize]) * 0.5 + DVec3::from(d.vertices[b as usize]) * 0.5;
        let smooth = mid * 0.5 + centres[e.faces[0]] * 0.25 + centres[e.faces[1]] * 0.25;
        out.vertices
            .push(smooth.lerp(mid, e.sharpness.min(1.)).to_array());
        if e.sharpness > 1. {
            for v in [a, b] {
                out.creases.push(Crease {
                    edge: [v, edge_index + i as u32],
                    sharpness: e.sharpness - 1.,
                });
            }
        }
    }
    out.vertices
        .extend(centres.into_iter().map(|p| p.to_array()));
    for (i, f) in d.faces.iter().enumerate() {
        for (j, &v) in f.iter().enumerate() {
            let edges = &c.face_edges[i];
            out.faces.push(vec![
                v,
                edge_index + edges[j] as u32,
                face_index + i as u32,
                edge_index + edges[(j + f.len() - 1) % f.len()] as u32,
            ]);
        }
    }
    Cage::build(out)
}

/// Split each face once using the Catmull-Clark face, edge and vertex rules.
pub fn subdivide_once(cage: &Cage) -> Result<Cage, String> {
    let count = cage.data().faces.iter().map(Vec::len).sum::<usize>();
    if count > MAX_FACES {
        return Err(format!(
            "The subdivision has {count} faces. The limit is {MAX_FACES}."
        ));
    }
    split(cage)
}

/// Triangulate the chosen level. At levels 1 to 4 the display samples lie
/// halfway between each control vertex and its next subdivision position.
pub fn subdivide(cage: &Cage, levels: u32) -> Result<Mesh, String> {
    sample(cage, levels, true)
}

/// Keep common vertex indexes across sharp edges for topology checks.
pub fn subdivide_topology(cage: &Cage, levels: u32) -> Result<Mesh, String> {
    sample(cage, levels, false)
}

fn sample(cage: &Cage, levels: u32, split_normals: bool) -> Result<Mesh, String> {
    if levels > 4 {
        return Err("Subdivision levels must be between 0 and 4.".into());
    }
    let faces = &cage.data().faces;
    let triangles: usize = if levels == 0 {
        faces.iter().map(|f| f.len() - 2).sum()
    } else {
        faces.iter().map(Vec::len).sum::<usize>() * 4usize.pow(levels - 1) * 2
    };
    if triangles > MAX_FACES {
        return Err(format!(
            "The result has {triangles} triangles. The limit is {MAX_FACES}."
        ));
    }
    let mut result = None;
    for _ in 0..levels {
        result = Some(split(result.as_ref().unwrap_or(cage))?);
    }
    triangulate(result.as_ref().unwrap_or(cage), levels > 0, split_normals)
}

fn triangulate(c: &Cage, fair: bool, split_normals: bool) -> Result<Mesh, String> {
    let d = c.data();
    let mut points = d
        .vertices
        .iter()
        .copied()
        .map(DVec3::from)
        .collect::<Vec<_>>();
    if fair {
        let centres = face_centres(c);
        // Sample halfway to the next vertex stencil at the final mesh resolution.
        for (i, p) in points.iter_mut().enumerate() {
            *p = *p * 0.5 + vertex_point(c, &centres, i) * 0.5;
        }
    }
    let face_normals = d
        .faces
        .iter()
        .map(|f| {
            let a = points[f[0] as usize];
            (1..f.len() - 1)
                .map(|j| (points[f[j] as usize] - a).cross(points[f[j + 1] as usize] - a))
                .sum::<DVec3>()
        })
        .collect::<Vec<_>>();
    if face_normals.iter().any(|n| !n.is_finite()) {
        return Err("The cage coordinates exceed the normal calculation range.".into());
    }
    let mut mesh = Mesh::default();
    let split_edges = split_normals && c.links.iter().any(|e| e.sharpness >= 1.);
    let corners = if !split_edges {
        mesh.vertices.extend(points.iter().map(|p| p.to_array()));
        mesh.normals.extend(c.vertex_faces.iter().map(|faces| {
            faces
                .iter()
                .map(|&f| face_normals[f])
                .sum::<DVec3>()
                .try_normalize()
                .unwrap_or(DVec3::Z)
                .to_array()
        }));
        std::borrow::Cow::Borrowed(&d.faces)
    } else {
        let mut corners = d.faces.iter().map(|f| vec![0; f.len()]).collect::<Vec<_>>();
        for (i, point) in points.into_iter().enumerate() {
            let mut done = BTreeSet::new();
            for &face in &c.vertex_faces[i] {
                if done.contains(&face) {
                    continue;
                }
                let mut group = BTreeSet::new();
                c.visit_fan(i, face, &mut group, true);
                let normal = group
                    .iter()
                    .map(|&f| face_normals[f])
                    .sum::<DVec3>()
                    .try_normalize()
                    .unwrap_or(DVec3::Z);
                let id = mesh.vertices.len() as u32;
                mesh.vertices.push(point.to_array());
                mesh.normals.push(normal.to_array());
                for f in group {
                    done.insert(f);
                    let j = d.faces[f].iter().position(|&v| v as usize == i).unwrap();
                    corners[f][j] = id;
                }
            }
        }
        std::borrow::Cow::Owned(corners)
    };
    for (f, ids) in corners.iter().enumerate() {
        let [a, b, c] = [ids[0], ids[1], ids[2]];
        if ids.len() == 3 {
            mesh.triangles.push([a, b, c]);
            continue;
        }
        let fourth = ids[3];
        let [p, q, r, s] = [a, b, c, fourth].map(|v| DVec3::from(mesh.vertices[v as usize]));
        let n = face_normals[f];
        if (q - p).cross(r - p).dot(n) < 0. || (r - p).cross(s - p).dot(n) < 0. {
            mesh.triangles.extend([[a, b, fourth], [b, c, fourth]]);
        } else {
            mesh.triangles.extend([[a, b, c], [a, c, fourth]]);
        }
    }
    Ok(mesh)
}
