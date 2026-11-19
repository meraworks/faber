//! Integrate surface loads in proportion to surface area.

use super::{Grid, Tri, VOID, bit, corners, index};
use faber_ports::Result;
use glam::DVec3;

const LIMIT: usize = 64_000_000;

fn budget_error(used: usize, needed: u128) -> String {
    let total = (used as u128).saturating_add(needed);
    format!(
        "The study needs at least {total} face samples. The limit is {LIMIT}. Use a larger resolution."
    )
}

pub(crate) struct Sampler<'a> {
    grid: &'a Grid,
    remaining: usize,
    samples: usize,
    progress: &'a mut dyn FnMut(usize, f64) -> bool,
}

impl<'a> Sampler<'a> {
    pub(crate) fn new(grid: &'a Grid, progress: &'a mut dyn FnMut(usize, f64) -> bool) -> Self {
        Self {
            grid,
            remaining: LIMIT,
            samples: 0,
            progress,
        }
    }

    pub(crate) fn face(
        &mut self,
        triangles: &[Tri],
        mut visit: impl FnMut(usize, f64, DVec3),
    ) -> Result<f64> {
        let mut area = 0.;
        let step = self.grid.h * 0.7;
        for tri in triangles {
            let vertices = tri.map(DVec3::from);
            let [a, b, c] = vertices;
            let cross = (b - a).cross(c - a);
            let twice = cross.length();
            if !twice.is_finite() {
                return Err("Invalid face coordinates.".into());
            }
            if twice == 0. {
                continue;
            }
            let normal = cross / twice;
            let length = (b - a).length().max((c - a).length()).max((c - b).length());
            let divisions = (length / step).ceil().max(1.);
            let slender = divisions > 16. && length > 16. * (twice / length);
            let bands = divisions as u128;
            let needed = if slender {
                bands
            } else {
                bands.saturating_mul(bands)
            };
            if needed > self.remaining as u128 {
                return Err(budget_error(self.samples, needed));
            }
            let mut sample = |point, area: f64| {
                if self.remaining == 0 {
                    return Err(budget_error(self.samples, 1));
                }
                self.remaining -= 1;
                self.samples += 1;
                if self.samples.is_multiple_of(256) && !(self.progress)(0, 1.) {
                    return Err("Study stopped.".into());
                }
                spread(self.grid, point, |node, weight| {
                    visit(node, area * weight, normal)
                })
            };
            if slender {
                strips(vertices, self.grid, &mut sample)?;
            } else {
                let div = divisions as usize;
                let patch = 0.5 * twice / (divisions * divisions);
                for row in 0..div {
                    for col in 0..div - row {
                        for offset in [1. / 3., 2. / 3.] {
                            if offset > 0.5 && row + col + 1 == div {
                                continue;
                            }
                            sample(
                                a + (b - a) * ((row as f64 + offset) / divisions)
                                    + (c - a) * ((col as f64 + offset) / divisions),
                                patch,
                            )?;
                        }
                    }
                }
            }
            area += 0.5 * twice;
        }
        if !(area > 0. && area.is_finite()) {
            return Err("The selected face has zero area.".into());
        }
        Ok(area)
    }
}

/// Cut the triangle at grid planes across its longest extent and at its middle vertex.
/// The strip width is linear, so each two-point Gauss rule preserves its area.
fn strips(
    mut vertices: [DVec3; 3],
    g: &Grid,
    sample: &mut impl FnMut(DVec3, f64) -> Result<()>,
) -> Result<()> {
    let min = vertices[0].min(vertices[1]).min(vertices[2]);
    let max = vertices[0].max(vertices[1]).max(vertices[2]);
    let axis = (max - min).max_position();
    vertices.sort_by(|a, b| a[axis].total_cmp(&b[axis]));
    let [a, b, c] = vertices;
    let slope = (c - a) / (c[axis] - a[axis]);
    let gauss = [0.5 - 0.5 / 3_f64.sqrt(), 0.5 + 0.5 / 3_f64.sqrt()];
    for (start, finish) in [(a, b), (b, c)] {
        if start[axis] == finish[axis] {
            continue;
        }
        let other = (finish - start) / (finish[axis] - start[axis]);
        let local_start = (start[axis] - g.origin[axis]) / g.h;
        let local_end = (finish[axis] - g.origin[axis]) / g.h;
        if !local_start.is_finite()
            || !local_end.is_finite()
            || local_start < -1.
            || local_end > g.n[axis] as f64 + 1.
        {
            return Err("The face does not touch the active grid.".into());
        }
        for plane in local_start.floor() as i64..local_end.ceil() as i64 {
            let left = local_start.max(plane as f64);
            let right = local_end.min((plane + 1) as f64);
            let span = (right - left) * g.h;
            let dx = (left - local_start) * g.h;
            // Follow the full spatial length when a short projected edge is steep.
            let bands = (span * slope.length().max(other.length()) / (g.h * 0.7))
                .ceil()
                .max(1.) as usize;
            let step = span / bands as f64;
            for band in 0..bands {
                for gx in gauss {
                    let t = dx + (band as f64 + gx) * step;
                    let front = a + slope * ((start[axis] - a[axis]) + t);
                    let across = start + other * t - front;
                    let lanes = (across.length() / (g.h * 0.7)).ceil().max(1.) as usize;
                    let weight = slope.cross(across).length() * step / (4. * lanes as f64);
                    for j in 0..lanes {
                        for gy in gauss {
                            sample(front + across * ((j as f64 + gy) / lanes as f64), weight)?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn spread(g: &Grid, point: DVec3, mut visit: impl FnMut(usize, f64)) -> Result<()> {
    let q = (point - DVec3::from(g.origin)) / g.h;
    let upper = DVec3::from(g.n.map(|v| v as f64)) + DVec3::ONE;
    if !q.is_finite() || q.cmplt(-DVec3::ONE).any() || q.cmpgt(upper).any() {
        return Err("The face does not touch the active grid.".into());
    }
    let base = q.floor().as_ivec3();
    let mut best = None;
    'search: for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let k = base + glam::IVec3::new(dx, dy, dz);
                if k.min_element() < 0 {
                    continue;
                }
                let p = k.to_array().map(|v| v as usize);
                if (0..3).any(|a| p[a] >= g.n[a]) {
                    continue;
                }
                let id = index(p, g.n);
                if g.fill[id] < VOID {
                    continue;
                }
                let local = q - k.as_dvec3();
                let frac = local.clamp(DVec3::ZERO, DVec3::ONE);
                let distance = local.distance_squared(frac);
                if best.is_none_or(|(_, _, d)| distance < d) {
                    best = Some((id, frac, distance));
                }
                if distance == 0. {
                    break 'search;
                }
            }
        }
    }
    let Some((id, frac, _)) = best.filter(|(_, _, d)| *d <= 1.) else {
        return Err("The face does not touch the active grid.".into());
    };

    for (i, node) in corners(g, id).into_iter().enumerate() {
        let bits = bit(i);
        let weight = (0..3)
            .map(|a| if bits[a] > 0 { frac[a] } else { 1. - frac[a] })
            .product::<f64>();
        if weight > 1e-10 {
            visit(node, weight)
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refuse(triangle: Tri, remaining: usize) -> String {
        let g = Grid {
            origin: [0.; 3],
            h: 1.,
            n: [30, 4, 1],
            fill: vec![1.; 120],
        };
        let mut run = |_, _| true;
        let mut sampler = Sampler::new(&g, &mut run);
        sampler.samples = LIMIT - remaining;
        sampler.remaining = remaining;
        sampler.face(&[triangle], |_, _, _| {}).unwrap_err()
    }

    #[test]
    fn face_sample_budget_names_the_required_count_and_limit() {
        let triangle = [[0., 0., 0.], [3., 0., 0.], [0., 4., 0.]];
        assert_eq!(
            refuse(triangle, 20),
            "The study needs at least 64000044 face samples. The limit is 64000000. Use a larger resolution."
        );
    }

    #[test]
    fn face_sample_budget_counts_streamed_strips_before_refusing() {
        let triangle = [[0., 0., 0.], [28., 0., 0.], [0., 1., 0.]];
        assert_eq!(
            refuse(triangle, 200),
            "The study needs at least 64000001 face samples. The limit is 64000000. Use a larger resolution."
        );
    }
}
