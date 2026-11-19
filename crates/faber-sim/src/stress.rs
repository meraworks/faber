use crate::{
    Grid,
    element::gradients,
    solve::{Elastic, corners},
};
use faber_ports::Result;
use glam::DVec3;

fn mises([x, y, z, xy, yz, zx]: [f64; 6]) -> f64 {
    (0.5 * ((x - y).powi(2) + (y - z).powi(2) + (z - x).powi(2))
        + 3. * (xy * xy + yz * yz + zx * zx))
        .sqrt()
}

/// Recover cell-centre stresses, average at each node and map them to the cells.
pub(crate) fn recover(
    g: &Grid,
    m: &Elastic,
    u: &[[f64; 3]],
    loads: &[crate::Load],
) -> Result<(Vec<f32>, Vec<[f64; 6]>)> {
    if !g.fill.iter().any(|&f| f >= 0.5) {
        return Err("No cell is half full. Use a smaller resolution.".into());
    }
    let grad = gradients(g.h, [0.; 3]);
    let (lambda, mu) = m.lame();
    let mut sums = vec![[0.; 6]; u.len()];
    let mut weights = vec![0.; u.len()];
    for (id, &fill) in g.fill.iter().enumerate().filter(|(_, f)| **f >= 0.02) {
        let nodes = corners(g, id);
        let mut du = [DVec3::ZERO; 3];
        for (node, gradient) in nodes.iter().zip(grad) {
            for a in 0..3 {
                du[a] += u[*node][a] * gradient
            }
        }
        for load in loads {
            if let crate::Load::Temperature {
                field,
                reference_c,
                expansion_um_mk,
            } = load
            {
                let strain = crate::thermal_strain::strain(
                    field,
                    nodes,
                    *reference_c,
                    *expansion_um_mk,
                    [0.; 3],
                );
                for (axis, row) in du.iter_mut().enumerate() {
                    row[axis] -= strain;
                }
            }
        }
        let trace = lambda * (du[0].x + du[1].y + du[2].z);
        let stress = [
            trace + 2. * mu * du[0].x,
            trace + 2. * mu * du[1].y,
            trace + 2. * mu * du[2].z,
            mu * (du[0].y + du[1].x),
            mu * (du[1].z + du[2].y),
            mu * (du[2].x + du[0].z),
        ];
        // Each corner receives one eighth of the material volume of its cell.
        let weight = f64::from(fill);
        for n in nodes {
            for (sum, val) in sums[n].iter_mut().zip(stress) {
                *sum += weight * val
            }
            weights[n] += weight;
        }
    }
    for (s, weight) in sums.iter_mut().zip(weights) {
        if weight > 0. {
            for val in s {
                *val /= weight
            }
        }
    }
    let node_mises: Vec<_> = sums.iter().map(|&s| mises(s)).collect();
    let mut von_mises = vec![0f32; g.fill.len()];
    let mut tensors = vec![[0.; 6]; g.fill.len()];
    for (id, _) in g.fill.iter().enumerate().filter(|(_, f)| **f >= 0.5) {
        for node in corners(g, id) {
            von_mises[id] += (node_mises[node] / 8.) as f32;
            for (dst, val) in tensors[id].iter_mut().zip(sums[node]) {
                *dst += val / 8.;
            }
        }
        if !von_mises[id].is_finite() {
            return Err("Stress exceeds the numeric range.".into());
        }
    }
    if g.fill.iter().any(|f| (0.02..0.5).contains(f)) {
        let sources = nearest_sources(g)?;
        for (id, &source) in sources.iter().enumerate() {
            if (0.02..0.5).contains(&g.fill[id]) {
                tensors[id] = tensors[source as usize];
                von_mises[id] = von_mises[source as usize];
            }
        }
    }
    Ok((von_mises, tensors))
}

// Each source gives (x - at)^2 + base along a grid line.
struct Site {
    source: u32,
    at: i64,
    base: i64,
    start: i64,
}

impl Site {
    fn first_before(&self, old: &Self) -> i64 {
        let numerator = self.base + self.at * self.at - old.base - old.at * old.at;
        let denominator = 2 * (self.at - old.at);
        // At equal distance, use the source with the smaller cell index.
        let strict = self.source >= old.source || numerator.rem_euclid(denominator) != 0;
        numerator.div_euclid(denominator) + i64::from(strict)
    }
}

// Compute exact squared Euclidean distance in three linear passes.
fn nearest_sources(g: &Grid) -> Result<Vec<u32>> {
    const NONE: u32 = u32::MAX;
    let mut source: Vec<u32> = g
        .fill
        .iter()
        .enumerate()
        .map(|(i, &f)| if f >= 0.5 { i as u32 } else { NONE })
        .collect();
    if source.iter().all(|s| *s == NONE) {
        return Err("No cell is half full. Use a smaller resolution.".into());
    }
    let mut distance = vec![0i64; source.len()];
    let mut sites = Vec::<Site>::new();
    let mut stride = 1;
    for length in g.n {
        if length > 1 {
            let block = stride * length;
            for start in (0..source.len()).step_by(block) {
                for first in start..start + stride {
                    sites.clear();
                    for x in 0..length {
                        let id = first + x * stride;
                        if source[id] == NONE {
                            continue;
                        }
                        let mut next = Site {
                            source: source[id],
                            at: x as i64,
                            base: distance[id],
                            start: 0,
                        };
                        // Keep only the intervals where a source is nearest.
                        while let Some(old) = sites.last() {
                            let first_x = next.first_before(old);
                            if first_x > old.start {
                                next.start = first_x;
                                break;
                            }
                            sites.pop();
                        }
                        if next.start < length as i64 {
                            sites.push(next);
                        }
                    }
                    if sites.is_empty() {
                        continue;
                    }
                    let mut active = 0;
                    for x in 0..length {
                        if active + 1 < sites.len() && sites[active + 1].start <= x as i64 {
                            active += 1;
                        }
                        let best = &sites[active];
                        let id = first + x * stride;
                        source[id] = best.source;
                        distance[id] = best.base + (x as i64 - best.at).pow(2);
                    }
                }
            }
        }
        stride *= length;
    }
    Ok(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solve::xyz;

    #[test]
    fn nearest_cells_match_squared_distance_and_break_ties_by_index() {
        let n = [7, 6, 5];
        let mut fill = vec![0.25; n.iter().product()];
        let sources = [2, 6, 19, 37, 76, 189];
        for &s in &sources {
            fill[s] = 0.5;
        }
        let g = Grid {
            n,
            fill,
            h: 1.,
            origin: [0.; 3],
        };
        let actual = nearest_sources(&g).unwrap();
        for (i, &found) in actual.iter().enumerate() {
            let p = xyz(i, n);
            let expected = sources
                .iter()
                .copied()
                .min_by_key(|&s| {
                    let q = xyz(s, n);
                    ((0..3).map(|d| p[d].abs_diff(q[d]).pow(2)).sum::<usize>(), s)
                })
                .unwrap();
            assert_eq!(found as usize, expected, "cell {i}");
        }
    }

    #[test]
    fn long_partial_rib_uses_the_closest_solid_cell() {
        let mut g = Grid {
            n: [80_001, 1, 1],
            fill: vec![0.25; 80_001],
            h: 1.,
            origin: [0.; 3],
        };
        assert!(nearest_sources(&g).is_err());
        g.fill[0] = 1.;
        g.fill[80_000] = 1.;
        let found = nearest_sources(&g).unwrap();
        assert!(found[..40_001].iter().all(|&s| s == 0));
        assert!(found[40_001..].iter().all(|&s| s == 80_000));
    }

    #[test]
    fn adjacent_cells_contribute_their_solid_volume_to_the_node_stress() {
        let g = Grid {
            origin: [0.; 3],
            h: 1.,
            n: [2, 1, 1],
            fill: vec![1., 0.5],
        };
        let m = Elastic {
            youngs_mpa: 1000.,
            poisson: 0.,
        };
        let mut u = vec![[0.; 3]; 12];
        for z in 0..=1 {
            for y in 0..=1 {
                for (x, value) in [0., 0.01, 0.04].into_iter().enumerate() {
                    u[g.node(x, y, z)][0] = value;
                }
            }
        }
        let (equivalent, stress) = recover(&g, &m, &u, &[]).unwrap();
        // The shared node has (10 + 0.5 * 30) / 1.5 MPa.
        let expected = (10. + 25. / 1.5) * 0.5;
        assert!((stress[0][0] - expected).abs() < 1e-10, "{}", stress[0][0]);
        assert!((f64::from(equivalent[0]) - expected).abs() < 1e-5);
    }
}
