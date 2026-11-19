mod frame;
pub use faber_cam::{HeightMap, Move, StockBox, Toolpath, Verification, stock_mesh};
use faber_geometry::bvh::Bvh;
use faber_kernel_occt::OcctKernel;
pub use faber_model::machining::Tool;
use faber_model::{
    ProjectData,
    machining::{Holes, Operation, OperationKind, Selection},
};
use faber_ports::{
    FaceInfo, Kernel, Mesh, Result, Shape,
    program::{Dialect, Program, ProgramExport, Section},
};
use glam::DAffine3;
use std::{collections::BTreeSet, path::Path};

#[derive(Debug, serde::Serialize)]
pub struct OperationReport {
    pub operation: String,
    pub tool: String,
    pub tool_number: u32,
    pub cut_length_mm: f64,
    pub rapid_length_mm: f64,
    pub minutes: f64,
    pub gouge_mm: f64,
    pub max_excess_mm: f64,
    pub removed_mm3: f64,
    pub rapid_collisions: usize,
    pub warnings: Vec<String>,
}
pub struct Run {
    pub part: String,
    pub world_from_machine: DAffine3,
    pub mesh: Mesh,
    pub stock: StockBox,
    pub paths: Vec<(Toolpath, Tool)>,
    pub target: HeightMap,
    pub verification: Verification,
    pub reports: Vec<OperationReport>,
    pub limits: Vec<String>,
}
const TOL: f64 = 0.02;
fn level(s: &Selection, shape: Option<&Shape>, faces: &[FaceInfo]) -> Result<(f64, Option<u32>)> {
    match s {
        Selection::Z(z) => Ok((*z, None)),
        Selection::Face(spec) => {
            let id = faber_core::pick_selector::single_face(
                &OcctKernel,
                shape.ok_or("Select a numeric height on a mesh part.")?,
                spec,
            )?;
            let face = faces
                .iter()
                .find(|f| f.index == id)
                .ok_or("The selected face is absent.")?;
            if !face.normal.is_some_and(|n| n[2].abs() > 1. - 1e-8) {
                return Err("Select a planar face parallel to the setup XY plane.".into());
            }
            Ok((face.centre[2], Some(id)))
        }
    }
}

struct Source<'a> {
    mesh: &'a Mesh,
    labels: &'a [u32],
    shape: Option<&'a Shape>,
    faces: &'a [FaceInfo],
    frame: &'a frame::Frame,
    target: &'a HeightMap,
}
impl Source<'_> {
    fn generate(&self, tool: &Tool, original: &Operation) -> Result<Toolpath> {
        let mut op = original.clone();
        let stock = &self.frame.stock;
        let mut selected = None;
        let bottom = match &mut op.kind {
            OperationKind::Contour { z, .. } | OperationKind::Pocket { floor: z, .. } => {
                let (height, index) = level(z, self.shape, self.faces)?;
                if let Some(index) = index {
                    let mut piece = Mesh::default();
                    for (tri, label) in self.mesh.triangles.iter().zip(self.labels) {
                        if *label == index {
                            let i = piece.vertices.len() as u32;
                            piece
                                .vertices
                                .extend(tri.map(|v| self.mesh.vertices[v as usize]));
                            piece.triangles.push([i, i + 1, i + 2]);
                        }
                    }
                    if piece.triangles.is_empty() {
                        return Err("The selected face has no triangles.".into());
                    }
                    selected = Some(piece);
                }
                *z = Selection::Z(height);
                Some(height)
            }
            OperationKind::Face { .. } => Some(self.frame.part_top),
            OperationKind::Cut2d { .. } => Some(0.),
            OperationKind::Drill {
                holes: Holes::Auto, ..
            } => None,
            _ => Some(stock.min[2]),
        };
        op.heights.bottom = op.heights.bottom.or(bottom);
        if let Some(z) = op.heights.bottom {
            faber_cam::validate_stock_depth(stock, z)?;
        }
        if op.heights.clearance < stock.max[2] - 1e-8 {
            return Err("The clearance height must clear the stock top.".into());
        }
        let path = match &op.kind {
            OperationKind::Face { .. } => faber_cam::face(stock, tool, &op),
            OperationKind::Contour { .. } => match &selected {
                Some(piece) => faber_cam::contour_on_face(self.mesh, tool, &op, piece),
                None => faber_cam::contour(self.mesh, tool, &op),
            },
            OperationKind::Pocket { .. } => match &selected {
                Some(piece) => faber_cam::pocket_on_floor(self.mesh, stock, tool, &op, piece),
                None => faber_cam::pocket(self.mesh, stock, tool, &op),
            },
            OperationKind::Drill { holes, .. } => {
                let points = match holes {
                    Holes::Auto => {
                        if self.shape.is_none() {
                            return Err("Give the hole centres for a mesh part.".into());
                        }
                        faber_cam::find_holes(self.faces, [0., 0., 1.])
                    }
                    Holes::List(xy) => xy
                        .iter()
                        .map(|p| faber_cam::Hole {
                            centre: *p,
                            diameter: tool.diameter,
                            top: op.heights.top,
                            bottom: op.heights.bottom.unwrap_or(stock.min[2]),
                        })
                        .collect(),
                };
                faber_cam::drill(&points, tool, &op)
            }
            OperationKind::Cut2d { .. } => faber_cam::cut2d(self.mesh, tool, &op),
            OperationKind::Rough3d { .. } => {
                faber_cam::three_d::rough3d_on_mesh(self.mesh, self.target, stock, tool, &op)
            }
            OperationKind::Parallel3d { .. } => faber_cam::parallel3d(self.target, tool, &op),
        }?;
        faber_cam::validate_stock_depth(stock, path.bounds()[0][2])?;
        Ok(path)
    }
}

pub fn calculate(
    root: &Path,
    data: &ProjectData,
    id: &str,
    operations: Option<&[String]>,
) -> Result<Run> {
    calculate_at(root, data, id, operations, None)
}

pub fn cached(
    root: &Path,
    p: &faber_core::Project,
    id: &str,
    operations: Option<&[String]>,
    resolution_mm: Option<f64>,
) -> Result<std::sync::Arc<Run>> {
    let build =
        || calculate_at(root, &p.data, id, operations, resolution_mm).map(std::sync::Arc::new);
    if operations.is_none() && resolution_mm.is_none() {
        p.cached("machining", id, build)
    } else {
        build()
    }
}

fn calculate_at(
    root: &Path,
    data: &ProjectData,
    id: &str,
    operations: Option<&[String]>,
    resolution_mm: Option<f64>,
) -> Result<Run> {
    faber_core::machining::validate(data)?;
    let m = &data.machining;
    let setup = m
        .setups
        .get(id)
        .ok_or_else(|| format!("Unknown setup: {id}."))?;
    let requested = operations.map(|items| items.iter().collect::<BTreeSet<_>>());
    let ops = m
        .operations
        .iter()
        .filter(|op| op.setup == id && requested.as_ref().is_none_or(|ids| ids.contains(&op.id)))
        .collect::<Vec<_>>();
    if operations.is_some_and(|items| items.len() != ops.len()) {
        return Err("Select each operation once from this setup.".into());
    }
    if ops.is_empty() {
        return Err(format!("Setup {id} needs an operation."));
    }
    let (surface, shape) = faber_core::local_surface(
        data,
        &setup.part,
        &OcctKernel,
        Some(&faber_files::FileAssets::new(root)),
        Some(&faber_files::FileFeatureCache::read_only(root)),
    )?;
    let mut mesh = surface.mesh;
    let frame = frame::Frame::new(&mut mesh, setup)?;
    let mut faces = shape
        .as_ref()
        .map(|s| OcctKernel.faces(s))
        .transpose()?
        .unwrap_or_default();
    frame.transform_faces(&mut faces);
    let cutter = |op: &Operation| {
        let mut tool = m.tools[&op.tool].clone();
        if let OperationKind::Cut2d { kerf, .. } = op.kind {
            tool.diameter = kerf;
            tool.plunge_mm_min = tool.feed_mm_min;
        }
        tool
    };
    let default_cell = ops
        .iter()
        .map(|op| cutter(op).diameter / 10.)
        .fold(0.25, f64::min);
    let cell = resolution_mm.unwrap_or(default_cell);
    if !(cell.is_finite() && cell > 0. && cell <= default_cell) {
        return Err(format!(
            "Set a grid size above zero and at most {default_cell} mm."
        ));
    }
    let bvh = Bvh::new(&mesh)?;
    let bores = faber_cam::find_holes(&faces, [0., 0., 1.]);
    let target =
        faber_cam::height_map::verification_target(&mesh, &bvh, &frame.stock, cell, &bores)?;
    let envelope = ops
        .iter()
        .any(|op| {
            matches!(op.kind, OperationKind::Parallel3d { .. })
                || (matches!(op.kind, OperationKind::Rough3d { .. })
                    && m.tools[&op.tool].kind != faber_model::machining::ToolKind::Flat)
        })
        .then(|| faber_cam::stock_height_map(&mesh, &bvh, &frame.stock, cell))
        .transpose()?;
    let source = Source {
        mesh: &mesh,
        labels: &surface.face_of_triangle,
        shape: shape.as_ref(),
        faces: &faces,
        frame: &frame,
        target: envelope.as_ref().unwrap_or(&target),
    };
    let mut paths = Vec::with_capacity(ops.len());
    let mut move_count = 0;
    for op in &ops {
        let tool = cutter(op);
        let path = source
            .generate(&tool, op)
            .map_err(|e| format!("Operation {}: {e}", op.id))?;
        move_count += path.moves.len();
        if move_count > 4_000_000 {
            return Err("The setup exceeds four million moves. Select fewer operations.".into());
        }
        paths.push((path, tool));
    }
    let (verification, stats) = faber_cam::verify_operations(&target, &frame.stock, &paths, TOL)?;
    let reports = stats
        .into_iter()
        .zip(&paths)
        .map(|(s, (p, t))| OperationReport {
            operation: s.operation,
            tool: s.tool,
            tool_number: t.number,
            cut_length_mm: p.cut_length(),
            rapid_length_mm: p.rapid_length(),
            minutes: s.minutes,
            gouge_mm: s.gouge_mm,
            max_excess_mm: s.max_excess_mm,
            rapid_collisions: s.rapid_collision_count,
            removed_mm3: s.removed_mm3,
            warnings: s.warnings,
        })
        .collect();
    let mut limits = vec![
        "The grid samples the top surface along the setup Z axis.".into(),
        "Verification excludes the holder, fixtures and machine travel.".into(),
    ];
    if ops.iter().any(|op| {
        matches!(
            op.kind,
            OperationKind::Rough3d { .. } | OperationKind::Parallel3d { .. }
        )
    }) {
        let hidden = faber_cam::hidden_area(&mesh, &bvh, [0., 0., 1.]);
        if hidden > 0.01 {
            limits.push(format!(
                "{hidden:.2} mm2 of the surface is not reachable from this direction."
            ));
        }
    }
    Ok(Run {
        part: setup.part.clone(),
        world_from_machine: faber_core::world_placement(data, &setup.part)?
            * frame.local_from_machine,
        stock: frame.stock,
        mesh,
        paths,
        target,
        verification,
        reports,
        limits,
    })
}
impl Run {
    pub fn summary(&self) -> serde_json::Value {
        serde_json::json!({"method":"Three-axis tool sweep on a height grid","resolution":{"cell_mm":self.target.cell,"cells":self.target.n,"tolerance_mm":TOL},"limits":self.limits,"operations":self.reports,
        "gouge_mm":self.verification.gouge_mm,"gouge_at":self.verification.gouge_at,"rapid_collisions":self.verification.rapid_collision_count,"minutes":self.verification.minutes})
    }
}
pub fn write_program(
    run: &Run,
    data: &ProjectData,
    path: &Path,
    dialect: Dialect,
    force: bool,
) -> Result<()> {
    let mut warnings = vec![];
    for r in &run.reports {
        if r.gouge_mm > TOL {
            warnings.push(format!(
                "Operation {} makes a gouge {:.3} mm deep.",
                r.operation, r.gouge_mm
            ));
        }
        if r.rapid_collisions > 0 {
            warnings.push(format!(
                "Operation {} has {} rapid moves through material.",
                r.operation, r.rapid_collisions
            ));
        }
    }
    if !warnings.is_empty() && !force {
        return Err(warnings.join(" "));
    }
    let mut sections = vec![];
    for (path, tool) in &run.paths {
        let op = data
            .machining
            .operations
            .iter()
            .find(|op| op.id == path.operation)
            .ok_or("The operation has changed.")?;
        let power = match op.kind {
            OperationKind::Cut2d { power, .. } => Some(power),
            _ => None,
        };
        sections.push(Section {
            tool_number: tool.number,
            speed: power.unwrap_or(tool.spindle_rpm),
            coolant: false,
            laser: power.is_some(),
            moves: path.moves.clone(),
        });
    }
    faber_files::gcode::Writer.write(
        &Program {
            name: data.name.clone(),
            sections,
            warnings,
        },
        dialect,
        path,
    )
}

/// Refuse output from an older saved part after the source or stock changed.
pub fn check_program_source(root: &Path, data: &ProjectData) -> Result<()> {
    if &faber_files::read_committed(root)? != data {
        return Err(
            "The project changed during verification. Calculate the toolpaths again.".into(),
        );
    }
    if crate::design_source().has_source(root) && faber_build::current_components(root).is_none() {
        let current = faber_build::evaluate(root, data)?.data;
        if current.parts != data.parts
            || current.params != data.params
            || current.components != data.components
        {
            return Err(
                "The design source changed. Build the project before writing a program.".into(),
            );
        }
    }
    Ok(())
}
