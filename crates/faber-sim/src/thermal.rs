//! Steady conduction in trilinear cells with surface heat transfer.
use crate::{
    Grid,
    element::gradients,
    solve::{corners, face_weights, sampling::Sampler},
};
use faber_ports::Result;

type Tri = [[f64; 3]; 3];

const WORKSPACE_LIMIT: usize = 768 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum ThermalBc {
    Temperature {
        cells: Vec<usize>,
        celsius: f64,
    },
    Power {
        cells: Vec<usize>,
        watts: f64,
    },
    TemperatureFace {
        triangles: Vec<Tri>,
        celsius: f64,
    },
    PowerFace {
        triangles: Vec<Tri>,
        watts: f64,
    },
    Convection {
        triangles: Vec<Tri>,
        w_m2k: f64,
        ambient_c: f64,
    },
}

pub struct ThermalCase {
    pub grid: Grid,
    pub conductivity_w_mk: f64,
    pub conditions: Vec<ThermalBc>,
}

#[derive(Debug)]
pub struct ThermalResult {
    pub temperature: Vec<f32>,
    pub max_c: f64,
    pub min_c: f64,
    pub iterations: usize,
}

struct Cell {
    nodes: [usize; 8],
    fill: f64,
}

fn conduction(h: f64, conductivity: f64) -> [[f64; 8]; 8] {
    let mut matrix = [[0.; 8]; 8];
    let a = 1. / 3_f64.sqrt();
    for x in [-a, a] {
        for y in [-a, a] {
            for z in [-a, a] {
                let gradient = gradients(1., [x, y, z]);
                for (i, row) in matrix.iter_mut().enumerate() {
                    for (j, term) in row.iter_mut().enumerate() {
                        *term += gradient[i].dot(gradient[j]) / 8.;
                    }
                }
            }
        }
    }
    let scale = (conductivity / 1000.) * h;
    matrix.map(|row| row.map(|value| value * scale))
}

fn temperatures(fixed: &mut [Option<f64>], weights: &[(usize, f64)], value: f64) -> Result<()> {
    if !value.is_finite() {
        return Err("The temperature must be finite.".into());
    }
    for &(i, w) in weights {
        if w > 0.
            && fixed[i]
                .replace(value)
                .is_some_and(|old| (old - value).abs() > 1e-9)
        {
            return Err("Two fixed temperatures act on the same node.".into());
        }
    }
    Ok(())
}

fn surface_weights(
    sampler: &mut Sampler<'_>,
    triangles: &[Tri],
) -> Result<(Vec<(usize, f64)>, f64)> {
    let mut weights = std::collections::BTreeMap::<usize, f64>::new();
    let area = sampler.face(triangles, |i, w, _| *weights.entry(i).or_default() += w)?;
    Ok((weights.into_iter().collect(), area))
}

fn root(parents: &mut [usize], mut i: usize) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i];
    }
    i
}

pub fn solve_thermal(case: &ThermalCase) -> Result<ThermalResult> {
    solve_thermal_with_progress(case, |_, _| true)
}

pub fn solve_thermal_with_progress(
    case: &ThermalCase,
    mut progress: impl FnMut(usize, f64) -> bool,
) -> Result<ThermalResult> {
    let grid = &case.grid;
    if !grid.h.is_finite()
        || grid.h <= 0.
        || grid.n.contains(&0)
        || !grid.origin.iter().all(|v| v.is_finite())
        || grid.n.iter().try_fold(1usize, |a, &b| a.checked_mul(b)) != Some(grid.fill.len())
        || grid.fill.len() > crate::CELL_LIMIT
        || grid.fill.iter().any(|v| !(0.0..=1.).contains(v))
    {
        return Err("Invalid thermal grid.".into());
    }
    if !(case.conductivity_w_mk > 0. && case.conductivity_w_mk.is_finite()) {
        return Err("Thermal conductivity must be finite and greater than zero.".into());
    }
    let n: usize = grid.n.map(|v| v + 1).iter().product();
    let active = grid.fill.iter().filter(|&&f| f >= 0.02).count();
    // Allow for nodal vectors, face weights and the solid cell table.
    let bytes = n
        .saturating_mul(112)
        .saturating_add(active.saturating_mul(std::mem::size_of::<Cell>()));
    if bytes > WORKSPACE_LIMIT {
        return Err(format!(
            "The thermal workspace needs {} MiB. The limit is 768 MiB. Use a larger resolution.",
            bytes.div_ceil(1024 * 1024)
        ));
    }
    if active == 0 {
        return Err("The grid has no solid cells.".into());
    }
    if !progress(0, 1.) {
        return Err("Study stopped.".into());
    }
    let reference =
        case.conditions
            .iter()
            .find_map(|bc| match bc {
                ThermalBc::Temperature { celsius, .. }
                | ThermalBc::TemperatureFace { celsius, .. } => Some(*celsius),
                ThermalBc::Convection { ambient_c, .. } => Some(*ambient_c),
                _ => None,
            })
            .unwrap_or(0.);
    let mut fixed = vec![None; n];
    let mut rhs = vec![0.; n];
    let mut convection = vec![0.; n];
    let mut sampler = Sampler::new(grid, &mut progress);
    for bc in &case.conditions {
        match bc {
            ThermalBc::Temperature { cells, celsius } => {
                temperatures(&mut fixed, &face_weights(grid, cells)?, *celsius)?
            }
            ThermalBc::TemperatureFace { triangles, celsius } => temperatures(
                &mut fixed,
                &surface_weights(&mut sampler, triangles)?.0,
                *celsius,
            )?,
            ThermalBc::Power { cells, watts } => {
                for (i, w) in face_weights(grid, cells)? {
                    rhs[i] += watts * w;
                }
            }
            ThermalBc::PowerFace { triangles, watts } => {
                let (weights, area) = surface_weights(&mut sampler, triangles)?;
                for (i, w) in weights {
                    rhs[i] += watts * (w / area);
                }
            }
            ThermalBc::Convection {
                triangles,
                w_m2k,
                ambient_c,
            } => {
                if !(*w_m2k > 0. && w_m2k.is_finite() && ambient_c.is_finite()) {
                    return Err(
                        "Convection needs a positive coefficient and a finite air temperature."
                            .into(),
                    );
                }
                for (i, w) in surface_weights(&mut sampler, triangles)?.0 {
                    let k = (w * 1e-6) * w_m2k;
                    convection[i] += k;
                    rhs[i] += k * (ambient_c - reference);
                }
            }
        }
    }
    if rhs.iter().any(|v| !v.is_finite()) {
        return Err("The heat input must be finite.".into());
    }
    if fixed.iter().all(Option::is_none) && convection.iter().all(|v| *v == 0.) {
        return Err("The study has heat input and no fixed temperature or convection.".into());
    }
    let matrix = conduction(grid.h, case.conductivity_w_mk);
    let mut parents: Vec<_> = (0..n).collect();
    if matrix
        .iter()
        .enumerate()
        .any(|(i, row)| row[i] <= 0. || row.iter().any(|v| !v.is_finite()))
    {
        return Err("Thermal stiffness exceeds the numerical range.".into());
    }
    let mut cells = Vec::with_capacity(active);
    let mut diagonal = convection.clone();
    for (i, &fill) in grid.fill.iter().enumerate() {
        if i.is_multiple_of(2048) && !progress(0, 1.) {
            return Err("Study stopped.".into());
        }
        if fill < 0.02 {
            continue;
        }
        let nodes = corners(grid, i);
        let fill = f64::from(fill);
        let first = root(&mut parents, nodes[0]);
        for (a, &node) in nodes.iter().enumerate() {
            let r = root(&mut parents, node);
            parents[r] = first;
            diagonal[node] += fill * matrix[a][a];
        }
        cells.push(Cell { nodes, fill });
    }
    if diagonal.iter().any(|k| !k.is_finite()) {
        return Err("Thermal stiffness exceeds the numerical range.".into());
    }
    let mut sinks = vec![false; n];
    for i in 0..n {
        if fixed[i].is_some() || convection[i] > 0. {
            sinks[root(&mut parents, i)] = true;
        }
    }
    for cell in &cells {
        if !sinks[root(&mut parents, cell.nodes[0])] {
            return Err("A separate solid has no heat sink.".into());
        }
    }
    let apply = |v: &[f64], out: &mut [f64]| {
        for ((o, &k), &x) in out.iter_mut().zip(&convection).zip(v) {
            *o = k * x;
        }
        for c in &cells {
            let t = c.nodes.map(|i| v[i]);
            for (a, &node) in c.nodes.iter().enumerate() {
                out[node] += c.fill
                    * matrix[a]
                        .iter()
                        .zip(t)
                        .map(|(k, x)| k * (x - t[a]))
                        .sum::<f64>();
            }
        }
    };
    let mut x: Vec<_> = fixed
        .iter()
        .map(|t| t.map_or(0., |value| value - reference))
        .collect();
    let mut ax = vec![0.; n];
    apply(&x, &mut ax);
    let free: Vec<_> = (0..n)
        .filter(|&i| fixed[i].is_none() && diagonal[i] > 0.)
        .collect();
    let mut r = vec![0.; n];
    let mut d = vec![0.; n];
    for &i in &free {
        r[i] = rhs[i] - ax[i];
        d[i] = r[i] / diagonal[i];
    }
    let mut rz = free.iter().map(|&i| r[i] * d[i]).sum::<f64>();
    let initial = rz;
    let mut iterations = 0;
    while rz > initial * 1e-18 && iterations < 20_000 {
        apply(&d, &mut ax);
        let denom = free.iter().map(|&i| d[i] * ax[i]).sum::<f64>();
        if !(denom > 0. && denom.is_finite()) {
            return Err("The thermal system is singular.".into());
        }
        let alpha = rz / denom;
        let mut next = 0.;
        for &i in &free {
            x[i] += alpha * d[i];
            r[i] -= alpha * ax[i];
            next += r[i] * (r[i] / diagonal[i]);
        }
        for &i in &free {
            d[i] = r[i] / diagonal[i] + next / rz * d[i];
        }
        rz = next;
        iterations += 1;
        if !progress(iterations, (rz / initial).sqrt()) {
            return Err("Study stopped.".into());
        }
    }
    if !rz.is_finite() || rz > initial * 1e-18 {
        return Err("The thermal study did not converge.".into());
    }
    let mut min_c = f64::INFINITY;
    let mut max_c = f64::NEG_INFINITY;
    for (i, delta) in x.iter_mut().enumerate().filter(|(i, _)| diagonal[*i] > 0.) {
        *delta += reference;
        let v = *delta;
        if !(v as f32).is_finite() {
            return Err(format!(
                "Temperature at node {i} exceeds the display range."
            ));
        }
        min_c = min_c.min(v);
        max_c = max_c.max(v);
    }
    Ok(ThermalResult {
        temperature: x.into_iter().map(|v| v as f32).collect(),
        max_c,
        min_c,
        iterations,
    })
}
