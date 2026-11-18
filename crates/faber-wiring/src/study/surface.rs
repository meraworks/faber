use faber_ports::Mesh;
use glam::DVec3;
use std::collections::HashMap;

/// Divide long edges on the existing surface. Keep the original vertex indices.
pub(super) fn refine(
    source: &Mesh,
    faces: &[u32],
    cell_size: f64,
    limit: usize,
) -> Option<(Mesh, Vec<u32>)> {
    if source.triangles.len() >= limit
        || source.vertices.len() > 3 * limit
        || (!faces.is_empty() && faces.len() != source.triangles.len())
        || (!source.normals.is_empty() && source.normals.len() != source.vertices.len())
    {
        return None;
    }
    let edge_squared = |mesh: &Mesh, a: u32, b: u32| {
        DVec3::from(mesh.vertices[a as usize]).distance_squared(mesh.vertices[b as usize].into())
    };
    let target = cell_size.powi(2);
    let mut detail: Option<(Mesh, Vec<u32>)> = None;
    loop {
        let mesh = detail.as_ref().map_or(source, |(mesh, _)| mesh);
        let available = limit - mesh.triangles.len();
        if available == 0 {
            break;
        }
        let mut lengths = Vec::new();
        for &[a, b, c] in &mesh.triangles {
            for (start, end) in [(a, b), (b, c), (c, a)] {
                let length = edge_squared(mesh, start, end);
                if length > target {
                    lengths.push(length);
                }
            }
        }
        if lengths.is_empty() {
            break;
        }
        // Each divided triangle edge adds one triangle. Keep both sides of a common edge.
        let threshold = if lengths.len() > available {
            let (_, first_excluded, _) =
                lengths.select_nth_unstable_by(available, |a, b| b.total_cmp(a));
            *first_excluded
        } else {
            target
        };
        let additions = lengths.iter().filter(|&&length| length > threshold).count();
        if additions == 0 {
            break;
        }
        let (mesh, face_ids) = detail.get_or_insert_with(|| (source.clone(), faces.to_vec()));
        let previous = std::mem::take(&mut mesh.triangles);
        let previous_faces = std::mem::take(face_ids);
        mesh.triangles.reserve(previous.len() + additions);
        if !previous_faces.is_empty() {
            face_ids.reserve(previous.len() + additions);
        }
        let mut midpoints = HashMap::new();
        for (index, [a, b, c]) in previous.into_iter().enumerate() {
            let [ab, bc, ca] = [(a, b), (b, c), (c, a)].map(|(i, j)| {
                if edge_squared(mesh, i, j) <= threshold {
                    return None;
                }
                let key = (i.min(j), i.max(j));
                Some(*midpoints.entry(key).or_insert_with(|| {
                    let position = (DVec3::from(mesh.vertices[i as usize])
                        + DVec3::from(mesh.vertices[j as usize]))
                        / 2.;
                    let id = mesh.vertices.len() as u32;
                    mesh.vertices.push(position.into());
                    if !mesh.normals.is_empty() {
                        let normal = DVec3::from(mesh.normals[i as usize])
                            + DVec3::from(mesh.normals[j as usize]);
                        mesh.normals.push(normal.normalize_or_zero().into());
                    }
                    id
                }))
            });
            let start = mesh.triangles.len();
            match (ab, bc, ca) {
                (None, None, None) => mesh.triangles.push([a, b, c]),
                (Some(m), None, None) => mesh.triangles.extend([[a, m, c], [m, b, c]]),
                (None, Some(m), None) => mesh.triangles.extend([[b, m, a], [m, c, a]]),
                (None, None, Some(m)) => mesh.triangles.extend([[c, m, b], [m, a, b]]),
                (Some(m), Some(n), None) => two_edges(mesh, [a, b, c], m, n),
                (None, Some(m), Some(n)) => two_edges(mesh, [b, c, a], m, n),
                (Some(n), None, Some(m)) => two_edges(mesh, [c, a, b], m, n),
                (Some(m), Some(n), Some(p)) => {
                    mesh.triangles
                        .extend([[a, m, p], [m, b, n], [p, n, c], [m, n, p]])
                }
            }
            if let Some(&id) = previous_faces.get(index) {
                face_ids.extend(std::iter::repeat_n(id, mesh.triangles.len() - start));
            }
        }
    }
    detail
}

fn two_edges(mesh: &mut Mesh, [a, b, c]: [u32; 3], m: u32, n: u32) {
    let squared = |i: u32, j: u32| {
        DVec3::from(mesh.vertices[i as usize]).distance_squared(mesh.vertices[j as usize].into())
    };
    let quad = if squared(m, c) <= squared(a, n) {
        [[a, m, c], [m, n, c]]
    } else {
        [[a, m, n], [a, n, c]]
    };
    mesh.triangles.push([m, b, n]);
    mesh.triangles.extend(quad);
}
