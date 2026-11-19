use crate::residual::Row;

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}

/// Solve the damped normal equations by Cholesky factorisation.
pub fn step(
    rows: &[Row],
    n: usize,
    lambda: f64,
    at: &[f64],
    pull: Option<(usize, [f64; 2], f64)>,
) -> Option<Vec<f64>> {
    let mut a = vec![0.; n * n];
    let mut rhs = vec![0.; n];
    for r in rows {
        for &(i, g) in &r.value.d {
            rhs[i] -= g * r.value.f;
            for &(j, h) in &r.value.d {
                if j <= i {
                    a[i * n + j] += g * h;
                }
            }
        }
    }
    if let Some((i, target, weight)) = pull {
        for j in 0..2 {
            a[(i + j) * n + i + j] += weight;
            rhs[i + j] += weight * (target[j] - at[i + j]);
        }
    }
    for i in 0..n {
        a[i * n + i] += lambda;
    }
    // A sparse row of the normal equations often keeps its leading zeros.
    let mut first = vec![0; n];
    for i in 0..n {
        let row = i * n;
        first[i] = (0..i).find(|&j| a[row + j] != 0.).unwrap_or(i);
        for j in first[i]..i {
            let lo = first[i].max(first[j]);
            let col = j * n;
            a[row + j] =
                (a[row + j] - dot(&a[row + lo..row + j], &a[col + lo..col + j])) / a[col + j];
        }
        let diag = a[row + i] - dot(&a[row + first[i]..row + i], &a[row + first[i]..row + i]);
        if diag <= 0. || !diag.is_finite() {
            return None;
        }
        a[row + i] = diag.sqrt();
        rhs[i] = (rhs[i] - dot(&a[row + first[i]..row + i], &rhs[first[i]..i])) / a[row + i];
    }
    for i in (0..n).rev() {
        let row = i * n;
        rhs[i] /= a[row + i];
        for j in first[i]..i {
            rhs[j] -= a[row + j] * rhs[i];
        }
    }
    rhs.iter().all(|v| v.is_finite()).then_some(rhs)
}

pub struct Rank {
    pub count: usize,
    pub movable: Vec<bool>,
    pub redundant: Vec<usize>,
}
/// Column-pivoted QR with a second projection for nearly dependent columns.
pub fn rank(rows: &[Row], n: usize, groups: usize) -> Rank {
    let m = rows.len();
    let mut col = vec![vec![0.; m]; n];
    let mut which = vec![Vec::new(); groups];
    for (i, row) in rows.iter().enumerate() {
        for &(j, d) in &row.value.d {
            col[j][i] = d;
        }
        if let Some(group) = row.constraint {
            which[group].push(i);
        }
    }
    let mut norms: Vec<_> = col.iter().map(|c| dot(c, c)).collect();
    let threshold = norms.iter().copied().fold(0., f64::max) * 1e-18;
    let mut available = vec![true; n];
    let mut pivots = Vec::new();
    let mut r: Vec<Vec<f64>> = Vec::new();
    let mut q: Vec<Vec<f64>> = Vec::new();
    for _ in 0..n.min(m) {
        let Some(j) = (0..n)
            .filter(|&j| available[j])
            .max_by(|&a, &b| norms[a].total_cmp(&norms[b]).then(b.cmp(&a)))
        else {
            break;
        };
        if norms[j] <= threshold {
            break;
        }
        available[j] = false;
        pivots.push(j);
        let len = norms[j].sqrt();
        let mut v = std::mem::take(&mut col[j]);
        for a in &mut v {
            *a /= len;
        }
        // Sparse columns are common when the constraints act on separate points.
        let support: Vec<_> = v
            .iter()
            .enumerate()
            .filter(|(_, x)| **x != 0.)
            .map(|(i, &x)| (i, x))
            .collect();
        let mut rr = vec![0.; n];
        rr[j] = len;
        for k in 0..n {
            if !available[k] {
                continue;
            }
            let u = &mut col[k];
            let mut proj = 0.;
            for _ in 0..2 {
                let d = support.iter().map(|&(i, a)| a * u[i]).sum::<f64>();
                if d == 0. {
                    break;
                }
                for &(i, a) in &support {
                    u[i] -= d * a;
                }
                proj += d;
            }
            rr[k] = proj;
            if proj != 0. {
                norms[k] = dot(u, u);
            }
        }
        q.push(v);
        r.push(rr);
    }
    let mut movable = available.clone();
    for f in (0..n).filter(|&j| available[j]) {
        let mut z = vec![0.; pivots.len()];
        for (k, &p) in pivots.iter().enumerate().rev() {
            let influence = (k + 1..pivots.len())
                .map(|j| r[k][pivots[j]] * z[j])
                .sum::<f64>();
            z[k] = (-r[k][f] - influence) / r[k][p];
            if z[k].abs() > 1e-9 {
                movable[p] = true;
            }
        }
    }
    let mut redundant = Vec::new();
    if pivots.len() < m {
        // Project each constraint's rows onto the left null space. It can be
        // removed when this small block has full row rank.
        let projection =
            |a: usize, b: usize| f64::from(a == b) - q.iter().map(|v| v[a] * v[b]).sum::<f64>();
        for (i, group) in which.iter().enumerate() {
            let removable = match group.as_slice() {
                [a] => projection(*a, *a) > 1e-10,
                [a, b] => {
                    let x = projection(*a, *a);
                    let y = projection(*b, *b);
                    let k = projection(*a, *b);
                    x * y - k * k > 1e-10 * x.max(y)
                }
                _ => false,
            };
            if removable {
                redundant.push(i);
            }
        }
    }
    Rank {
        count: pivots.len(),
        movable,
        redundant,
    }
}
