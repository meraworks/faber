use crate::{Elastic, Grid, Support};
use crate::{
    StaticCase,
    eigen::symmetric_eigen,
    element_matrix,
    solve::{self, Constraint, Operator, Row, VOID, corners, dot, xyz},
};
use faber_ports::Result;
use glam::DVec3;
use std::collections::BTreeMap;
mod band;

pub const MODAL_FREE_LIMIT: &str = "No fixture: the first 6 modes are rigid motion.";
#[derive(Clone, Debug)]
pub struct ModalCase {
    pub grid: Grid,
    pub material: Elastic,
    pub density_g_cm3: f64,
    pub supports: Vec<Support>,
    pub modes: usize,
}
#[derive(Clone, Debug)]
pub struct Mode {
    pub frequency_hz: f64,
    pub shape: Vec<[f32; 3]>,
    pub mass_share: [f64; 3],
}
pub fn solve_modal(c: &ModalCase) -> Result<Vec<Mode>> {
    solve_modal_with_progress(c, |_, _| true)
}
type Field = Vec<DVec3>;

fn mass_product(m: &[f64], u: &[DVec3], v: &[DVec3]) -> f64 {
    m.iter()
        .zip(u)
        .zip(v)
        .map(|((&m, a), b)| m * a.dot(*b))
        .sum()
}
fn orthonormalise(m: &[f64], v: &mut [DVec3], locked: &[Field], previous: &[Field]) -> bool {
    for _ in 0..2 {
        for a in locked.iter().chain(previous) {
            let t = mass_product(m, v, a);
            for (x, b) in v.iter_mut().zip(a) {
                *x -= t * *b;
            }
        }
    }
    let len = mass_product(m, v, v).sqrt();
    if len <= 1e-30 || !len.is_finite() {
        return false;
    }
    for x in v {
        *x /= len;
    }
    true
}
fn sample(seed: &mut u64) -> f64 {
    *seed = seed.wrapping_add(0x9e3779b97f4a7c15);
    let mut z = (*seed ^ (*seed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((z ^ (z >> 31)) >> 11) as f64 * (2. / ((1_u64 << 53) as f64)) - 1.
}

fn node_masses(g: &Grid, density: f64, nodes: usize) -> (Vec<f64>, Vec<usize>) {
    let mut mass = vec![0.; nodes];
    let mut groups: Vec<_> = (0..nodes).collect();
    fn root(p: &mut [usize], mut n: usize) -> usize {
        while p[n] != n {
            p[n] = p[p[n]];
            n = p[n];
        }
        n
    }
    for (c, &f) in g.fill.iter().enumerate().filter(|(_, v)| **v >= VOID) {
        let ids = corners(g, c);
        let first = root(&mut groups, ids[0]);
        let m = f64::from(f) * density * 1e-9 * g.h.powi(3) / 8.;
        for id in ids {
            mass[id] += m;
            let p = root(&mut groups, id);
            groups[p] = first;
        }
    }
    for i in 0..nodes {
        groups[i] = root(&mut groups, i);
    }
    (mass, groups)
}

fn rigid_modes(
    grid: &Grid,
    rows: &[Row],
    groups: &[usize],
    bc: &[Constraint],
    m: &[f64],
    maximum: usize,
) -> Vec<Field> {
    let mut sets = BTreeMap::<usize, Vec<usize>>::new();
    for (i, row) in rows.iter().enumerate() {
        sets.entry(groups[row.node]).or_default().push(i);
    }
    let dims = grid.n.map(|d| d + 1);
    let position = |i: usize| DVec3::from(xyz(rows[i].node, dims).map(|v| v as f64));
    let mut found = Vec::new();
    for indices in sets.into_values() {
        let total = indices.iter().map(|&i| m[i]).sum::<f64>();
        let centre = indices.iter().map(|&i| m[i] * position(i)).sum::<DVec3>() / total;
        let size = indices
            .iter()
            .map(|&i| position(i).distance(centre))
            .fold(1., f64::max);
        let basis = |i| {
            let p = (position(i) - centre) / size;
            [
                DVec3::X,
                DVec3::Y,
                DVec3::Z,
                DVec3::X.cross(p),
                DVec3::Y.cross(p),
                DVec3::Z.cross(p),
            ]
        };
        let mut gram = vec![vec![0.; 6]; 6];
        for &i in indices.iter().filter(|&&i| bc[i].rank > 0) {
            let constrained = basis(i).map(|a| a - bc[i].project(a));
            for a in 0..6 {
                for b in 0..6 {
                    gram[a][b] += constrained[a].dot(constrained[b]);
                }
            }
        }
        let (values, vectors) = symmetric_eigen(gram);
        let tol = values.iter().copied().fold(0., f64::max) * 1e-12;
        for (value, coef) in values.into_iter().zip(vectors) {
            if value > tol {
                continue;
            }
            let mut field = vec![DVec3::ZERO; rows.len()];
            for &i in &indices {
                field[i] = bc[i].project(basis(i).iter().zip(&coef).map(|(v, &k)| *v * k).sum());
            }
            if orthonormalise(m, &mut field, &found, &[]) {
                found.push(field);
            }
            if found.len() == maximum {
                return found;
            }
        }
    }
    found
}

fn mode(rows: &[Row], n: usize, m: &[f64], field: &[DVec3], frequency_hz: f64) -> Mode {
    let total = m.iter().sum::<f64>();
    let energy = mass_product(m, field, field);
    let resultant = m.iter().zip(field).map(|(&m, v)| m * *v).sum::<DVec3>();
    let scale = field.iter().map(|v| v.length()).fold(0., f64::max);
    let mut shape = vec![[0.; 3]; n];
    for (row, u) in rows.iter().zip(field) {
        shape[row.node] = (*u / scale).as_vec3().to_array();
    }
    Mode {
        frequency_hz,
        shape,
        mass_share: resultant
            .to_array()
            .map(|v| (v * v / (total * energy)).clamp(0., 1.)),
    }
}

struct Shifted<'a> {
    op: Operator<'a>,
    bc: Vec<Constraint>,
    mass: Vec<f64>,
    inverse: Field,
    shift: f64,
    band: Option<band::Band>,
}
impl Shifted<'_> {
    fn inverse(&mut self, rhs: &[DVec3], poll: &mut dyn FnMut() -> bool) -> Result<Field> {
        if !poll() {
            return Err("Study stopped.".into());
        }
        if let Some(b) = &self.band {
            return Ok(b.solve(rhs));
        }
        let n = rhs.len();
        let mut v = vec![DVec3::ZERO; n];
        let mut r = rhs.to_vec();
        let mut d: Field = (0..n)
            .map(|i| self.bc[i].project(self.inverse[i] * r[i]))
            .collect();
        let mut rz = dot(&r, &d);
        let target = dot(rhs, rhs) * 1e-12;
        let mut q = vec![DVec3::ZERO; n];
        for iteration in 0..6000 {
            if iteration % 8 == 0 && !poll() {
                return Err("Study stopped.".into());
            }
            self.op.apply(&d, &mut q);
            for (i, x) in q.iter_mut().enumerate() {
                *x = self.bc[i].project(*x) + self.shift * self.mass[i] * d[i];
            }
            let denom = dot(&d, &q);
            if !denom.is_finite() || denom <= 0. {
                return Err("The shifted modal stiffness is singular.".into());
            }
            let a = rz / denom;
            let mut rr = 0.;
            for i in 0..n {
                v[i] += a * d[i];
                r[i] -= a * q[i];
                rr += r[i].length_squared();
            }
            if rr <= target {
                return Ok(v);
            }
            let mut next = 0.;
            for i in 0..n {
                q[i] = self.bc[i].project(self.inverse[i] * r[i]);
                next += q[i].dot(r[i]);
            }
            for i in 0..n {
                d[i] = q[i] + (next / rz) * d[i];
            }
            rz = next;
        }
        Err("The modal stiffness solve did not converge.".into())
    }
}

/// Return false from the callback to stop at the next solver step.
pub fn solve_modal_with_progress(
    c: &ModalCase,
    mut progress: impl FnMut(usize, f64) -> bool,
) -> Result<Vec<Mode>> {
    let g = &c.grid;
    solve::validate_elastic_grid(g, c.material)?;
    if !(c.density_g_cm3 > 0. && c.density_g_cm3.is_finite()) {
        return Err("Modal analysis needs a finite positive density.".into());
    }
    if !(1..=32).contains(&c.modes) {
        return Err("Select 1 to 32 modes.".into());
    }
    if !progress(0, 1.) {
        return Err("Study stopped.".into());
    }
    let n = g.n.iter().map(|a| a + 1).product::<usize>();
    let block = c.modes + 4;
    if n.saturating_mul(320 + 24 * (3 * block + 12)) > 768 * 1024 * 1024 {
        return Err(
            "The modal workspace exceeds 768 MiB. Use a larger cell size or fewer modes.".into(),
        );
    }
    let helper = StaticCase {
        grid: g.clone(),
        material: c.material,
        supports: c.supports.clone(),
        loads: vec![],
        tolerance: 1e-6,
        max_iterations: 6000,
    };
    let (all_bc, loads) = solve::boundary(&helper, n, &mut progress)?;
    drop(loads);
    drop(helper);
    let element = element_matrix(&c.material, g.h);
    let op = Operator::new(g, element);
    let (all_mass, groups) = node_masses(g, c.density_g_cm3, n);
    let bc: Vec<_> = op.rows.iter().map(|r| all_bc[r.node]).collect();
    drop(all_bc);
    let mass: Vec<_> = op.rows.iter().map(|r| all_mass[r.node]).collect();
    drop(all_mass);
    if mass.iter().any(|m| !m.is_finite() || *m <= 0.) {
        return Err("The nodal masses must be finite and positive.".into());
    }
    let dof = bc.iter().map(|b| 3 - b.rank).sum::<usize>();
    if c.modes > dof {
        return Err(format!(
            "The grid has {dof} free degrees of freedom. It cannot provide {} modes.",
            c.modes
        ));
    }
    let locked = rigid_modes(g, &op.rows, &groups, &bc, &mass, c.modes);
    let mut result: Vec<_> = locked
        .iter()
        .map(|v| mode(&op.rows, n, &mass, v, 0.))
        .collect();
    if result.len() == c.modes {
        return Ok(result);
    }
    let want = c.modes - result.len();
    let width = (want + 4).min(dof - result.len());
    let length = g.h * *g.n.iter().max().unwrap() as f64;
    let shift = 1e-3 * c.material.youngs_mpa / (c.density_g_cm3 * 1e-9 * length * length);
    let inverse = op
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (op.inverse_diag(r).recip() + DVec3::splat(shift * mass[i])).recip())
        .collect();
    let band = band::Band::new(g, &op.rows, &bc, &element, &mass, shift, &mut || {
        progress(0, 1.)
    })?;
    let mut system = Shifted {
        op,
        bc,
        mass,
        inverse,
        shift,
        band,
    };
    let mut seed = 29;
    let mut fields = Vec::new();
    for _ in 0..width {
        let mut v = system
            .bc
            .iter()
            .map(|b| {
                b.project(DVec3::new(
                    sample(&mut seed),
                    sample(&mut seed),
                    sample(&mut seed),
                ))
            })
            .collect::<Field>();
        if !orthonormalise(&system.mass, &mut v, &locked, &fields) {
            return Err("The modal start vectors lost independence.".into());
        }
        fields.push(v);
    }
    let mut previous = vec![0.; want];
    let mut change = 1.;
    for step in 1..=48 {
        let mut next = Vec::new();
        for v in &fields {
            let rhs: Field = v.iter().zip(&system.mass).map(|(u, &m)| m * *u).collect();
            let mut candidate = system.inverse(&rhs, &mut || progress(step, change))?;
            if !orthonormalise(&system.mass, &mut candidate, &locked, &next) {
                return Err("The modal basis lost independence.".into());
            }
            next.push(candidate);
        }
        let mut small = vec![vec![0.; width]; width];
        let mut work = vec![DVec3::ZERO; system.bc.len()];
        for (i, v) in next.iter().enumerate() {
            system.op.apply(v, &mut work);
            for j in 0..=i {
                small[i][j] = dot(&next[j], &work);
                small[j][i] = small[i][j];
            }
        }
        let (values, rotation) = symmetric_eigen(small);
        let frequencies: Vec<_> = values
            .iter()
            .take(want)
            .map(|v| v.max(0.).sqrt() / std::f64::consts::TAU)
            .collect();
        if frequencies.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err("The grid has an unresolved zero-stiffness mode.".into());
        }
        change = frequencies
            .iter()
            .zip(&previous)
            .map(|(a, b)| (a - b).abs() / a)
            .fold(0., f64::max);
        for (out, coef) in fields.iter_mut().zip(rotation) {
            out.fill(DVec3::ZERO);
            for (v, f) in next.iter().zip(coef) {
                for (o, x) in out.iter_mut().zip(v) {
                    *o += f * *x;
                }
            }
        }
        if !progress(step, change) {
            return Err("Study stopped.".into());
        }
        if step >= 3 && change < 0.001 {
            result.extend(
                fields
                    .iter()
                    .zip(frequencies)
                    .map(|(v, f)| mode(&system.op.rows, n, &system.mass, v, f)),
            );
            return Ok(result);
        }
        previous = frequencies;
    }
    Err(format!(
        "Modal iteration did not converge in 48 steps. Relative frequency change: {change:.4e}."
    ))
}
