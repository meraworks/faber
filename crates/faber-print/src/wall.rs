use crate::mesh::{Facet, geometry, ray_offset, signed_volume};
use faber_geometry::bvh::Bvh;
use faber_ports::Mesh;
use rayon::prelude::*;

/// Compare at the precision of the reported wall thickness.
pub fn is_thin_wall(wall: f32, minimum_mm: f64) -> bool {
    wall.is_finite() && wall > 0. && wall < minimum_mm as f32
}

pub(crate) fn measure(f: &Facet, bvh: &Bvh, eps: f64) -> f32 {
    if f.area == 0. {
        return 0.;
    }
    bvh.first_hit((f.centre - f.n * eps).into(), (-f.n).into())
        .map_or(0., |hit| (hit.t + eps) as f32)
}

/// Distance to the opposite surface along each inward triangle normal.
pub fn wall_thickness(mesh: &Mesh, bvh: &Bvh) -> Vec<f32> {
    let reverse = signed_volume(mesh) < 0.;
    let eps = ray_offset(mesh);
    mesh.triangles
        .par_iter()
        .with_min_len(1024)
        .map(|&t| {
            let mut facet = geometry(mesh, t);
            if reverse {
                facet.n = -facet.n
            }
            measure(&facet, bvh, eps)
        })
        .collect()
}
