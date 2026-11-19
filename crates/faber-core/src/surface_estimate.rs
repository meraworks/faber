use super::check_size;
use faber_ports::{Mesh, Result};
use glam::DVec3;

pub fn estimate_curvature(mesh: &Mesh) -> Vec<[f64; 2]> {
    checked_estimate_curvature(mesh).unwrap_or_default()
}

pub fn checked_estimate_curvature(mesh: &Mesh) -> Result<Vec<[f64; 2]>> {
    check_size(mesh)?;
    let (points, remap) = mesh.welded_vertices(1e-8)?;
    let points: Vec<_> = points.into_iter().map(DVec3::from).collect();
    let mut normals = vec![DVec3::ZERO; points.len()];
    let mut neighbours = vec![Vec::<u32>::new(); points.len()];
    for indices in &mesh.triangles {
        let ids = indices.map(|i| remap[i as usize] as usize);
        let [a, b, c] = ids.map(|i| points[i]);
        let normal = (b - a).cross(c - a);
        if !normal.length_squared().is_finite() {
            return Err("A triangle exceeds the surface analysis coordinate limit.".into());
        }
        let Some(unit) = normal.try_normalize() else {
            continue;
        };
        for i in 0..3 {
            let centre = ids[i];
            let left = ids[(i + 1) % 3];
            let right = ids[(i + 2) % 3];
            let u = points[left] - points[centre];
            let v = points[right] - points[centre];
            let angle = normal.length().atan2(u.dot(v));
            normals[centre] += angle * unit;
            neighbours[centre].extend([left as u32, right as u32]);
        }
    }
    for ring in &mut neighbours {
        ring.sort_unstable();
        ring.dedup();
    }
    let mut samples = Vec::with_capacity(points.len());
    let mut patch = Vec::new();
    for (i, ring) in neighbours.iter().enumerate() {
        let Some(n) = normals[i].try_normalize() else {
            samples.push([0.; 2]);
            continue;
        };
        patch.clear();
        patch.extend_from_slice(ring);
        // A short ring has too few directions for a full curvature fit.
        if ring.len() < 8 {
            'more: for &j in ring {
                for &k in &neighbours[j as usize] {
                    if k != i as u32 && !patch.contains(&k) {
                        patch.push(k);
                    }
                    if patch.len() >= 32 {
                        break 'more;
                    }
                }
            }
        }
        samples.push(fit(points[i], n, &points, &patch));
    }
    Ok(remap
        .into_iter()
        .map(|i| samples.get(i as usize).copied().unwrap_or([0.; 2]))
        .collect())
}

// Fit the normal and its two principal curvatures in a local tangent frame.
// Chord length gives an exact normal section on a sphere, including at a pole.
fn fit(p: DVec3, n: DVec3, points: &[DVec3], ring: &[u32]) -> [f64; 2] {
    let (u, v) = n.any_orthonormal_pair();
    let h = ring
        .iter()
        .map(|&j| points[j as usize].distance(p))
        .fold(0., f64::max);
    if !h.is_finite() || h <= 0. {
        return [0.; 2];
    }
    let mut system = [[0.; 6]; 5];
    let mut total = 0.;
    let mut count = 0;
    let mut max_height: f64 = 0.;
    for &j in ring {
        let chord = (points[j as usize] - p) / h;
        let x = chord.dot(u);
        let y = chord.dot(v);
        let z = chord.dot(n);
        let tangent = x * x + y * y;
        if tangent <= f64::EPSILON {
            continue;
        }
        let length2 = chord.length_squared();
        let row = [
            2. * x,
            2. * y,
            length2,
            length2 * (x * x - y * y) / tangent,
            length2 * (2. * x * y) / tangent,
        ];
        let weight = 1. / length2;
        for (i, line) in system.iter_mut().enumerate() {
            for (entry, &r) in line[..5].iter_mut().zip(&row) {
                *entry += row[i] * r * weight;
            }
            line[5] += row[i] * (-2. * z) * weight;
        }
        total += -2. * z / length2;
        count += 1;
        max_height = max_height.max(z.abs());
    }
    if count == 0 || max_height <= 1e-12 {
        return [0.; 2];
    }
    if let Some([tilt_x, tilt_y, mean, cos2, sin2]) = solve(system) {
        let split = cos2.hypot(sin2);
        let scale = h * 1_f64.hypot(tilt_x.hypot(tilt_y));
        let pair = [(mean - split) / scale, (mean + split) / scale];
        if pair.iter().all(|v| v.is_finite()) {
            return pair;
        }
    }
    // A boundary vertex can have fewer than five independent samples.
    [total / (f64::from(count) * h); 2]
}

fn solve(mut system: [[f64; 6]; 5]) -> Option<[f64; 5]> {
    let scale = system
        .iter()
        .enumerate()
        .map(|(i, line)| line[i].abs())
        .fold(0., f64::max);
    for column in 0..5 {
        let pivot = (column..5)
            .max_by(|&i, &j| system[i][column].abs().total_cmp(&system[j][column].abs()))?;
        if system[pivot][column].abs() <= scale * 1e-12 {
            return None;
        }
        system.swap(column, pivot);
        let diag = system[column][column];
        for entry in &mut system[column][column..] {
            *entry /= diag;
        }
        let row = system[column];
        for (i, line) in system.iter_mut().enumerate() {
            if i == column {
                continue;
            }
            let factor = line[column];
            for (a, &b) in line[column..].iter_mut().zip(&row[column..]) {
                *a -= factor * b;
            }
        }
    }
    Some(system.map(|r| r[5]))
}
