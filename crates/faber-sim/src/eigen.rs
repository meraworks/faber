//! Jacobi diagonalisation of the small symmetric Ritz matrix.

/// Return ascending eigenvalues and their corresponding unit vectors.
pub(super) fn symmetric_eigen(mut a: Vec<Vec<f64>>) -> (Vec<f64>, Vec<Vec<f64>>) {
    let n = a.len();
    let mut vectors = vec![vec![0.; n]; n];
    let mut p_row = vec![0.; n];
    let mut q_row = vec![0.; n];
    for (i, v) in vectors.iter_mut().enumerate() {
        v[i] = 1.;
    }
    for _ in 0..64 {
        let scale = (0..n).map(|i| a[i][i].abs()).fold(0., f64::max);
        let mut changed = false;
        for p in 0..n {
            for q in p + 1..n {
                let off = a[p][q];
                if off.abs() <= 1e-14 * scale {
                    continue;
                }
                changed = true;
                let tau = (a[q][q] - a[p][p]) / (2. * off);
                let t = 1_f64.copysign(tau) / (tau.abs() + tau.hypot(1.));
                let c = 1. / t.hypot(1.);
                let s = t * c;
                a[p][p] -= t * off;
                a[q][q] += t * off;
                a[p][q] = 0.;
                a[q][p] = 0.;
                for (i, row) in a.iter_mut().enumerate() {
                    if i != p && i != q {
                        let (u, v) = (row[p], row[q]);
                        row[p] = c * u - s * v;
                        row[q] = s * u + c * v;
                    }
                    p_row[i] = row[p];
                    q_row[i] = row[q];
                }
                a[p].copy_from_slice(&p_row);
                a[q].copy_from_slice(&q_row);
                let (left, right) = vectors.split_at_mut(q);
                for (x, y) in left[p].iter_mut().zip(&mut right[0]) {
                    let (u, v) = (*x, *y);
                    *x = c * u - s * v;
                    *y = s * u + c * v;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut order: Vec<_> = (0..n).collect();
    order.sort_by(|&i, &j| a[i][i].total_cmp(&a[j][j]));
    (
        order.iter().map(|&i| a[i][i]).collect(),
        order
            .into_iter()
            .map(|i| std::mem::take(&mut vectors[i]))
            .collect(),
    )
}
