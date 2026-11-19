//! A regular grid with one empty cell around the part.

use faber_geometry::bvh::Bvh;
use faber_ports::{Mesh, Result};
use glam::DVec3;
use rayon::prelude::*;

pub const CELL_LIMIT: usize = 4_000_000;

#[derive(Clone, Debug)]
pub struct Grid {
    pub origin: [f64; 3],
    pub h: f64,
    pub n: [usize; 3],
    /// The solid fraction of each cell. The X index changes fastest.
    pub fill: Vec<f32>,
}

impl Grid {
    pub fn volume(&self) -> f64 {
        self.fill.iter().map(|&f| f64::from(f)).sum::<f64>() * self.h.powi(3)
    }

    pub fn node(&self, i: usize, j: usize, k: usize) -> usize {
        i + (self.n[0] + 1) * (j + (self.n[1] + 1) * k)
    }
}

pub fn default_h(bounds: [[f64; 3]; 2]) -> f64 {
    let span = DVec3::from(bounds[1]) - DVec3::from(bounds[0]);
    ((span.x * span.y * span.z / 150_000.).cbrt()).clamp(0.2, 10.)
}

pub fn voxelise(mesh: &Mesh, bvh: &Bvh, h: f64) -> Result<Grid> {
    voxelise_with_progress(mesh, bvh, h, |_, _| true)
}

/// Report the number of finished cells on the calling thread. False stops the scan.
pub fn voxelise_with_progress(
    mesh: &Mesh,
    bvh: &Bvh,
    h: f64,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<Grid> {
    if !h.is_finite() || h <= 0. {
        return Err("The resolution must be finite and greater than zero.".into());
    }
    let mut low = DVec3::splat(f64::INFINITY);
    let mut high = DVec3::splat(f64::NEG_INFINITY);
    for (index, &v) in mesh.triangles.iter().flatten().enumerate() {
        if index.is_multiple_of(65_536) && !progress(0, 0) {
            return Err("Study stopped.".into());
        }
        let point = mesh
            .vertices
            .get(v as usize)
            .ok_or("The mesh has an invalid triangle.")?;
        let p = DVec3::from(*point);
        if !p.is_finite() {
            return Err("Mesh coordinates must be finite.".into());
        }
        low = low.min(p);
        high = high.max(p);
    }
    let span = high - low;
    if !span.is_finite() || span.min_element() <= 0. {
        return Err("The mesh must enclose a volume.".into());
    }
    let shape = span.to_array().map(|s| (s / h).ceil() as u128);
    let padded = shape.map(|n| n.saturating_add(2));
    for dimensions in [shape, padded] {
        let count = dimensions.into_iter().fold(1u128, u128::saturating_mul);
        if count > CELL_LIMIT as u128 {
            return Err(format!(
                "The grid has {count} cells. The limit is {CELL_LIMIT}. Use a larger resolution."
            ));
        }
    }
    let n = padded.map(|s| s as usize);
    let origin = (low - h).to_array();
    let mut fill = vec![0.; n.iter().product()];
    let [nx, ny, nz] = n;
    let total = fill.len();
    let rows_per_batch = (16_384 / nx).max(1);
    if !progress(0, total) {
        return Err("Study stopped.".into());
    }
    let mut completed = 0;
    for batch in fill.chunks_mut(rows_per_batch * nx) {
        let first_row = completed / nx;
        batch
            .par_chunks_exact_mut(nx)
            .enumerate()
            .for_each(|(row_index, row)| {
                let index = first_row + row_index;
                let (j, k) = (index % ny, index / ny);
                if j == 0 || j + 1 == ny || k == 0 || k + 1 == nz {
                    return;
                }
                for sz in [1. / 6., 0.5, 5. / 6.] {
                    for sy in [1. / 6., 0.5, 5. / 6.] {
                        // Move the ray a small amount to avoid mesh edges at sample points.
                        let y = origin[1] + h * (j as f64 + sy + 1.9e-10);
                        let z = origin[2] + h * (k as f64 + sz + 3.1e-10);
                        let hits = bvh.hits([origin[0], y, z], [1., 0., 0.]);
                        if hits.is_empty() {
                            continue;
                        }
                        let mut crossed = 0;
                        for (i, count) in row.iter_mut().enumerate().take(nx - 1).skip(1) {
                            for sx in [1. / 6., 0.5, 5. / 6.] {
                                let t = h * (i as f64 + sx);
                                while crossed < hits.len() && hits[crossed].t < t {
                                    crossed += 1;
                                }
                                let inside = if hits.len().is_multiple_of(2) {
                                    crossed % 2 == 1
                                } else {
                                    // A tangent ray can touch the surface only once.
                                    bvh.inside([origin[0] + t, y, z])
                                };
                                if inside {
                                    *count += 1.;
                                }
                            }
                        }
                    }
                }
                for f in row {
                    // The smallest positive fraction is 1/27, above the 0.02 limit.
                    *f /= 27.;
                }
            });
        completed += batch.len();
        if !progress(completed, total) {
            return Err("Study stopped.".into());
        }
    }
    Ok(Grid { origin, h, n, fill })
}

/// Find the active cells that touch at least one triangle of the selected face.
pub fn cells_near(grid: &Grid, triangles: &[[[f64; 3]; 3]]) -> Vec<usize> {
    let mut touched = vec![false; grid.fill.len()];
    let origin = DVec3::from(grid.origin);
    let tolerance = 1e-9;
    for triangle in triangles {
        let p = triangle.map(|v| (DVec3::from(v) - origin) / grid.h);
        let edges = [p[1] - p[0], p[2] - p[1], p[0] - p[2]];
        let normal = edges[0].cross(edges[1]);
        if !normal.is_finite() || normal.length_squared() == 0. {
            continue;
        }
        let low = (p[0].min(p[1]).min(p[2]) - tolerance).ceil() - 1.;
        let high = (p[0].max(p[1]).max(p[2]) + tolerance).floor();
        let first = low.to_array().map(|v| v as usize);
        let end: [usize; 3] = std::array::from_fn(|i| ((high[i] + 1.) as usize).min(grid.n[i]));
        // The index ranges test the three box axes. Test the other ten axes here.
        let axes: Vec<_> = std::iter::once(normal)
            .chain(
                edges
                    .iter()
                    .flat_map(|e| [DVec3::X, DVec3::Y, DVec3::Z].map(|a| e.cross(a))),
            )
            .filter(|a| a.length_squared() > 0.)
            .map(|a| {
                let t = p.map(|v| v.dot(a));
                let half_width = a.abs().element_sum() * (0.5 + tolerance);
                (
                    a,
                    t[0].min(t[1]).min(t[2]) - half_width,
                    t[0].max(t[1]).max(t[2]) + half_width,
                )
            })
            .collect();
        for k in first[2]..end[2] {
            for j in first[1]..end[1] {
                for i in first[0]..end[0] {
                    let cell = i + grid.n[0] * (j + grid.n[1] * k);
                    if touched[cell] || grid.fill[cell] == 0. {
                        continue;
                    }
                    let centre = DVec3::new(i as f64 + 0.5, j as f64 + 0.5, k as f64 + 0.5);
                    touched[cell] = axes.iter().all(|(axis, min, max)| {
                        let projection = centre.dot(*axis);
                        *min <= projection && projection <= *max
                    });
                }
            }
        }
    }
    touched
        .into_iter()
        .enumerate()
        .filter_map(|(i, yes)| yes.then_some(i))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use faber_geometry::bvh::Bvh;
    use faber_ports::Mesh;

    fn cuboid(size: [f64; 3]) -> Mesh {
        let [x, y, z] = size;
        Mesh {
            vertices: vec![
                [0., 0., 0.],
                [x, 0., 0.],
                [0., y, 0.],
                [x, y, 0.],
                [0., 0., z],
                [x, 0., z],
                [0., y, z],
                [x, y, z],
            ],
            triangles: vec![
                [0, 2, 3],
                [0, 3, 1],
                [4, 5, 7],
                [4, 7, 6],
                [0, 1, 5],
                [0, 5, 4],
                [2, 6, 7],
                [2, 7, 3],
                [0, 4, 6],
                [0, 6, 2],
                [1, 3, 7],
                [1, 7, 5],
            ],
            ..Mesh::default()
        }
    }

    fn sphere() -> Mesh {
        let mut mesh = Mesh::default();
        let rows = 40;
        let cols = 80;
        for i in 0..=rows {
            let phi = std::f64::consts::PI * i as f64 / rows as f64;
            for j in 0..cols {
                let angle = std::f64::consts::TAU * j as f64 / cols as f64;
                mesh.vertices.push([
                    10. * phi.sin() * angle.cos(),
                    10. * phi.sin() * angle.sin(),
                    10. * phi.cos(),
                ]);
                if i < rows {
                    let a = i * cols + j;
                    let b = i * cols + (j + 1) % cols;
                    mesh.triangles
                        .extend([[a, a + cols, b + cols], [a, b + cols, b]]);
                }
            }
        }
        mesh
    }

    fn grid(mesh: &Mesh) -> Grid {
        voxelise(mesh, &Bvh::new(mesh).unwrap(), 1.).unwrap()
    }

    #[test]
    fn cancel_after_the_first_partial_grid_batch() {
        let mesh = cuboid([64.; 3]);
        let bvh = Bvh::new(&mesh).unwrap();
        let mut completed = 0;
        let result = voxelise_with_progress(&mesh, &bvh, 1., |count, total| {
            completed = count;
            assert!(count <= total);
            count == 0
        });
        assert_eq!(result.unwrap_err(), "Study stopped.");
        assert!(completed > 0 && completed < 66_usize.pow(3));
    }

    #[test]
    fn progress_ends_at_the_full_grid_and_preserves_cell_fractions() {
        let mesh = sphere();
        let bvh = Bvh::new(&mesh).unwrap();
        let mut last = 0;
        let g = voxelise_with_progress(&mesh, &bvh, 1., |count, total| {
            assert!(count >= last && count <= total);
            last = count;
            true
        })
        .unwrap();
        let digest = g.fill.iter().fold(0xcbf29ce484222325u64, |hash, f| {
            (hash ^ u64::from(f.to_bits())).wrapping_mul(0x100000001b3)
        });
        assert_eq!(last, g.fill.len());
        assert_eq!(digest, 0xd962_358f_ada3_608d);
    }

    #[test]
    fn box_volume_is_exact() {
        let g = grid(&cuboid([10., 20., 30.]));
        assert!((g.volume() - 6000.).abs() < 1e-6);
        assert_eq!(g.origin, [-1.; 3]);
        assert_eq!(g.n, [12, 22, 32]);
        assert_eq!(g.node(1, 2, 3), 1 + 13 * (2 + 23 * 3));
    }

    #[test]
    fn sphere_volume_is_close() {
        let g = grid(&sphere());
        assert!((g.volume() / 4188.79 - 1.).abs() < 0.015, "{}", g.volume());
        assert!(g.fill.iter().any(|&f| f > 0. && f < 1.));
    }

    #[test]
    fn rotated_box_volume_is_close() {
        let mut mesh = cuboid([10., 20., 30.]);
        let (s, c) = 30_f64.to_radians().sin_cos();
        for [x, y, _] in &mut mesh.vertices {
            (*x, *y) = (c * (*x) - s * (*y), s * (*x) + c * (*y));
        }
        let v = grid(&mesh).volume();
        assert!((v / 6000. - 1.).abs() < 0.015, "{v}");
    }

    #[test]
    fn default_h_gives_about_the_target() {
        let h = default_h([[0.; 3], [100., 50., 20.]]);
        assert!((100000. ..=200000.).contains(&(100. * 50. * 20. / h.powi(3))));
        assert_eq!(default_h([[0.; 3], [0.1; 3]]), 0.2);
        assert_eq!(default_h([[0.; 3], [1e6; 3]]), 10.);
    }

    #[test]
    fn too_fine_grid_is_refused() {
        let mesh = cuboid([100.; 3]);
        let bvh = Bvh::new(&mesh).unwrap();
        let error = voxelise(&mesh, &bvh, 0.2).err().unwrap();
        assert_eq!(
            error,
            "The grid has 125000000 cells. The limit is 4000000. Use a larger resolution."
        );
    }

    #[test]
    fn cells_near_finds_the_top_layer() {
        let mesh = cuboid([10., 20., 30.]);
        let top: Vec<_> = mesh.triangles[2..4]
            .iter()
            .map(|vs| vs.map(|i| mesh.vertices[i as usize]))
            .collect();
        let g = grid(&mesh);
        let actual = cells_near(&g, &top);
        assert_eq!(actual.len(), 200);
        assert!(actual.iter().all(|&i| i / (g.n[0] * g.n[1]) == 30));
    }
}
