//! Factor the shifted stiffness once when the active grid has a narrow band.
use crate::{
    Grid,
    solve::{Constraint, Row, VOID, corners, xyz},
};
use faber_ports::Result;
use glam::DVec3;

pub(super) struct Band {
    data: Vec<f64>,
    width: usize,
    slots: Vec<[usize; 3]>,
    n: usize,
}
const ABSENT: usize = usize::MAX;

impl Band {
    pub fn new(
        g: &Grid,
        rows: &[Row],
        bc: &[Constraint],
        element: &[[f64; 24]; 24],
        mass: &[f64],
        shift: f64,
        poll: &mut dyn FnMut() -> bool,
    ) -> Result<Option<Self>> {
        if rows.len() > 60_000 {
            return Ok(None);
        }
        let dims = g.n.map(|n| n + 1);
        let mut axes = [0, 1, 2];
        axes.sort_by_key(|&a| g.n[a]);
        let mut order: Vec<_> = (0..rows.len()).collect();
        order.sort_unstable_by_key(|&i| {
            let p = xyz(rows[i].node, dims);
            [p[axes[2]], p[axes[1]], p[axes[0]]]
        });
        let mut slots = vec![[ABSENT; 3]; rows.len()];
        let mut global = vec![ABSENT; dims.iter().product()];
        let mut n = 0;
        for i in order {
            global[rows[i].node] = i;
            for (j, axis) in [DVec3::X, DVec3::Y, DVec3::Z].into_iter().enumerate() {
                let d = bc[i].project(axis);
                if d.length_squared() < 1e-24 {
                    continue;
                }
                if d.distance_squared(axis) > 1e-24 {
                    return Ok(None);
                }
                slots[i][j] = n;
                n += 1;
            }
        }
        let cell_slots = |cell| corners(g, cell).map(|node| slots[global[node]]);
        let mut width = 1;
        for (cell, _) in g.fill.iter().enumerate().filter(|(_, f)| **f >= VOID) {
            let ids = cell_slots(cell);
            let mut min = n;
            let mut max = 0;
            for &i in ids.iter().flatten().filter(|&&k| k != ABSENT) {
                min = min.min(i);
                max = max.max(i);
            }
            if min < n {
                width = width.max(max - min + 1);
            }
        }
        if n == 0 || n * width > 8_000_000 || n as u64 * width as u64 * width as u64 > 2_400_000_000
        {
            return Ok(None);
        }
        let mut data = vec![0.; n * width];
        for (cell, &fill) in g.fill.iter().enumerate().filter(|(_, f)| **f >= VOID) {
            let ids = cell_slots(cell);
            for p in 0..24 {
                let i = ids[p / 3][p % 3];
                if i == ABSENT {
                    continue;
                }
                for q in 0..=p {
                    let j = ids[q / 3][q % 3];
                    if j == ABSENT {
                        continue;
                    }
                    let hi = i.max(j);
                    data[hi * width + width - 1 - i.abs_diff(j)] += f64::from(fill) * element[p][q];
                }
            }
        }
        for (i, ids) in slots.iter().enumerate() {
            for &id in ids.iter().filter(|&&i| i != ABSENT) {
                data[(id + 1) * width - 1] += mass[i] * shift;
            }
        }
        for i in 0..n {
            if i % 128 == 0 && !poll() {
                return Err("Study stopped.".into());
            }
            let diag = (i + 1) * width - 1;
            let start = i.saturating_sub(width - 1);
            for j in start..i {
                let k = diag - (i - j);
                let jd = (j + 1) * width - 1;
                let mut sum = 0.;
                for t in 1..=j - start {
                    sum += data[k - t] * data[jd - t];
                }
                data[k] = (data[k] - sum) / data[jd];
            }
            let mut d = data[diag];
            for &v in &data[diag - (i - start)..diag] {
                d -= v * v;
            }
            if d <= 0. || !d.is_finite() {
                return Err("The shifted modal stiffness is singular.".into());
            }
            data[diag] = d.sqrt();
        }
        Ok(Some(Self {
            data,
            width,
            slots,
            n,
        }))
    }
    pub fn solve(&self, rhs: &[DVec3]) -> Vec<DVec3> {
        let w = self.width;
        let mut x = vec![0.; self.n];
        for (b, ids) in rhs.iter().zip(&self.slots) {
            for (j, &i) in ids.iter().enumerate() {
                if i != ABSENT {
                    x[i] = b[j];
                }
            }
        }
        for i in 0..self.n {
            let d = (i + 1) * w - 1;
            let begin = i.saturating_sub(w - 1);
            let mut sum = x[i];
            for (&coefficient, &value) in self.data[d - (i - begin)..d].iter().zip(&x[begin..i]) {
                sum -= coefficient * value;
            }
            x[i] = sum / self.data[d];
        }
        for i in (0..self.n).rev() {
            let d = (i + 1) * w - 1;
            x[i] /= self.data[d];
            let v = x[i];
            let begin = i.saturating_sub(w - 1);
            for (&coefficient, value) in self.data[d - (i - begin)..d].iter().zip(&mut x[begin..i])
            {
                *value -= coefficient * v;
            }
        }
        self.slots
            .iter()
            .map(|ids| DVec3::from(ids.map(|i| if i == ABSENT { 0. } else { x[i] })))
            .collect()
    }
}
