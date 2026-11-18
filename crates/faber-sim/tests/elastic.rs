use faber_sim::{
    Elastic, Grid, Load, StaticCase, Support, solve_static, solve_static_with_progress,
};

fn box_grid(n: [usize; 3], h: f64) -> Grid {
    Grid {
        origin: [0.; 3],
        h,
        n,
        fill: vec![1.; n.iter().product()],
    }
}

fn end_cells(g: &Grid, axis: usize, high: bool) -> Vec<usize> {
    let end = if high { g.n[axis] - 1 } else { 0 };
    (0..g.fill.len())
        .filter(|&i| {
            let xyz = [i % g.n[0], i / g.n[0] % g.n[1], i / (g.n[0] * g.n[1])];
            xyz[axis] == end && g.fill[i] > 0.
        })
        .collect()
}

fn beam(h: f64, youngs_mpa: f64, poisson: f64, force: [f64; 3]) -> StaticCase {
    let n = [10., 10., 100.].map(|s| (s / h) as usize);
    let grid = box_grid(n, h);
    let fixed = end_cells(&grid, 2, false);
    let loaded = end_cells(&grid, 2, true);
    StaticCase {
        grid,
        material: Elastic {
            youngs_mpa,
            poisson,
        },
        supports: vec![Support::Fixed { cells: fixed }],
        loads: vec![Load::Force {
            cells: loaded,
            newtons: force,
        }],
        tolerance: 1e-6,
        max_iterations: 20_000,
    }
}

fn tip(case: &StaticCase, u: &[[f64; 3]], axis: usize) -> f64 {
    let g = &case.grid;
    let [nx, ny, nz] = g.n;
    (0..=ny)
        .flat_map(|y| (0..=nx).map(move |x| u[g.node(x, y, nz)][axis]))
        .sum::<f64>()
        / ((nx + 1) * (ny + 1)) as f64
}

fn near(actual: f64, expected: f64, relative: f64) {
    assert!(
        (actual - expected).abs() <= relative * expected.abs(),
        "Expected {expected}, got {actual}"
    );
}

#[test]
fn axial_bar() {
    let case = beam(2.5, 1000., 0., [0., 0., 1000.]);
    let result = solve_static(&case).unwrap();
    near(tip(&case, &result.displacement, 2), 1., 0.02);
    near(
        result.von_mises.iter().map(|&s| s as f64).sum::<f64>() / result.von_mises.len() as f64,
        10.,
        0.02,
    );
    near(result.reaction[2], -1000., 0.001);
    assert!(result.reaction[0].abs() + result.reaction[1].abs() < 0.001);
    assert!(result.residual <= case.tolerance);
}

#[test]
fn cantilever_tip() {
    let case = beam(1.25, 2000., 0.3, [0., -10., 0.]);
    let result = solve_static(&case).unwrap();
    near(tip(&case, &result.displacement, 1), -2., 0.08);
    near(result.reaction[1], 10., 0.001);
}

#[test]
fn rotated_bar_matches() {
    use faber_geometry::bvh::Bvh;
    use faber_ports::Mesh;
    let (s, c) = 30_f64.to_radians().sin_cos();
    let rotate = |[x, y, z]: [f64; 3]| [x, c * y - s * z, s * y + c * z];
    let vertices = [
        [0., 0., 0.],
        [10., 0., 0.],
        [10., 10., 0.],
        [0., 10., 0.],
        [0., 0., 100.],
        [10., 0., 100.],
        [10., 10., 100.],
        [0., 10., 100.],
    ]
    .map(rotate)
    .to_vec();
    let triangles = vec![
        [0, 2, 1],
        [0, 3, 2],
        [4, 5, 6],
        [4, 6, 7],
        [0, 1, 5],
        [0, 5, 4],
        [1, 2, 6],
        [1, 6, 5],
        [2, 3, 7],
        [2, 7, 6],
        [3, 0, 4],
        [3, 4, 7],
    ];
    let mesh = Mesh {
        vertices,
        triangles,
        normals: vec![],
    };
    let grid = faber_sim::voxelise(&mesh, &Bvh::new(&mesh).unwrap(), 2.).unwrap();
    let tris = |slice: &[[u32; 3]]| {
        slice
            .iter()
            .map(|t| t.map(|i| mesh.vertices[i as usize]))
            .collect()
    };
    let mut case = beam(2.5, 1000., 0., [0.; 3]);
    case.supports = vec![Support::FixedFace {
        triangles: tris(&mesh.triangles[..2]),
    }];
    case.loads = vec![Load::Traction {
        triangles: tris(&mesh.triangles[2..4]),
        newtons: rotate([0., 0., 1000.]),
    }];
    case.grid = grid;
    let result = solve_static(&case).unwrap();
    let mut tip = 0.;
    let mut count = 0;
    for (i, u) in result.displacement.iter().enumerate() {
        let nx = case.grid.n[0] + 1;
        let ny = case.grid.n[1] + 1;
        let y = case.grid.origin[1] + (i / nx % ny) as f64 * case.grid.h;
        let z = case.grid.origin[2] + (i / (nx * ny)) as f64 * case.grid.h;
        if -s * y + c * z >= 99. && u != &[0.; 3] {
            tip += -s * u[1] + c * u[2];
            count += 1;
        }
    }
    assert!(count > 0);
    near(tip / count as f64, 1., 0.05);
}

#[test]
fn plate_with_hole() {
    let mut grid = box_grid([200, 80, 8], 0.5);
    for (i, f) in grid.fill.iter_mut().enumerate() {
        let x = (i % 200) as f64 * 0.5 - 50.;
        let y = (i / 200 % 80) as f64 * 0.5 - 20.;
        let mut solid = 0;
        for a in 0..3 {
            for b in 0..3 {
                let dx = x + (a as f64 + 0.5) / 6.;
                let dy = y + (b as f64 + 0.5) / 6.;
                if dx * dx + dy * dy >= 25. {
                    solid += 1;
                }
            }
        }
        *f = solid as f32 / 9.;
    }
    let fixed = end_cells(&grid, 0, false);
    let load = end_cells(&grid, 0, true);
    let case = StaticCase {
        grid,
        material: Elastic {
            youngs_mpa: 210_000.,
            poisson: 0.3,
        },
        supports: vec![Support::Fixed { cells: fixed }],
        loads: vec![Load::Force {
            cells: load,
            newtons: [1000., 0., 0.],
        }],
        tolerance: 1e-6,
        max_iterations: 20_000,
    };
    let result = solve_static(&case).unwrap();
    let peak = result.von_mises.iter().copied().fold(0., f32::max) as f64;
    let ratio = peak / (1000. / (30. * 4.));
    assert!((2.0..=3.2).contains(&ratio), "Stress concentration {ratio}");
    near(result.reaction[0], -1000., 0.001);
}

#[test]
fn gravity_bar() {
    let mut case = beam(2.5, 1000., 0.3, [0.; 3]);
    case.supports = vec![Support::Fixed {
        cells: end_cells(&case.grid, 2, true),
    }];
    case.loads = vec![Load::Gravity {
        direction: [0., 0., -1.],
        density_g_cm3: 7.85,
    }];
    let result = solve_static(&case).unwrap();
    near(result.reaction[2], 78.5 * 0.00980665, 0.01);
}

#[test]
fn free_body_is_refused() {
    let mut case = beam(2.5, 1000., 0., [1., 0., 0.]);
    case.supports.clear();
    assert_eq!(
        solve_static(&case).unwrap_err(),
        "The study has no fixture. Add a fixed face."
    );
    case.supports = vec![Support::Slide {
        cells: end_cells(&case.grid, 2, false),
        normal: [0., 0., 1.],
    }];
    assert_eq!(
        solve_static(&case).unwrap_err(),
        "The fixtures leave the part free to move. Add a fixed face."
    );
}

#[test]
fn loose_island_is_refused() {
    let mut case = beam(2.5, 1000., 0., [1., 0., 0.]);
    case.grid.fill[32..48].fill(0.);
    assert_eq!(
        solve_static(&case).unwrap_err(),
        "592 cells have no connection to a fixture."
    );
}

#[test]
fn no_convergence_is_an_error() {
    let mut case = beam(2.5, 1000., 0., [1., 0., 0.]);
    case.max_iterations = 3;
    let error = solve_static(&case).unwrap_err();
    assert!(
        error.starts_with("The solver did not converge in 3 iterations (residual "),
        "{error}"
    );
}

#[test]
fn stop_at_an_iteration_cancels_the_solve() {
    let case = beam(2.5, 1000., 0., [1., 0., 0.]);
    let mut last = 0;
    let error = solve_static_with_progress(&case, |i, _| {
        last = i;
        i < 2
    })
    .unwrap_err();
    assert_eq!(error, "Study stopped.");
    assert_eq!(last, 2);
}

#[test]
fn pressure_acts_into_the_face() {
    let mut case = beam(2.5, 1000., 0., [0.; 3]);
    let a = [0., 0., 100.];
    let b = [10., 0., 100.];
    let c = [10., 10., 100.];
    let d = [0., 10., 100.];
    case.loads = vec![Load::Pressure {
        triangles: vec![[a, b, c], [a, c, d]],
        mpa: 10.,
    }];
    let result = solve_static(&case).unwrap();
    near(result.reaction[2], 1000., 0.001);
    near(tip(&case, &result.displacement, 2), -1., 0.02);
}

#[test]
fn three_slide_planes_allow_poisson_contraction_and_remove_rigid_motion() {
    let mut case = beam(2.5, 1000., 0.3, [0., 0., 1000.]);
    case.supports = (0..3)
        .map(|axis| {
            let mut normal = [0.; 3];
            normal[axis] = 1.;
            Support::Slide {
                cells: end_cells(&case.grid, axis, false),
                normal,
            }
        })
        .collect();
    let result = solve_static(&case).unwrap();
    near(tip(&case, &result.displacement, 2), 1., 0.001);
    let far = case
        .grid
        .node(case.grid.n[0], case.grid.n[1], case.grid.n[2]);
    near(result.displacement[far][0], -0.03, 0.001);
    near(result.displacement[far][1], -0.03, 0.001);
    for s in &result.stress_mpa {
        near(s[2], 10., 0.001);
        assert!(
            [s[0], s[1], s[3], s[4], s[5]]
                .iter()
                .all(|v| v.abs() < 0.001)
        );
    }
}

fn long_rail() -> StaticCase {
    let length = 14_100.;
    let grid = box_grid([14_100, 1, 1], 1.);
    let face = |z| {
        let a = [0., 0., z];
        let b = [length, 0., z];
        let c = [length, 1., z];
        let d = [0., 1., z];
        vec![[a, b, c], [a, c, d]]
    };
    StaticCase {
        grid,
        material: Elastic {
            youngs_mpa: 1000.,
            poisson: 0.,
        },
        supports: vec![Support::FixedFace {
            triangles: face(0.),
        }],
        loads: vec![Load::Pressure {
            triangles: face(1.),
            mpa: 1.,
        }],
        tolerance: 1e-6,
        max_iterations: 100,
    }
}

#[test]
fn slender_face_integrates_the_pressure_over_the_rail() {
    let case = long_rail();
    let result = solve_static(&case).unwrap();
    near(result.reaction[2], 14_100., 1e-6);
    near(
        result.displacement[case.grid.node(7050, 0, 1)][2],
        -0.001,
        0.02,
    );
}

#[test]
fn stop_during_slender_face_sampling_precedes_the_first_iteration() {
    let mut calls = 0;
    let error = solve_static_with_progress(&long_rail(), |iteration, _| {
        assert_eq!(iteration, 0);
        calls += 1;
        calls < 2
    })
    .unwrap_err();
    assert_eq!(error, "Study stopped.");
    assert_eq!(calls, 2);
}
