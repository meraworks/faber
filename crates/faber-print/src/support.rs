use crate::mesh::{Facet, Surface, direction, extent, turn};
use faber_core::overhang::{self, Facing};
use faber_geometry::bvh::Bvh;
use faber_ports::{Mesh, Result};
use glam::DVec3;
use rayon::prelude::*;
use std::collections::BTreeMap;

pub(crate) struct View {
    pub size: [f64; 3],
    pub bed: f64,
    pub contact: f64,
    pub overhang: f64,
    pub support: f64,
    pub facings: Vec<Facing>,
}

fn column(f: &Facet, bvh: &Bvh, up: DVec3, bed: f64, eps: f64) -> f64 {
    let height = (f.centre.dot(up) - bed).max(0.);
    let distance = bvh
        .first_hit((f.centre - up * eps).into(), (-up).into())
        .map_or(height, |h| height.min(h.t + eps));
    f.area * (-f.n.dot(up)).max(0.) * distance
}

pub(crate) fn view(s: &Surface, bvh: &Bvh, up: DVec3, angle: f64) -> Result<View> {
    let (lo, hi) = extent(&s.mesh.vertices, turn(up));
    let facings = overhang::classify(&s.mesh, up.into(), 90. - angle)?;
    let mut contact = 0.;
    let mut overhang = 0.;
    for (f, kind) in s.facets.iter().zip(&facings) {
        match kind {
            Facing::Bed => contact += f.area,
            Facing::Support { .. } => overhang += f.area,
            _ => (),
        }
    }
    let support = s
        .facets
        .par_iter()
        .zip(&facings)
        .with_min_len(1024)
        .map(|(f, kind)| {
            if matches!(kind, Facing::Support { .. }) {
                column(f, bvh, up, lo.z, s.eps)
            } else {
                0.
            }
        })
        .sum();
    Ok(View {
        size: (hi - lo).into(),
        bed: lo.z,
        contact,
        overhang,
        support,
        facings,
    })
}

pub fn support_volume(mesh: &Mesh, bvh: &Bvh, up: [f64; 3], overhang_deg: f64) -> f64 {
    direction(up)
        .and_then(|u| Surface::new(&[mesh]).and_then(|s| view(&s, bvh, u, overhang_deg)))
        .map_or(0., |v| v.support)
}

fn root(parents: &mut [usize], mut i: usize) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i]
    }
    i
}
pub(crate) fn low_points(s: &Surface, up: DVec3, bed: f64) -> Vec<[f64; 3]> {
    let verts = &s.mesh.vertices;
    let heights: Vec<_> = verts.iter().map(|&p| DVec3::from(p).dot(up)).collect();
    let mut parent: Vec<_> = (0..verts.len()).collect();
    let mut lower = vec![false; verts.len()];
    for t in &s.mesh.triangles {
        for j in 0..3 {
            let (a, b) = (t[j] as usize, t[(j + 1) % 3] as usize);
            let dz = heights[a] - heights[b];
            if dz.abs() <= s.eps {
                let i = root(&mut parent, a);
                let k = root(&mut parent, b);
                parent[k] = i;
            } else {
                lower[if dz > 0. { a } else { b }] = true;
            }
        }
    }
    let mut groups = BTreeMap::new();
    for (i, &v) in verts.iter().enumerate() {
        let idx = root(&mut parent, i);
        let data = groups.entry(idx).or_insert((DVec3::ZERO, 0usize, true));
        data.0 += DVec3::from(v);
        data.1 += 1;
        data.2 &= !lower[i] && heights[i] > bed + 0.05;
    }
    groups
        .into_values()
        .filter(|v| v.2)
        .map(|(sum, n, _)| (sum / n as f64).into())
        .collect()
}

/// Local low points above the bed. Equal-height connected vertices form one plateau.
pub fn islands(mesh: &Mesh, up: [f64; 3], bed: f64) -> Vec<[f64; 3]> {
    direction(up)
        .and_then(|u| Surface::new(&[mesh]).map(|s| low_points(&s, u, bed)))
        .unwrap_or_default()
}
