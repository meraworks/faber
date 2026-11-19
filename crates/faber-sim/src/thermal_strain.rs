//! Thermal strain and equivalent nodal forces for an elastic cell.
use crate::{Elastic, Grid, element::gradients, solve::corners};
use faber_ports::Result;
use glam::DVec3;

pub(crate) fn strain(
    field: &[f32],
    nodes: [usize; 8],
    reference: f64,
    expansion: f64,
    natural: [f64; 3],
) -> f64 {
    let mut temperature = 0.;
    for (i, id) in nodes.into_iter().enumerate() {
        let weight = (0..3)
            .map(|a| {
                if i & (1 << a) == 0 {
                    1. - natural[a]
                } else {
                    1. + natural[a]
                }
            })
            .product::<f64>()
            / 8.;
        temperature += weight * f64::from(field[id]);
    }
    expansion * 1e-6 * (temperature - reference)
}

pub(crate) fn add_forces(
    grid: &Grid,
    material: &Elastic,
    field: &[f32],
    reference: f64,
    expansion: f64,
    forces: &mut [DVec3],
) -> Result<()> {
    if field.len() != forces.len()
        || field.iter().any(|t| !t.is_finite())
        || !reference.is_finite()
        || !expansion.is_finite()
    {
        return Err(
            "Thermal strain needs one finite temperature per node and finite material data.".into(),
        );
    }
    let (lambda, mu) = material.lame();
    let bulk = 3. * lambda + 2. * mu;
    let a = 1. / 3_f64.sqrt();
    let points: Vec<_> = [-a, a]
        .into_iter()
        .flat_map(|x| {
            [-a, a]
                .into_iter()
                .flat_map(move |y| [-a, a].map(|z| [x, y, z]))
        })
        .map(|p| (p, gradients(grid.h, p)))
        .collect();
    for (id, &fill) in grid.fill.iter().enumerate().filter(|(_, v)| **v >= 0.02) {
        let nodes = corners(grid, id);
        let volume = f64::from(fill) * (grid.h / 2.).powi(3);
        for &(p, g) in &points {
            let pressure = bulk * strain(field, nodes, reference, expansion, p);
            for (node, gradient) in nodes.into_iter().zip(g) {
                forces[node] += gradient * (volume * pressure);
            }
        }
    }
    Ok(())
}
