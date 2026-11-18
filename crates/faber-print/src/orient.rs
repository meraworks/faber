use crate::{
    mesh::{Surface, direction, extent, turn},
    support::view,
};
use faber_core::euler_from_matrix;
use faber_geometry::bvh::Bvh;
use faber_model::Printer;
use faber_ports::{Mesh, Result};
use glam::{DMat3, DQuat, DVec3};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default)]
pub struct Weights {
    pub strength_direction: Option<[f64; 3]>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Candidate {
    pub rotation_deg: [f64; 3],
    pub down: [f64; 3],
    pub size_mm: [f64; 3],
    pub score: f64,
    pub support_mm3: f64,
    pub overhang_mm2: f64,
    pub height_mm: f64,
    pub bed_contact_mm2: f64,
    pub fits: bool,
}

pub fn rank(mesh: &Mesh, printer: &Printer, weights: Weights) -> Result<Vec<Candidate>> {
    printer.validate()?;
    let strength = weights.strength_direction.map(direction).transpose()?;
    let s = Surface::new(&[mesh])?;
    let tree = Bvh::new(&s.mesh)?;
    let mut seen = vec![false; s.facets.len()];
    let mut planar = vec![];
    for i in 0..s.facets.len() {
        if seen[i] || s.facets[i].area == 0. {
            continue;
        }
        let n = s.facets[i].n;
        let group = s.region(i, &mut seen, |j| s.facets[j].n.dot(n) > 1. - 1e-9);
        let area = group.iter().map(|&j| s.facets[j].area).sum::<f64>();
        if area > s.area * 0.02 {
            planar.push((area, n));
        }
    }
    planar.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut directions = vec![
        -DVec3::Z,
        DVec3::Z,
        DVec3::X,
        -DVec3::X,
        DVec3::Y,
        -DVec3::Y,
    ];
    for (_, n) in planar {
        if directions.len() == 40 {
            break;
        }
        if directions
            .iter()
            .all(|d| d.dot(n) < 1_f64.to_radians().cos())
        {
            directions.push(n);
        }
    }
    let (lo, hi) = extent(&s.mesh.vertices, DQuat::IDENTITY);
    let longest = (hi - lo).max_element();
    let mut choices = vec![];
    for down in directions {
        let v = view(&s, &tree, -down, printer.overhang_deg)?;
        let mut rotation = turn(-down);
        let mut size = v.size;
        let fits = |size: [f64; 3]| (0..3).all(|a| size[a] <= printer.bed_mm[a] + s.eps);
        if !fits(size) {
            if fits([size[1], size[0], size[2]]) {
                size.swap(0, 1);
                rotation = DQuat::from_rotation_z(std::f64::consts::FRAC_PI_2) * rotation;
            } else if let Some(found) = crate::footprint::turn_to_fit(
                &s.mesh.vertices,
                rotation,
                size,
                printer.bed_mm,
                s.eps,
            ) {
                (rotation, size) = found;
            }
        }
        let score = v.support / s.volume + 0.5 * v.overhang / s.area + 0.2 * size[2] / longest
            - 0.3 * v.contact / s.area
            + strength.map_or(0., |axis| axis.dot(down).powi(2));
        choices.push(Candidate {
            down: down.into(),
            rotation_deg: euler_from_matrix(DMat3::from_quat(rotation)),
            size_mm: size,
            score,
            support_mm3: v.support,
            overhang_mm2: v.overhang,
            height_mm: size[2],
            bed_contact_mm2: v.contact,
            fits: fits(size),
        });
    }
    choices.sort_by(|a, b| b.fits.cmp(&a.fits).then(a.score.total_cmp(&b.score)));
    choices.truncate(5);
    Ok(choices)
}
