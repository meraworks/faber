use crate::{
    mesh::{Surface, direction},
    support::{low_points, view},
    wall,
};
use faber_core::overhang::Facing;
use faber_geometry::bvh::Bvh;
use faber_model::Printer;
use faber_ports::{Mesh, Result};
use glam::DVec3;
use rayon::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    TooLarge,
    ThinWall,
    Overhang,
    Island,
    SmallContact,
    LongBridge,
    TallAndNarrow,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub kind: Kind,
    pub severity: Severity,
    pub message: String,
    pub at: [f64; 3],
    pub value: f64,
    pub limit: f64,
    pub area_mm2: f64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Resolution {
    pub triangles: usize,
    pub wall_samples_per_triangle: u8,
}
#[derive(Debug, Clone, Serialize)]
pub struct PrintReport {
    pub method: &'static str,
    pub resolution: Resolution,
    pub fits: bool,
    pub size_mm: [f64; 3],
    pub height_mm: f64,
    pub layers: u32,
    pub bed_contact_mm2: f64,
    pub overhang_mm2: f64,
    pub support_mm3: f64,
    pub thin_wall_mm2: f64,
    pub min_wall_mm: f64,
    pub islands: u32,
    pub findings: Vec<Finding>,
    pub limits: Vec<String>,
}

fn finding(
    kind: Kind,
    message: String,
    at: [f64; 3],
    value: f64,
    limit: f64,
    area_mm2: f64,
) -> Finding {
    Finding {
        kind,
        severity: if kind == Kind::TooLarge {
            Severity::Error
        } else {
            Severity::Warning
        },
        message,
        at,
        value,
        limit,
        area_mm2,
    }
}
fn centre(s: &Surface, ids: &[usize]) -> (f64, [f64; 3]) {
    let area = ids.iter().map(|&i| s.facets[i].area).sum::<f64>();
    let pos = ids
        .iter()
        .map(|&i| s.facets[i].centre * (s.facets[i].area / area))
        .sum::<DVec3>();
    (area, pos.into())
}

/// Span between edges with material below them on opposite sides of a region.
fn bridge(s: &Surface, ids: &[usize], up: DVec3) -> f64 {
    let mut anchors = Vec::new();
    for &i in ids {
        let f = &s.facets[i];
        if -f.n.dot(up) < 0.98 {
            continue;
        }
        let tri = s.mesh.triangles[i];
        for (j, &neighbour) in s.adjacent[i].iter().enumerate() {
            let a = DVec3::from(s.mesh.vertices[tri[j] as usize]);
            let b = DVec3::from(s.mesh.vertices[tri[(j + 1) % 3] as usize]);
            let middle = (a + b) * 0.5;
            if s.facets[neighbour].centre.dot(up) >= middle.dot(up) - s.eps {
                continue;
            }
            let n = (b - a).cross(up).normalize_or_zero();
            let toward = if n.dot(f.centre - middle) > 0. { n } else { -n };
            anchors.push((middle, toward));
        }
    }
    // Bin anchor directions. This keeps the work linear on finely tessellated edges.
    let x = up.any_orthonormal_vector();
    let y = up.cross(x);
    let mut lo = [f64::INFINITY; 32];
    let mut hi = [f64::NEG_INFINITY; 32];
    for (p, inward) in anchors {
        let i = (inward.dot(y).atan2(inward.dot(x)) / std::f64::consts::TAU * 32.).round() as i32;
        let i = i.rem_euclid(32) as usize;
        let (sn, cs) = (std::f64::consts::TAU * i as f64 / 32.).sin_cos();
        let projected = p.dot(cs * x + sn * y);
        lo[i] = lo[i].min(projected);
        hi[(i + 16) % 32] = hi[(i + 16) % 32].max(-projected);
    }
    (0..32).map(|i| hi[i] - lo[i]).fold(0., f64::max)
}

pub fn check(meshes: &[&Mesh], printer: &Printer, up: [f64; 3]) -> Result<PrintReport> {
    check_with_walls(meshes, printer, up).map(|(r, _)| r)
}

pub fn check_with_walls(
    meshes: &[&Mesh],
    printer: &Printer,
    up: [f64; 3],
) -> Result<(PrintReport, Vec<Vec<f32>>)> {
    printer.validate()?;
    let up = direction(up)?;
    let surface = Surface::new(meshes)?;
    let assembly_tree = Bvh::new(&surface.mesh)?;
    let metrics = view(&surface, &assembly_tree, up, printer.overhang_deg)?;
    let size = metrics.size;
    let fits = (0..3).all(|a| size[a] <= printer.bed_mm[a] + surface.eps);
    let mut findings = vec![];
    let anchor = surface
        .mesh
        .vertices
        .iter()
        .min_by(|a, b| {
            DVec3::from(**a)
                .dot(up)
                .total_cmp(&DVec3::from(**b).dot(up))
        })
        .copied()
        .unwrap_or_default();
    for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
        if size[i] > printer.bed_mm[i] + surface.eps {
            findings.push(finding(
                Kind::TooLarge,
                format!(
                    "Size {} mm along {axis} is larger than the bed {} mm.",
                    size[i], printer.bed_mm[i]
                ),
                anchor,
                size[i],
                printer.bed_mm[i],
                0.,
            ));
        }
    }
    let mut values = vec![];
    let mut thin = vec![false; surface.facets.len()];
    let mut min_wall = f64::INFINITY;
    for (range, mesh) in surface.parts.iter().zip(meshes) {
        let local_tree = (meshes.len() > 1).then(|| Bvh::new(mesh)).transpose()?;
        let tree = local_tree.as_ref().unwrap_or(&assembly_tree);
        let walls: Vec<_> = surface.facets[range.clone()]
            .par_iter()
            .with_min_len(1024)
            .map(|f| wall::measure(f, tree, surface.eps))
            .collect();
        for (id, &w) in range.clone().zip(&walls) {
            if surface.facets[id].area == 0. {
                continue;
            }
            if !w.is_finite() || w <= 0. {
                return Err(format!(
                    "Cannot measure the wall at triangle {id}. Repair the mesh first."
                ));
            }
            min_wall = min_wall.min(f64::from(w));
            thin[id] = wall::is_thin_wall(w, printer.min_wall_mm);
        }
        values.push(walls);
    }
    let wall_at = |i: usize| {
        let part = surface.parts.partition_point(|r| r.end <= i);
        f64::from(values[part][i - surface.parts[part].start])
    };
    let mut seen = vec![false; thin.len()];
    let mut thin_area = 0.;
    for i in 0..thin.len() {
        if thin[i] && !seen[i] {
            let group = surface.region(i, &mut seen, |j| thin[j]);
            let (area, at) = centre(&surface, &group);
            thin_area += area;
            let value = group
                .iter()
                .map(|&j| wall_at(j))
                .fold(f64::INFINITY, f64::min);
            findings.push(finding(
                Kind::ThinWall,
                format!("Wall thickness is {value:.2} mm."),
                at,
                value,
                printer.min_wall_mm,
                area,
            ));
        }
    }
    seen.fill(false);
    for i in 0..seen.len() {
        if matches!(metrics.facings[i], Facing::Support { .. }) && !seen[i] {
            let group = surface.region(i, &mut seen, |j| {
                matches!(metrics.facings[j], Facing::Support { .. })
            });
            let (area, at) = centre(&surface, &group);
            let angle = group
                .iter()
                .filter_map(|&j| {
                    if let Facing::Support { angle_deg, .. } = metrics.facings[j] {
                        Some(90. - angle_deg)
                    } else {
                        None
                    }
                })
                .fold(0., f64::max);
            findings.push(finding(
                Kind::Overhang,
                format!("Overhang covers {area:.2} mm²."),
                at,
                angle,
                printer.overhang_deg,
                area,
            ));
            let span = bridge(&surface, &group, up);
            if span > printer.bridge_mm {
                findings.push(finding(
                    Kind::LongBridge,
                    format!("Bridge span is about {span:.2} mm."),
                    at,
                    span,
                    printer.bridge_mm,
                    area,
                ));
            }
        }
    }
    let low = low_points(&surface, up, metrics.bed);
    for &at in &low {
        findings.push(finding(
            Kind::Island,
            "This region starts above the bed.".into(),
            at,
            DVec3::from(at).dot(up) - metrics.bed,
            0.,
            0.,
        ));
    }
    if metrics.contact < 25. {
        findings.push(finding(
            Kind::SmallContact,
            format!("Bed contact is {:.2} mm².", metrics.contact),
            anchor,
            metrics.contact,
            25.,
            metrics.contact,
        ));
    }
    if metrics.contact > 0. {
        let ratio = size[2] / metrics.contact.sqrt();
        if ratio > 8. {
            findings.push(finding(
                Kind::TallAndNarrow,
                format!("Height to contact width ratio is {ratio:.2}."),
                anchor,
                ratio,
                8.,
                metrics.contact,
            ));
        }
    }
    let layers = (size[2] / printer.layer_mm).ceil();
    if !layers.is_finite() || layers > f64::from(u32::MAX) {
        return Err(format!(
            "The print has {layers} layers. The limit is {}. Use a larger layer height.",
            u32::MAX
        ));
    }
    let report = PrintReport {
        method: "Placed triangle surfaces, inward wall rays and projected support columns.",
        resolution: Resolution {
            triangles: surface.facets.len(),
            wall_samples_per_triangle: 1,
        },
        fits,
        size_mm: size,
        height_mm: size[2],
        layers: layers as u32,
        bed_contact_mm2: metrics.contact,
        overhang_mm2: metrics.overhang,
        support_mm3: metrics.support,
        thin_wall_mm2: thin_area,
        min_wall_mm: min_wall,
        islands: low.len() as u32,
        findings,
        limits: vec![
            "Support volume is an estimate. Check supports in the slicer.".into(),
            "Wall thickness uses one inward ray per triangle. Small features depend on the mesh resolution.".into(),
        ],
    };
    Ok((report, values))
}
