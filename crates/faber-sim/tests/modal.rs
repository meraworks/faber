use faber_sim::{Elastic, Grid, ModalCase, Support, solve_modal, solve_modal_with_progress};

fn bar(h: f64, poisson: f64, modes: usize) -> ModalCase {
    let n = [10., 10., 100.].map(|v| (v / h) as usize);
    ModalCase {
        grid: Grid {
            origin: [0.; 3],
            n,
            h,
            fill: vec![1.; n.iter().product()],
        },
        material: Elastic {
            youngs_mpa: 200_000.,
            poisson,
        },
        density_g_cm3: 7.85,
        supports: vec![Support::Fixed {
            cells: (0..n[0] * n[1]).collect(),
        }],
        modes,
    }
}
fn near(actual: f64, want: f64, tol: f64) {
    assert!(
        (actual / want - 1.).abs() < tol,
        "Expected {want}, got {actual}"
    );
}
#[test]
fn cantilever_first_frequency() {
    let result = solve_modal(&bar(1.25, 0.3, 2)).unwrap();
    near(result[0].frequency_hz, 815.4, 0.08);
    near(result[1].frequency_hz, result[0].frequency_hz, 0.01);
}
#[test]
fn axial_mode() {
    let result = solve_modal(&bar(1.25, 0., 9)).unwrap();
    let axial = result
        .iter()
        .max_by(|a, b| a.mass_share[2].total_cmp(&b.mass_share[2]))
        .unwrap();
    near(axial.frequency_hz, 12619., 0.05);
    assert!(axial.mass_share[2] > 0.7);
}
#[test]
fn modes_are_sorted_and_orthogonal() {
    let c = bar(5., 0.3, 4);
    let modes = solve_modal(&c).unwrap();
    let [nx, ny, nz] = c.grid.n;
    let masses: Vec<_> = (0..=nz)
        .flat_map(|z| {
            (0..=ny).flat_map(move |y| {
                (0..=nx).map(move |x| {
                    [(x, nx), (y, ny), (z, nz)]
                        .iter()
                        .fold(1., |m, &(i, n)| m * if i == 0 || i == n { 0.5 } else { 1. })
                })
            })
        })
        .collect();
    let inner = |a: &[[f32; 3]], b: &[[f32; 3]]| {
        masses
            .iter()
            .zip(a)
            .zip(b)
            .map(|((&m, a), b)| {
                m * (0..3)
                    .map(|j| f64::from(a[j]) * f64::from(b[j]))
                    .sum::<f64>()
            })
            .sum::<f64>()
    };
    for (i, a) in modes.iter().enumerate() {
        near(
            a.shape
                .iter()
                .map(|p| p.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>().sqrt())
                .fold(0., f64::max),
            1.,
            1e-6,
        );
        for b in &modes[..i] {
            assert!(b.frequency_hz <= a.frequency_hz);
            let correlation = inner(&a.shape, &b.shape)
                / (inner(&a.shape, &a.shape) * inner(&b.shape, &b.shape)).sqrt();
            assert!(correlation.abs() < 1e-6, "{correlation}");
        }
        assert!(a.shape[..(nx + 1) * (ny + 1)].iter().all(|p| *p == [0.; 3]));
    }
}
#[test]
fn free_body_has_six_zero_modes() {
    let mut c = bar(5., 0.3, 7);
    c.supports.clear();
    let r = solve_modal(&c).unwrap();
    assert!(r[..6].iter().all(|m| m.frequency_hz < 1.));
    assert!(r[6].frequency_hz > 500.);
    for axis in 0..3 {
        near(r[..6].iter().map(|m| m.mass_share[axis]).sum(), 1., 1e-7);
    }
}
#[test]
fn too_many_modes_are_refused() {
    let mut c = bar(10., 0.3, 500);
    assert!(solve_modal(&c).unwrap_err().contains("mode"));
    c.modes = 0;
    assert!(solve_modal(&c).is_err());
    c.grid.n = [1; 3];
    c.grid.fill = vec![1.];
    c.supports = vec![Support::Fixed { cells: vec![0] }];
    c.modes = 13;
    assert!(solve_modal(&c).unwrap_err().contains("12 free degrees"));
}

#[test]
fn oblique_slide_preserves_the_allowed_rigid_modes() {
    let mut c = bar(5., 0.3, 5);
    c.supports = vec![Support::Slide {
        cells: vec![0, 1, 2, 3],
        normal: [1., 1., 0.],
    }];
    let modes = solve_modal(&c).unwrap();
    assert!(modes[..4].iter().all(|m| m.frequency_hz < 1.));
    assert!(modes[4].frequency_hz > 100.);
    for mode in modes {
        for p in &mode.shape[..9] {
            assert!((p[0] + p[1]).abs() < 2e-6);
        }
    }
}

#[test]
fn density_sets_the_frequency_scale() {
    let mut c = bar(5., 0.3, 3);
    let a = solve_modal(&c).unwrap();
    c.density_g_cm3 *= 4.;
    let b = solve_modal(&c).unwrap();
    for (a, b) in a.iter().zip(b) {
        near(a.frequency_hz, 2. * b.frequency_hz, 1e-7);
    }
}
#[test]
fn modal_analysis_can_be_cancelled() {
    let c = bar(2.5, 0.3, 3);
    let mut polls = 0;
    let e = solve_modal_with_progress(&c, |_, _| {
        polls += 1;
        polls < 4
    })
    .unwrap_err();
    assert!(e.contains("stopped"), "{e}");
}
