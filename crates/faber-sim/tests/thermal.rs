use faber_sim::thermal::{ThermalBc, ThermalCase, solve_thermal};
use faber_sim::{Elastic, Grid, Load, StaticCase, Support, solve_static};

fn bar() -> Grid {
    Grid {
        n: [20, 2, 2],
        h: 5.,
        origin: [0.; 3],
        fill: vec![1.; 80],
    }
}
fn end(x: usize) -> Vec<usize> {
    (0..4).map(|i| x + i * 20).collect()
}
fn cool() -> ThermalBc {
    ThermalBc::Temperature {
        cells: end(0),
        celsius: 20.,
    }
}
fn heat(watts: f64) -> ThermalBc {
    ThermalBc::Power {
        cells: end(19),
        watts,
    }
}
fn case(conditions: Vec<ThermalBc>) -> ThermalCase {
    ThermalCase {
        grid: bar(),
        conductivity_w_mk: 200.,
        conditions,
    }
}
#[test]
fn linear_temperature_in_a_bar() {
    let g = bar();
    let r = solve_thermal(&case(vec![
        cool(),
        ThermalBc::Temperature {
            cells: end(19),
            celsius: 100.,
        },
    ]))
    .unwrap();
    assert!((r.temperature[g.node(10, 1, 1)] - 60.).abs() < 0.5);
}
#[test]
fn power_gives_the_gradient() {
    let r = solve_thermal(&case(vec![heat(10.), cool()])).unwrap();
    assert!((r.max_c - 70.).abs() < 1., "{}", r.max_c);
}
#[test]
fn convection_cools_a_block() {
    let convection = ThermalBc::Convection {
        triangles: vec![
            [[0., 0., 0.], [0., 10., 0.], [0., 10., 10.]],
            [[0., 0., 0.], [0., 10., 10.], [0., 0., 10.]],
        ],
        w_m2k: 25.,
        ambient_c: 20.,
    };
    let r = solve_thermal(&case(vec![heat(5.), convection])).unwrap();
    let g = bar();
    let mut mean = 0.;
    for y in 0..3 {
        for z in 0..3 {
            mean += r.temperature[g.node(0, y, z)] as f64 / 9.;
        }
    }
    assert!((mean - 2020.).abs() / 2020. < 0.03, "{mean}");
}
#[test]
fn no_heat_sink_is_refused() {
    assert_eq!(
        solve_thermal(&case(vec![heat(2.)])).unwrap_err(),
        "The study has heat input and no fixed temperature or convection."
    );
}
#[test]
fn each_separate_solid_needs_a_heat_sink() {
    let mut c = case(vec![heat(2.), cool()]);
    for i in end(10) {
        c.grid.fill[i] = 0.;
    }
    assert!(solve_thermal(&c).unwrap_err().contains("heat sink"));
}
#[test]
fn conflicting_temperatures_are_refused() {
    assert!(
        solve_thermal(&case(vec![
            cool(),
            ThermalBc::Temperature {
                cells: end(0),
                celsius: 40.
            }
        ]))
        .is_err()
    );
}
fn expansion(both: bool) -> StaticCase {
    let g = bar();
    let mut supports = vec![Support::Fixed { cells: end(0) }];
    if both {
        supports.push(Support::Fixed { cells: end(19) });
    }
    StaticCase {
        material: Elastic {
            youngs_mpa: 68900.,
            poisson: 0.,
        },
        loads: vec![Load::Temperature {
            field: vec![70.; g.n.iter().map(|n| n + 1).product()],
            reference_c: 20.,
            expansion_um_mk: 23.6,
        }],
        grid: g,
        supports,
        tolerance: 1e-9,
        max_iterations: 1500,
    }
}
#[test]
fn clamped_bar_thermal_stress() {
    let r = solve_static(&expansion(true)).unwrap();
    assert!(
        (r.stress_mpa[10][0] + 81.302).abs() < 2.4,
        "{:?}",
        r.stress_mpa[10]
    );
}
#[test]
fn free_bar_grows_without_stress() {
    // The end can expand in its plane. Three slide faces remove rigid motion.
    let mut c = expansion(false);
    c.supports = vec![
        Support::Slide {
            cells: end(0),
            normal: [1., 0., 0.],
        },
        Support::Slide {
            cells: (0..80).filter(|i| (i / 20) % 2 == 0).collect(),
            normal: [0., 1., 0.],
        },
        Support::Slide {
            cells: (0..40).collect(),
            normal: [0., 0., 1.],
        },
    ];
    let r = solve_static(&c).unwrap();
    assert!((r.displacement[c.grid.node(20, 1, 1)][0] - 0.118).abs() < 0.00236);
    assert!(r.von_mises.iter().copied().fold(0., f32::max) < 1.);
}

fn end_face(origin: [f64; 3], x: f64) -> Vec<[[f64; 3]; 3]> {
    let [a, b, c, d] = [[x, 0., 0.], [x, 10., 0.], [x, 10., 10.], [x, 0., 10.]]
        .map(|p| std::array::from_fn(|axis| p[axis] + origin[axis]));
    vec![[a, b, c], [a, c, d]]
}

#[test]
fn weak_convection_without_heat_stays_exactly_at_ambient() {
    let c = case(vec![ThermalBc::Convection {
        triangles: end_face([0.; 3], 0.),
        w_m2k: 1e-7,
        ambient_c: 23.5,
    }]);
    let r = solve_thermal(&c).unwrap();
    assert_eq!(r.iterations, 0);
    assert_eq!((r.min_c, r.max_c), (23.5, 23.5));
}

#[test]
fn moving_the_grid_and_faces_preserves_the_temperature_field() {
    let run = |origin| {
        let mut c = case(vec![
            ThermalBc::TemperatureFace {
                triangles: end_face(origin, 0.),
                celsius: 20.,
            },
            ThermalBc::PowerFace {
                triangles: end_face(origin, 100.),
                watts: 10.,
            },
        ]);
        c.grid.origin = origin;
        solve_thermal(&c).unwrap()
    };
    let a = run([0.; 3]);
    let b = run([1300., -770., 410.]);
    assert!((a.max_c - 70.).abs() < 0.5, "{}", a.max_c);
    assert_eq!(a.temperature, b.temperature);
}

#[test]
fn uniform_grid_scaling_preserves_a_temperature_driven_solution() {
    for h in [1e130, 1e-130] {
        let mut c = case(vec![
            cool(),
            ThermalBc::Temperature {
                cells: end(19),
                celsius: 100.,
            },
        ]);
        c.grid.h = h;
        let r = solve_thermal(&c).unwrap();
        assert!(
            (r.temperature[c.grid.node(10, 1, 1)] - 60.).abs() < 0.01,
            "h={h:e}, {r:?}"
        );
    }
}

#[test]
fn unrepresentable_thermal_stiffness_is_refused() {
    let mut c = case(vec![cool()]);
    c.grid.h = f64::MAX;
    c.conductivity_w_mk = f64::MAX;
    assert!(solve_thermal(&c).is_err());
}

#[test]
fn a_thin_grid_above_the_workspace_limit_is_refused_before_setup() {
    let c = ThermalCase {
        grid: Grid {
            n: [2_000_000, 1, 1],
            fill: vec![1.; 2_000_000],
            h: 1.,
            origin: [0.; 3],
        },
        conductivity_w_mk: 1.,
        conditions: vec![],
    };
    let error = faber_sim::thermal::solve_thermal_with_progress(&c, |_, _| false).unwrap_err();
    assert!(error.contains("workspace"), "{error}");
}

#[test]
fn cancellation_during_grid_assembly_stops_before_iterations() {
    let mut polls = 0;
    let result = faber_sim::thermal::solve_thermal_with_progress(
        &case(vec![cool(), heat(10.)]),
        |iteration, _| {
            if iteration == 0 {
                polls += 1;
            }
            polls < 2
        },
    );
    assert_eq!(result.unwrap_err(), "Study stopped.");
}

#[test]
#[ignore = "Measures release solver time on 51,200 cells"]
fn benchmark_scalar_heating_and_cancel() {
    let n = [50, 32, 32];
    let face = |x| (0..n[1] * n[2]).map(|i| x + i * n[0]).collect();
    let c = ThermalCase {
        grid: Grid {
            n,
            h: 2.,
            origin: [0.; 3],
            fill: vec![1.; n.iter().product()],
        },
        conductivity_w_mk: 200.,
        conditions: vec![
            ThermalBc::Temperature {
                cells: face(0),
                celsius: 20.,
            },
            ThermalBc::Power {
                cells: face(n[0] - 1),
                watts: 100.,
            },
        ],
    };
    let start = std::time::Instant::now();
    let result = solve_thermal(&c).unwrap();
    println!(
        "51,200 cells, {:.2} °C, {} iterations in {:.3} s",
        result.max_c,
        result.iterations,
        start.elapsed().as_secs_f64()
    );
    assert!((result.max_c - (20. + 100. * 100. / (0.2 * 64. * 64.))).abs() < 0.01);
    let start = std::time::Instant::now();
    let mut reached = 0;
    let stopped = faber_sim::thermal::solve_thermal_with_progress(&c, |i, _| {
        reached = i;
        i < 3
    });
    assert_eq!(stopped.unwrap_err(), "Study stopped.");
    assert_eq!(reached, 3);
    println!(
        "Cancel at iteration {reached} after {:.3} s",
        start.elapsed().as_secs_f64()
    );
}
