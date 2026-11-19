//! Draw the target, initial stock and verified stock in the placed setup frame.
use crate::machining::{Move, Run, stock_mesh};
use faber_ports::{Mesh, Result};
use faber_render::machining::{CUT, GOUGE, Path as DisplayPath, Scene, Segment};
use glam::{DAffine3, DVec3};
use std::path::Path;

fn placed(mut mesh: Mesh, frame: DAffine3) -> Mesh {
    for p in &mut mesh.vertices {
        *p = frame.transform_point3((*p).into()).into();
    }
    for n in &mut mesh.normals {
        *n = frame
            .transform_vector3((*n).into())
            .normalize_or_zero()
            .into();
    }
    mesh
}

fn stock_box(lo: [f64; 3], hi: [f64; 3]) -> Mesh {
    let vertices = (0..8)
        .map(|i| std::array::from_fn(|j| if i & (1 << j) == 0 { lo[j] } else { hi[j] }))
        .collect();
    // The indices use x + 2y + 4z.
    let faces = [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ];
    Mesh {
        vertices,
        triangles: faces
            .into_iter()
            .flat_map(|[a, b, c, d]| [[a, b, c], [a, c, d]])
            .collect(),
        normals: vec![],
    }
}

/// Keep small gouges visible when the stock mesh uses fewer cells than the verification.
fn cut_colours(run: &Run, cut: &Mesh) -> Vec<[u8; 3]> {
    let stock = &run.verification.stock;
    let target = &run.target;
    let edge = |a: [f64; 3], b: [f64; 3], px: f64, py: f64| {
        (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])
    };
    let mut colours = vec![CUT; cut.triangles.len()];
    if run.verification.gouge_mm <= 0.02 {
        return colours;
    }
    for (i, indices) in cut.triangles.iter().enumerate() {
        let [a, b, c] = indices.map(|j| cut.vertices[j as usize]);
        if edge(a, b, c[0], c[1]) <= 1e-12 {
            continue;
        }
        let range = |axis: usize| {
            let min = a[axis].min(b[axis]).min(c[axis]);
            let max = a[axis].max(b[axis]).max(c[axis]);
            let start = ((min - stock.origin[axis]) / stock.cell - 0.5 - 1e-9)
                .ceil()
                .max(0.) as usize;
            let end = (((max - stock.origin[axis]) / stock.cell - 0.5 + 1e-9).floor() + 1.).max(0.)
                as usize;
            start.min(stock.n[axis])..end.min(stock.n[axis])
        };
        let xs = range(0);
        if range(1).any(|y| {
            xs.clone().any(|x| {
                let j = x + y * stock.n[0];
                let px = stock.origin[0] + (x as f64 + 0.5) * stock.cell;
                let py = stock.origin[1] + (y as f64 + 0.5) * stock.cell;
                target.z[j] - stock.z[j] > 0.02
                    && edge(a, b, px, py) >= -1e-8
                    && edge(b, c, px, py) >= -1e-8
                    && edge(c, a, px, py) >= -1e-8
            })
        }) {
            colours[i] = GOUGE;
        }
    }
    colours
}
/// A through gouge removes the stock surface itself. Mark its footprint on the stock floor.
fn mark_through_cuts(run: &Run, cut: &mut Mesh, colours: &mut Vec<[u8; 3]>) {
    if run.verification.gouge_mm <= 0.02 {
        return;
    }
    let grid = &run.verification.stock;
    let [nx, ny] = grid.n;
    let stride = nx.max(ny).div_ceil(256).max(1);
    let w = nx.div_ceil(stride);
    let mut removed = vec![false; w * ny.div_ceil(stride)];
    let floor = run.stock.min[2];
    for (i, (&z, &target)) in grid.z.iter().zip(&run.target.z).enumerate() {
        if f64::from(z) <= floor + 1e-6 && f64::from(target) > floor + 0.02 {
            removed[(i / nx / stride) * w + (i % nx / stride)] = true;
        }
    }
    for (i, marked) in removed.into_iter().enumerate() {
        if !marked {
            continue;
        }
        let pos = |x, y| {
            [
                (grid.origin[0] + (x * stride) as f64 * grid.cell).min(run.stock.max[0]),
                (grid.origin[1] + (y * stride) as f64 * grid.cell).min(run.stock.max[1]),
                floor,
            ]
        };
        let (x, y) = (i % w, i / w);
        let k = cut.vertices.len() as u32;
        cut.vertices
            .extend([pos(x, y), pos(x + 1, y), pos(x + 1, y + 1), pos(x, y + 1)]);
        cut.triangles.extend([[k, k + 1, k + 2], [k, k + 2, k + 3]]);
        colours.extend([GOUGE, GOUGE]);
    }
}
pub fn scene(run: &Run) -> Result<Scene> {
    let grid = &run.verification.stock;
    if run.target.n != grid.n
        || run.target.cell != grid.cell
        || run.target.origin != grid.origin
        || run.target.z.len() != grid.z.len()
    {
        return Err("The cut stock does not match the target grid.".into());
    }
    let frame = run.world_from_machine;
    let mut cut = stock_mesh(&run.verification);
    let mut cut_colours = cut_colours(run, &cut);
    mark_through_cuts(run, &mut cut, &mut cut_colours);
    let stock = placed(stock_box(run.stock.min, run.stock.max), frame);
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = -lo;
    let mut include = |p: [f64; 3]| {
        lo = lo.min(p.into());
        hi = hi.max(p.into());
    };
    for p in &stock.vertices {
        include(*p);
    }
    let mut previous: Option<[f64; 3]> = None;
    let paths = run
        .paths
        .iter()
        .map(|(p, tool)| {
            let mut segments = Vec::new();
            let laser = tool.kind == faber_model::machining::ToolKind::Laser;
            let world = |p: [f64; 3]| frame.transform_point3(p.into()).to_array();
            for (index, m) in p.moves.iter().enumerate() {
                let Some(mut to) = m.target() else { continue };
                if laser {
                    to[2] = run.stock.max[2];
                }
                include(world(to));
                if let Some(mut from) = previous {
                    let rapid = matches!(m, Move::Rapid { .. });
                    if laser {
                        from[2] = to[2];
                    } else if index == 0 && rapid && from[2] != to[2] && from[..2] != to[..2] {
                        let corner = [from[0], from[1], to[2]];
                        include(world(corner));
                        segments.push(Segment {
                            from: world(from),
                            to: world(corner),
                            rapid,
                        });
                        from = corner;
                    }
                    if from != to {
                        segments.push(Segment {
                            from: world(from),
                            to: world(to),
                            rapid,
                        });
                    }
                }
                previous = Some(to);
            }
            DisplayPath {
                id: p.operation.clone(),
                segments,
            }
        })
        .collect();
    Ok(Scene {
        part: placed(run.mesh.clone(), frame),
        cut: placed(cut, frame),
        stock,
        cut_colours,
        paths,
        bounds: [lo.into(), hi.into()],
    })
}
pub fn render(run: &Run, path: &Path, width: u32, height: u32) -> Result<()> {
    scene(run)?.save(path, width, height)
}
