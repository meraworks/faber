//! Matrix-free, small-displacement analysis of trilinear elastic cells.

use crate::{Grid, element::element_matrix, stress};
use faber_ports::Result;
use glam::DVec3;
use rayon::prelude::*;
use std::collections::{HashMap, VecDeque};

type Tri = [[f64; 3]; 3];
type Matrix = [[f64; 24]; 24];
const LOCAL_CACHE_LIMIT: usize = 1024;
pub(crate) const VOID: f32 = 0.02;

#[derive(Clone, Copy, Debug)]
pub struct Elastic {
    pub youngs_mpa: f64,
    pub poisson: f64,
}
#[derive(Clone, Debug)]
pub enum Support {
    Fixed {
        cells: Vec<usize>,
    },
    Slide {
        cells: Vec<usize>,
        normal: [f64; 3],
    },
    FixedFace {
        triangles: Vec<Tri>,
    },
    SlideFace {
        triangles: Vec<Tri>,
        normal: [f64; 3],
    },
}
#[derive(Clone, Debug)]
pub enum Load {
    Temperature {
        field: Vec<f32>,
        reference_c: f64,
        expansion_um_mk: f64,
    },
    Force {
        cells: Vec<usize>,
        newtons: [f64; 3],
    },
    Traction {
        triangles: Vec<Tri>,
        newtons: [f64; 3],
    },
    Pressure {
        triangles: Vec<Tri>,
        mpa: f64,
    },
    Gravity {
        direction: [f64; 3],
        density_g_cm3: f64,
    },
}
#[derive(Clone, Debug)]
pub struct StaticCase {
    pub grid: Grid,
    pub material: Elastic,
    pub supports: Vec<Support>,
    pub loads: Vec<Load>,
    pub tolerance: f64,
    pub max_iterations: usize,
}
#[derive(Debug)]
pub struct StaticResult {
    pub displacement: Vec<[f64; 3]>,
    pub von_mises: Vec<f32>,
    pub stress_mpa: Vec<[f64; 6]>,
    pub iterations: usize,
    pub residual: f64,
    pub reaction: [f64; 3],
}

pub(crate) fn xyz(i: usize, n: [usize; 3]) -> [usize; 3] {
    [i % n[0], i / n[0] % n[1], i / (n[0] * n[1])]
}
fn index(p: [usize; 3], n: [usize; 3]) -> usize {
    p[0] + n[0] * (p[1] + n[1] * p[2])
}
fn bit(c: usize) -> [usize; 3] {
    [c & 1, (c >> 1) & 1, c >> 2]
}
pub(crate) fn corners(g: &Grid, c: usize) -> [usize; 8] {
    let [x, y, z] = xyz(c, g.n);
    std::array::from_fn(|i| {
        let [u, v, w] = bit(i);
        g.node(x + u, y + v, z + w)
    })
}
fn shift(p: [usize; 3], axis: usize, d: isize, n: [usize; 3]) -> Option<usize> {
    let mut q = p;
    q[axis] = q[axis].checked_add_signed(d)?;
    (q[axis] < n[axis]).then(|| index(q, n))
}
fn validate(c: &StaticCase) -> Result<()> {
    validate_elastic_grid(&c.grid, c.material)?;
    if !(c.tolerance.is_finite() && c.tolerance > 0. && c.tolerance < 1.) {
        return Err("The solver tolerance must be between zero and one.".into());
    }
    if c.supports.is_empty() {
        return Err("The study has no fixture. Add a fixed face.".into());
    }
    Ok(())
}
pub(crate) fn validate_elastic_grid(g: &Grid, m: Elastic) -> Result<()> {
    if !(g.h > 0. && g.h.is_finite() && g.origin.iter().all(|v| v.is_finite()))
        || g.n.contains(&0)
        || g.n.iter().try_fold(1usize, |a, &n| a.checked_mul(n)) != Some(g.fill.len())
        || g.fill.len() > crate::CELL_LIMIT
        || g.fill.iter().any(|f| !(0.0..=1.).contains(f))
    {
        return Err("Invalid stress grid.".into());
    }
    if !(m.youngs_mpa.is_finite() && m.youngs_mpa > 0. && m.poisson > -1. && m.poisson < 0.5) {
        return Err("Invalid elastic material.".into());
    }
    if !g.fill.iter().any(|&f| f >= VOID) {
        return Err("The grid has no solid cells.".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Constraint {
    axis: [DVec3; 3],
    pub(crate) rank: usize,
}
impl Constraint {
    fn add(&mut self, normal: Option<DVec3>) {
        let Some(n) = normal else {
            self.axis = [DVec3::X, DVec3::Y, DVec3::Z];
            self.rank = 3;
            return;
        };
        if self.rank == 3 {
            return;
        }
        let d = self.project(n);
        if d.length() > 1e-8 {
            self.axis[self.rank] = d.normalize();
            self.rank += 1;
        }
    }
    pub(crate) fn project(&self, mut v: DVec3) -> DVec3 {
        if self.rank == 3 {
            return DVec3::ZERO;
        }
        for a in &self.axis[..self.rank] {
            v -= a.dot(v) * *a;
        }
        v
    }
}

fn unit(v: [f64; 3]) -> Result<DVec3> {
    DVec3::from(v)
        .try_normalize()
        .filter(|n| n.is_finite())
        .ok_or_else(|| "A direction must have a finite, nonzero length.".into())
}

/// Use the largest exposed side of a selected cell layer.
pub(crate) fn face_weights(g: &Grid, cells: &[usize]) -> Result<Vec<(usize, f64)>> {
    if cells.is_empty() || cells.iter().any(|&c| c >= g.fill.len() || g.fill[c] < VOID) {
        return Err("The face has no active cell at this resolution.".into());
    }
    let mut ids = cells.to_vec();
    ids.sort_unstable();
    ids.dedup();
    let exposed = |p, side: usize| {
        shift(
            p,
            side / 2,
            if side.is_multiple_of(2) { -1 } else { 1 },
            g.n,
        )
        .is_none_or(|i| g.fill[i] < VOID)
    };
    let mut count = [0.; 6];
    let mut position = [0.; 6];
    for &id in &ids {
        let p = xyz(id, g.n);
        for side in 0..6 {
            if exposed(p, side) {
                count[side] += f64::from(g.fill[id]);
                position[side] += ((p[side / 2] as f64 + 0.5) / g.n[side / 2] as f64 - 0.5)
                    * if side.is_multiple_of(2) { -1. } else { 1. };
            }
        }
    }
    let side = (0..6)
        .max_by(|&a, &b| {
            count[a]
                .total_cmp(&count[b])
                .then(position[a].total_cmp(&position[b]))
        })
        .unwrap();
    if count[side] == 0. {
        return Err("The selected cells do not touch an outer face.".into());
    }
    let mut weights = Vec::new();
    for id in ids {
        if exposed(xyz(id, g.n), side) {
            for (local, node) in corners(g, id).into_iter().enumerate() {
                if bit(local)[side / 2] == side % 2 {
                    weights.push((node, f64::from(g.fill[id]) / (4. * count[side])));
                }
            }
        }
    }
    Ok(weights)
}

pub(crate) mod sampling;

pub(crate) fn boundary(
    case: &StaticCase,
    nodes: usize,
    progress: &mut dyn FnMut(usize, f64) -> bool,
) -> Result<(Vec<Constraint>, Vec<DVec3>)> {
    let g = &case.grid;
    let mut sampler = sampling::Sampler::new(g, progress);
    let mut constraints = vec![Constraint::default(); nodes];
    let mut force = vec![DVec3::ZERO; nodes];
    for support in &case.supports {
        let normal = match support {
            Support::Slide { normal, .. } | Support::SlideFace { normal, .. } => {
                Some(unit(*normal)?)
            }
            _ => None,
        };
        match support {
            Support::Fixed { cells } | Support::Slide { cells, .. } => {
                for (node, _) in face_weights(g, cells)? {
                    constraints[node].add(normal)
                }
            }
            Support::FixedFace { triangles } | Support::SlideFace { triangles, .. } => {
                sampler.face(triangles, |node, _, _| constraints[node].add(normal))?;
            }
        }
    }
    for load in &case.loads {
        match load {
            Load::Temperature {
                field,
                reference_c,
                expansion_um_mk,
            } => {
                crate::thermal_strain::add_forces(
                    g,
                    &case.material,
                    field,
                    *reference_c,
                    *expansion_um_mk,
                    &mut force,
                )?;
            }
            Load::Force { cells, newtons } => {
                for (node, w) in face_weights(g, cells)? {
                    force[node] += DVec3::from(*newtons) * w;
                }
            }
            Load::Traction { triangles, newtons } => {
                let mut weights = HashMap::<usize, f64>::new();
                let area = sampler.face(triangles, |node, w, _| {
                    *weights.entry(node).or_default() += w
                })?;
                for (node, w) in weights {
                    force[node] += DVec3::from(*newtons) * (w / area);
                }
            }
            Load::Pressure { triangles, mpa } => {
                if !mpa.is_finite() {
                    return Err("Invalid pressure.".into());
                }
                sampler.face(triangles, |node, w, normal| {
                    force[node] -= normal * (w * mpa)
                })?;
            }
            Load::Gravity {
                direction,
                density_g_cm3,
            } => {
                if !density_g_cm3.is_finite() || *density_g_cm3 <= 0. {
                    return Err("Invalid material density.".into());
                }
                let f = unit(*direction)? * (*density_g_cm3 * g.h.powi(3) * 9.80665e-6 / 8.);
                for (id, &fill) in g.fill.iter().enumerate().filter(|(_, f)| **f >= VOID) {
                    for node in corners(g, id) {
                        force[node] += f * f64::from(fill)
                    }
                }
            }
        }
    }
    if force.iter().any(|f| !f.is_finite()) {
        return Err("The applied load must be finite.".into());
    }
    Ok((constraints, force))
}

fn restrain_mode(basis: &mut Vec<[f64; 6]>, axis: DVec3, r: DVec3) {
    if basis.len() == 6 {
        return;
    }
    let w = r.cross(axis);
    let mut v = [axis.x, axis.y, axis.z, w.x, w.y, w.z];
    for b in basis.iter() {
        let dot = (0..6).map(|i| b[i] * v[i]).sum::<f64>();
        for i in 0..6 {
            v[i] -= dot * b[i]
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm > 1e-10 {
        basis.push(v.map(|x| x / norm))
    }
}

/// Test the six rigid movements separately for each connected solid.
fn check_fixtures(g: &Grid, bc: &[Constraint]) -> Result<()> {
    let mut visited = vec![false; g.fill.len()];
    let mut loose = 0;
    let mut mechanism = false;
    for (start, &f) in g.fill.iter().enumerate() {
        if f < VOID || visited[start] {
            continue;
        }
        visited[start] = true;
        let origin = xyz(start, g.n).map(|x| x as f64);
        let scale = *g.n.iter().max().unwrap() as f64;
        let mut cells = 0;
        let mut rank = vec![];
        let mut q = VecDeque::from([start]);
        while let Some(id) = q.pop_front() {
            cells += 1;
            if rank.len() < 6 {
                for node in corners(g, id) {
                    let constraint = &bc[node];
                    if constraint.rank == 0 {
                        continue;
                    }
                    let p = xyz(node, g.n.map(|n| n + 1)).map(|x| x as f64);
                    let r = (DVec3::from(p) - DVec3::from(origin)) / scale;
                    for &a in &constraint.axis[..constraint.rank] {
                        restrain_mode(&mut rank, a, r)
                    }
                }
            }
            for ax in 0..3 {
                for d in [-1, 1] {
                    if let Some(i) = shift(xyz(id, g.n), ax, d, g.n)
                        && !visited[i]
                        && g.fill[i] >= VOID
                    {
                        visited[i] = true;
                        q.push_back(i);
                    }
                }
            }
        }
        if rank.is_empty() {
            loose += cells
        } else if rank.len() < 6 {
            mechanism = true
        }
    }
    if loose > 0 {
        return Err(format!("{loose} cells have no connection to a fixture."));
    }
    if mechanism {
        return Err("The fixtures leave the part free to move. Add a fixed face.".into());
    }
    Ok(())
}

struct Term {
    offset: isize,
    columns: [DVec3; 3],
}
struct Stencil {
    terms: Vec<Term>,
    inv_diag: DVec3,
}
pub(crate) struct Row {
    pub(crate) node: usize,
    padded: usize,
    stencil: u16,
}
pub(crate) struct Operator<'a> {
    grid: &'a Grid,
    stiffness_fill: &'a [f32],
    element: Matrix,
    strides: [usize; 3],
    pub(crate) rows: Vec<Row>,
    stencils: Vec<Stencil>,
    padded: Vec<DVec3>,
}

fn neighbourhood(g: &Grid, stiffness_fill: &[f32], p: [usize; 3]) -> [u32; 8] {
    std::array::from_fn(|corner| {
        let d = bit(corner);
        if (0..3).any(|a| p[a] < d[a] || p[a] - d[a] >= g.n[a]) {
            return 0;
        }
        let i = index(std::array::from_fn(|a| p[a] - d[a]), g.n);
        if g.fill[i] >= VOID {
            stiffness_fill[i].to_bits()
        } else {
            0
        }
    })
}
fn local_offset(i: usize, j: usize, strides: [usize; 3]) -> isize {
    let a = bit(i);
    let b = bit(j);
    (0..3)
        .map(|d| (b[d] as isize - a[d] as isize) * strides[d] as isize)
        .sum()
}
fn stencil(k: &Matrix, fill: [u32; 8], strides: [usize; 3]) -> Stencil {
    let mut blocks = [[[0.; 3]; 3]; 27];
    for (i, &f) in fill.iter().enumerate().filter(|(_, f)| **f != 0) {
        let scale = f64::from(f32::from_bits(f));
        for j in 0..8 {
            let idx = (13 + local_offset(i, j, [1, 3, 9])) as usize;
            for a in 0..3 {
                for b in 0..3 {
                    blocks[idx][b][a] += scale * k[i * 3 + a][j * 3 + b];
                }
            }
        }
    }
    let inv_diag = DVec3::new(blocks[13][0][0], blocks[13][1][1], blocks[13][2][2]).recip();
    let terms = blocks
        .into_iter()
        .enumerate()
        .filter(|(_, block)| block.iter().flatten().any(|&x| x != 0.))
        .map(|(i, block)| {
            let p = xyz(i, [3; 3]);
            let offset = (0..3)
                .map(|a| (p[a] as isize - 1) * strides[a] as isize)
                .sum();
            Term {
                offset,
                columns: block.map(DVec3::from),
            }
        })
        .collect();
    Stencil { terms, inv_diag }
}

impl<'a> Operator<'a> {
    pub(crate) fn new(g: &'a Grid, k: Matrix) -> Self {
        Self::weighted(g, k, &g.fill, LOCAL_CACHE_LIMIT)
    }
    fn weighted(g: &'a Grid, k: Matrix, stiffness_fill: &'a [f32], cache_limit: usize) -> Self {
        let n = g.n.map(|n| n + 1);
        let pad = n.map(|n| n + 2);
        let strides = [1, pad[0], pad[0] * pad[1]];
        let mut assigned = vec![false; n.iter().product()];
        let mut rows = vec![];
        let mut stencils = vec![];
        let mut patterns = HashMap::new();
        // Each of the eight cell colours owns its remaining shared nodes.
        // Rows with the same eight fill fractions share one local convolution.
        for colour in 0..8 {
            let [dx, dy, dz] = bit(colour);
            for z in (dz..g.n[2]).step_by(2) {
                for y in (dy..g.n[1]).step_by(2) {
                    for x in (dx..g.n[0]).step_by(2) {
                        let id = index([x, y, z], g.n);
                        if g.fill[id] < VOID {
                            continue;
                        }
                        for node in corners(g, id) {
                            if std::mem::replace(&mut assigned[node], true) {
                                continue;
                            }
                            let p = xyz(node, n);
                            let fill = neighbourhood(g, stiffness_fill, p);
                            let key = if let Some(&i) = patterns.get(&fill) {
                                i
                            } else if stencils.len() < cache_limit {
                                let i = stencils.len() as u16;
                                stencils.push(stencil(&k, fill, strides));
                                patterns.insert(fill, i);
                                i
                            } else {
                                u16::MAX
                            };
                            rows.push(Row {
                                node,
                                padded: index(p.map(|x| x + 1), pad),
                                stencil: key,
                            });
                        }
                    }
                }
            }
        }
        Self {
            grid: g,
            stiffness_fill,
            element: k,
            strides,
            rows,
            stencils,
            padded: vec![DVec3::ZERO; pad.iter().product()],
        }
    }
    pub(crate) fn inverse_diag(&self, row: &Row) -> DVec3 {
        if let Some(s) = self.stencils.get(usize::from(row.stencil)) {
            return s.inv_diag;
        }
        let fill = neighbourhood(
            self.grid,
            self.stiffness_fill,
            xyz(row.node, self.grid.n.map(|n| n + 1)),
        );
        let mut diag = DVec3::ZERO;
        for (i, f) in fill.into_iter().enumerate() {
            for a in 0..3 {
                diag[a] += f64::from(f32::from_bits(f)) * self.element[i * 3 + a][i * 3 + a]
            }
        }
        diag.recip()
    }
    pub(crate) fn apply(&mut self, x: &[DVec3], out: &mut [DVec3]) {
        for (row, v) in self.rows.iter().zip(x) {
            self.padded[row.padded] = *v;
        }
        let work = |(row, y): (&Row, &mut DVec3)| {
            let mut value = DVec3::ZERO;
            if let Some(s) = self.stencils.get(usize::from(row.stencil)) {
                for t in &s.terms {
                    let v = self.padded[row.padded.wrapping_add_signed(t.offset)];
                    value += t.columns[0] * v.x + t.columns[1] * v.y + t.columns[2] * v.z;
                }
            } else {
                let fill = neighbourhood(
                    self.grid,
                    self.stiffness_fill,
                    xyz(row.node, self.grid.n.map(|n| n + 1)),
                );
                for (i, f) in fill.into_iter().enumerate().filter(|(_, f)| *f != 0) {
                    let mut cell = DVec3::ZERO;
                    for j in 0..8 {
                        let v = self.padded[row.padded.wrapping_add_signed(local_offset(
                            i,
                            j,
                            self.strides,
                        ))];
                        for a in 0..3 {
                            for b in 0..3 {
                                cell[a] += self.element[3 * i + a][3 * j + b] * v[b]
                            }
                        }
                    }
                    value += cell * f64::from(f32::from_bits(f));
                }
            }
            *y = value;
        };
        if self.rows.len() >= 8192 {
            self.rows
                .par_iter()
                .zip(out.par_iter_mut())
                .with_min_len(2048)
                .for_each(work)
        } else {
            self.rows.iter().zip(out).for_each(work)
        }
    }
}

pub(crate) fn dot(x: &[DVec3], y: &[DVec3]) -> f64 {
    x.iter().zip(y).map(|(a, b)| a.dot(*b)).sum()
}

pub fn solve_static(case: &StaticCase) -> Result<StaticResult> {
    solve_static_with_progress(case, |_, _| true)
}

/// Return false from the callback to stop at the next iteration.
pub fn solve_static_with_progress(
    case: &StaticCase,
    mut progress: impl FnMut(usize, f64) -> bool,
) -> Result<StaticResult> {
    let field = displacements(case, None, None, &mut progress)?;
    let (von_mises, stress_mpa) =
        stress::recover(&case.grid, &case.material, &field.displacement, &case.loads)?;
    Ok(StaticResult {
        displacement: field.displacement,
        von_mises,
        stress_mpa,
        reaction: field.reaction,
        iterations: field.iterations,
        residual: field.residual,
    })
}

pub(crate) struct ElasticField {
    pub displacement: Vec<[f64; 3]>,
    iterations: usize,
    residual: f64,
    reaction: [f64; 3],
}
pub(crate) fn displacements(
    case: &StaticCase,
    stiffness_fill: Option<&[f32]>,
    start: Option<&[[f64; 3]]>,
    progress: &mut dyn FnMut(usize, f64) -> bool,
) -> Result<ElasticField> {
    validate(case)?;
    if !progress(0, 1.) {
        return Err("Study stopped.".into());
    }
    let g = &case.grid;
    let count = g.n.iter().map(|v| v + 1).product();
    let (constraints, loads) = boundary(case, count, progress)?;
    check_fixtures(g, &constraints)?;
    let k = element_matrix(&case.material, g.h);
    let mut op = match stiffness_fill {
        Some(weights) => Operator::weighted(g, k, weights, u16::MAX as usize),
        None => Operator::new(g, k),
    };
    let n = op.rows.len();
    let bc: Vec<_> = op.rows.iter().map(|r| constraints[r.node]).collect();
    drop(constraints);
    let rhs: Vec<_> = op
        .rows
        .iter()
        .zip(&bc)
        .map(|(r, b)| b.project(loads[r.node]))
        .collect();
    let inv: Vec<_> = op.rows.iter().map(|r| op.inverse_diag(r)).collect();
    let mut u = vec![DVec3::ZERO; n];
    let mut r = rhs.clone();
    if let Some(start) = start {
        for (i, row) in op.rows.iter().enumerate() {
            u[i] = bc[i].project(DVec3::from(start[row.node]));
        }
        let mut product = vec![DVec3::ZERO; n];
        op.apply(&u, &mut product);
        for (i, v) in r.iter_mut().enumerate() {
            *v -= bc[i].project(product[i]);
        }
    }
    let mut z: Vec<_> = (0..n).map(|i| bc[i].project(r[i] * inv[i])).collect();
    let mut dir = z.clone();
    let mut q = vec![DVec3::ZERO; n];
    let rhs_norm = dot(&rhs, &rhs).sqrt();
    let mut residual = if rhs_norm == 0. {
        0.
    } else {
        dot(&r, &r).sqrt() / rhs_norm
    };
    let mut rz = dot(&r, &z);
    let mut iterations = 0;
    while residual > case.tolerance && iterations < case.max_iterations {
        op.apply(&dir, &mut q);
        let denom = dot(&dir, &q);
        if !denom.is_finite() || denom <= 0. || !rz.is_finite() {
            return Err("The elastic stiffness is singular or not finite.".into());
        }
        let alpha = rz / denom;
        let mut rr = 0.;
        for i in 0..n {
            u[i] += alpha * dir[i];
            r[i] -= alpha * bc[i].project(q[i]);
            rr += r[i].length_squared();
        }
        iterations += 1;
        residual = rr.sqrt() / rhs_norm;
        if !progress(iterations, residual) {
            return Err("Study stopped.".into());
        }
        let recompute = residual <= case.tolerance;
        if recompute {
            op.apply(&u, &mut q);
            for i in 0..n {
                r[i] = rhs[i] - bc[i].project(q[i]);
            }
            residual = dot(&r, &r).sqrt() / rhs_norm;
            if residual <= case.tolerance {
                break;
            }
        }
        for i in 0..n {
            z[i] = bc[i].project(r[i] * inv[i]);
        }
        let rz_next = dot(&r, &z);
        let beta = if recompute { 0. } else { rz_next / rz };
        for i in 0..n {
            dir[i] = z[i] + beta * dir[i];
        }
        rz = rz_next;
    }
    if !residual.is_finite() || residual > case.tolerance {
        return Err(format!(
            "The solver did not converge in {iterations} iterations (residual {residual:.6e})."
        ));
    }
    op.apply(&u, &mut q);
    let mut reaction = DVec3::ZERO;
    let mut displacement = vec![[0.; 3]; count];
    for (i, row) in op.rows.iter().enumerate() {
        let imbalance = q[i] - loads[row.node];
        reaction += imbalance - bc[i].project(imbalance);
        displacement[row.node] = u[i].into();
    }
    Ok(ElasticField {
        displacement,
        iterations,
        residual,
        reaction: reaction.into(),
    })
}
