use crate::{
    Result, Sketch, dense,
    residual::{Problem, Row},
};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Solved,
    Under,
    Over,
}

#[derive(Clone, Debug, Serialize)]
pub struct Solution {
    pub points: BTreeMap<String, [f64; 2]>,
    pub radii: BTreeMap<String, f64>,
    pub status: Status,
    pub dof: usize,
    pub free_points: Vec<String>,
    /// Circle radii that can change while the constraints hold.
    pub free_radii: Vec<String>,
    pub redundant: Vec<usize>,
    pub conflicting: Vec<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnosis_limit: Option<String>,
    pub iterations: usize,
    pub residual: f64,
}
const TOL: f64 = 1e-9;
const MAX_ITERATIONS: usize = 100;
type Pull = Option<(usize, [f64; 2], f64)>;

struct Fit {
    x: Vec<f64>,
    rows: Vec<Row>,
    iterations: usize,
    exhausted: bool,
}
impl Fit {
    fn worst(&self) -> f64 {
        self.rows.iter().map(|r| r.value.f.abs()).fold(0., f64::max)
    }
    fn failure(&self) -> String {
        let worst = self
            .rows
            .iter()
            .max_by(|a, b| a.value.f.abs().total_cmp(&b.value.f.abs()));
        let label = worst
            .and_then(|r| r.constraint)
            .map_or_else(|| "an arc radius".into(), |i| format!("constraint {i}"));
        format!(
            "The sketch did not converge. The largest residual is {:.12} mm at {label}.",
            self.worst()
        )
    }
}
fn cost(rows: &[Row], x: &[f64], pull: Pull) -> f64 {
    let p = pull.map_or(0., |(i, t, w)| {
        w * ((x[i] - t[0]).powi(2) + (x[i + 1] - t[1]).powi(2))
    });
    rows.iter().fold(p, |a, r| a + r.value.f * r.value.f)
}
fn fit(
    p: &Problem<'_>,
    mut x: Vec<f64>,
    skip: Option<usize>,
    pull: Pull,
    limit: usize,
) -> Result<Fit> {
    let mut rows = p.rows(&x, skip)?;
    let mut error = cost(&rows, &x, pull);
    let mut lambda = 1e-4;
    let mut iterations = 0;
    while iterations < limit {
        if rows.iter().all(|r| r.value.f.abs() < 1e-11)
            && pull.is_none_or(|(i, t, _)| (t[0] - x[i]).hypot(t[1] - x[i + 1]) < 1e-11)
        {
            break;
        }
        iterations += 1;
        if let Some(dx) = dense::step(&rows, x.len(), lambda, &x, pull) {
            let trial: Vec<_> = x.iter().zip(&dx).map(|(v, d)| v + d).collect();
            if trial.iter().all(|a| a.is_finite())
                && p.circles.values().all(|&i| trial[i] > 0.)
                && let Ok(candidate) = p.rows(&trial, skip)
            {
                let next = cost(&candidate, &trial, pull);
                if next < error {
                    let change = error - next;
                    x = trial;
                    rows = candidate;
                    error = next;
                    lambda = (lambda * 0.2).max(1e-15);
                    if change <= error * 1e-14 {
                        break;
                    }
                    continue;
                }
            }
            if dx.iter().all(|d| d.abs() < 1e-13) {
                break;
            }
        }
        lambda *= 10.;
        if lambda > 1e15 {
            break;
        }
    }
    Ok(Fit {
        x,
        rows,
        iterations,
        exhausted: iterations == limit,
    })
}

pub fn solve(sketch: &Sketch) -> Result<Solution> {
    let p = Problem::new(sketch)?;
    let f = fit(&p, p.start.clone(), None, None, MAX_ITERATIONS)?;
    finish(&p, f)
}

/// Pull one point, then project the result onto the hard constraints.
pub fn drag(sketch: &Sketch, point: &str, target: [f64; 2]) -> Result<Solution> {
    let p = Problem::new(sketch)?;
    let i = p.index(point)?;
    if !target.iter().all(|v| v.is_finite()) {
        return Err("The drag target must be finite.".into());
    }
    let baseline = fit(&p, p.start.clone(), None, None, MAX_ITERATIONS)?;
    if baseline.worst() >= TOL
        || (baseline.x[i] - target[0]).hypot(baseline.x[i + 1] - target[1]) <= 1e-12
    {
        return finish(&p, baseline);
    }
    let available = MAX_ITERATIONS - baseline.iterations;
    let soft = fit(&p, baseline.x, None, Some((i, target, 1.)), available / 2)?;
    let mut f = fit(&p, soft.x, None, None, available - soft.iterations)?;
    f.iterations += soft.iterations + baseline.iterations;
    finish(&p, f)
}

fn finish(p: &Problem<'_>, f: Fit) -> Result<Solution> {
    let residual = f.worst();
    if residual >= TOL && f.exhausted {
        return Err(f.failure());
    }
    if residual < TOL {
        p.validate_result(&f.x)?;
    }
    let mut conflicting = vec![];
    let mut diagnosis_limit = None;
    if residual >= TOL {
        // Only a constraint connected to a violated equation can restore consistency.
        let mut active = vec![false; f.x.len()];
        for r in &f.rows {
            if r.value.f.abs() >= TOL {
                for &(i, _) in &r.value.d {
                    active[i] = true;
                }
            }
        }
        loop {
            let mut changed = false;
            for r in &f.rows {
                if r.value.d.iter().any(|&(i, _)| active[i]) {
                    for &(i, _) in &r.value.d {
                        if !active[i] {
                            active[i] = true;
                            changed = true;
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        use crate::Constraint::*;
        let affine = p
            .sketch
            .entities
            .values()
            .all(|e| !matches!(e, crate::Entity::Arc { .. }))
            && p.sketch.constraints.iter().all(|c| {
                matches!(
                    c,
                    Fix { .. }
                        | Coincident { .. }
                        | Horizontal { .. }
                        | Vertical { .. }
                        | DistanceX { .. }
                        | DistanceY { .. }
                        | Midpoint { .. }
                )
            });
        let mut candidates = vec![false; p.sketch.constraints.len()];
        let mut errors = vec![0_f64; p.sketch.constraints.len()];
        for r in &f.rows {
            if let Some(i) = r.constraint {
                errors[i] = errors[i].max(r.value.f.abs());
                // Removing a satisfied affine row cannot change the least-squares optimum.
                if r.value.f.abs() >= TOL || (!affine && r.value.d.iter().any(|&(i, _)| active[i]))
                {
                    candidates[i] = true;
                }
            }
        }
        let mut ordered: Vec<_> = candidates
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, active)| **active)
            .map(|(i, _)| i)
            .filter(|&i| {
                !p.sketch
                    .constraints
                    .iter()
                    .enumerate()
                    .any(|(j, c)| i != j && *c == p.sketch.constraints[i])
            })
            .collect();
        let limit = (320_000_000 / f.x.len().max(1).pow(3)).clamp(1, 64);
        if ordered.len() > limit {
            diagnosis_limit = Some(format!(
                "Conflict isolation checks {limit} of {} candidate constraints. The conflict list can be incomplete.",
                ordered.len()
            ));
            ordered.sort_by(|&a, &b| errors[b].total_cmp(&errors[a]).then(b.cmp(&a)));
        }
        for &i in ordered.iter().take(limit) {
            if fit(p, f.x.clone(), Some(i), None, MAX_ITERATIONS)?.worst() < TOL {
                conflicting.push(i);
            }
        }
        if conflicting.is_empty() {
            conflicting.extend(
                f.rows
                    .iter()
                    .filter(|r| r.value.f.abs() >= TOL)
                    .filter_map(|r| r.constraint),
            );
        }
        conflicting.sort_unstable();
        conflicting.dedup();
    }
    let rank = dense::rank(&f.rows, f.x.len(), p.sketch.constraints.len());
    let dof = f.x.len() - rank.count;
    let status = if residual >= TOL {
        Status::Over
    } else if dof == 0 {
        Status::Solved
    } else {
        Status::Under
    };
    let points = p
        .points
        .iter()
        .map(|(&id, &i)| (id.to_owned(), [f.x[i], f.x[i + 1]]))
        .collect();
    let free_points = p
        .points
        .iter()
        .filter(|(_, i)| rank.movable[**i] || rank.movable[**i + 1])
        .map(|(&id, _)| id.to_string())
        .collect();
    Ok(Solution {
        points,
        radii: p.radii(&f.x)?,
        status,
        dof,
        free_points,
        free_radii: p
            .circles
            .iter()
            .filter(|(_, i)| rank.movable[**i])
            .map(|(&id, _)| id.to_owned())
            .collect(),
        redundant: rank.redundant,
        conflicting,
        diagnosis_limit,
        iterations: f.iterations,
        residual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Constraint;
    #[test]
    fn no_solution_names_the_worst_constraint() {
        let mut sketch = Sketch::default();
        sketch.points.insert("a".into(), [0., 0.]);
        sketch.points.insert("b".into(), [1., 0.]);
        sketch.constraints.push(Constraint::DistanceX {
            a: "a".into(),
            b: "b".into(),
            value: 24.,
        });
        let p = Problem::new(&sketch).unwrap();
        let err = finish(&p, fit(&p, p.start.clone(), None, None, 1).unwrap()).unwrap_err();
        assert!(
            err.contains("largest residual") && err.contains("mm at constraint 0"),
            "{err}"
        );
    }
}
