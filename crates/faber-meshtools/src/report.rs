use crate::topology::{self, Topology};
use faber_ports::{Mesh, Result};
use glam::DVec3;
use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
pub struct MeshReport {
    pub vertices: usize,
    pub triangles: usize,
    pub shells: usize,
    pub open_edges: usize,
    pub non_manifold_edges: usize,
    pub degenerate: usize,
    pub flipped: usize,
    pub duplicate: usize,
    pub watertight: bool,
    pub volume_mm3: Option<f64>,
    pub area_mm2: f64,
    pub bounds: [[f64; 3]; 2],
    pub problems: Vec<String>,
}
pub(crate) fn inspected(mesh: &Mesh) -> Result<MeshReport> {
    let graph = Topology::new(mesh);
    let mut problems = Vec::new();
    let flipped = graph.flip.iter().filter(|b| **b).count();
    for (n, label) in [
        (graph.open, "open edges"),
        (graph.non_manifold, "non-manifold edges"),
        (graph.split_vertices(mesh), "non-manifold vertices"),
        (graph.degenerate, "degenerate triangles"),
        (graph.duplicate, "duplicate triangles"),
        (flipped, "flipped triangles"),
        (
            graph.shells.iter().filter(|s| !s.orientable).count(),
            "shells with conflicting winding",
        ),
    ] {
        if n > 0 {
            problems.push(format!("{n} {label}"));
        }
    }
    if graph.shells.is_empty() {
        problems.push("The mesh has no surface.".into());
    }
    let watertight = problems.is_empty();
    let area = mesh
        .triangles
        .iter()
        .enumerate()
        .filter(|(i, _)| !graph.drop[*i])
        .map(|(_, &t)| topology::cross(mesh, t).length() * 0.5)
        .sum::<f64>();
    let volume = watertight.then(|| {
        graph
            .shells
            .iter()
            .map(|s| topology::volume(mesh, &s.faces))
            .sum::<f64>()
    });
    if !area.is_finite() || volume.is_some_and(|v| !v.is_finite()) {
        return Err("Mesh measurements exceed the numeric range.".into());
    }
    let bounds = if mesh.vertices.is_empty() {
        [[0.; 3]; 2]
    } else {
        let (low, high) = mesh
            .vertices
            .iter()
            .fold((DVec3::INFINITY, DVec3::NEG_INFINITY), |(l, h), &v| {
                (l.min(v.into()), h.max(v.into()))
            });
        [low.into(), high.into()]
    };
    Ok(MeshReport {
        vertices: mesh.vertices.len(),
        triangles: mesh.triangles.len(),
        shells: graph.shells.len(),
        open_edges: graph.open,
        non_manifold_edges: graph.non_manifold,
        degenerate: graph.degenerate,
        duplicate: graph.duplicate,
        flipped,
        watertight,
        volume_mm3: volume,
        area_mm2: area,
        bounds,
        problems,
    })
}
pub fn try_report(mesh: &Mesh) -> Result<MeshReport> {
    inspected(&topology::try_weld(mesh, topology::WELD_MM)?.mesh)
}
pub fn report(mesh: &Mesh) -> MeshReport {
    try_report(mesh).unwrap_or_else(|e| MeshReport {
        vertices: mesh.vertices.len(),
        triangles: mesh.triangles.len(),
        problems: vec![e],
        ..Default::default()
    })
}
