use faber_ports::Mesh;
use faber_sim::{
    Elastic, Grid, Load, StaticCase, Support,
    shape::{Plane, ShapeCase, ShapeResult, optimise_shape},
    surface::iso_surface,
};
use glam::DVec3;
use std::{collections::HashMap, sync::OnceLock};
fn beam(h: f64) -> ShapeCase {
    let n = [120., 20., 40.].map(|v| (v / h).round() as usize);
    let grid = Grid {
        n,
        h,
        origin: [0.; 3],
        fill: vec![1.; n.iter().product()],
    };
    let id = |x, y, z| x + n[0] * (y + n[1] * z);
    let supports = [0, n[0] - 1]
        .map(|x| Support::Fixed {
            cells: (0..n[1]).map(|y| id(x, y, 0)).collect(),
        })
        .to_vec();
    let loaded = (0..n[1])
        .flat_map(|y| [id(n[0] / 2 - 1, y, n[2] - 1), id(n[0] / 2, y, n[2] - 1)])
        .collect();
    ShapeCase {
        base: StaticCase {
            grid,
            material: Elastic {
                youngs_mpa: 2000.,
                poisson: 0.3,
            },
            supports,
            loads: vec![Load::Force {
                cells: loaded,
                newtons: [0., 0., -100.],
            }],
            tolerance: 5e-5,
            max_iterations: 3000,
        },
        keep: vec![id(0, 0, 1), id(n[0] - 1, 0, 1)],
        target_volume: 0.5,
        min_member_mm: h * 1.5,
        max_iterations: 45,
        symmetry: vec![Plane {
            origin: [60., 0., 0.],
            normal: [1., 0., 0.],
        }],
        density_g_cm3: 1.24,
    }
}
fn result() -> &'static ShapeResult {
    static R: OnceLock<ShapeResult> = OnceLock::new();
    R.get_or_init(|| optimise_shape(&beam(5.)).unwrap())
}
fn volume(mesh: &Mesh) -> f64 {
    mesh.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
            a.dot(b.cross(c)) / 6.
        })
        .sum()
}
fn closed(mesh: &Mesh) {
    let mut edges = HashMap::new();
    for &[a, b, c] in &mesh.triangles {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            assert_ne!(u, v);
            let e = edges.entry((u.min(v), u.max(v))).or_insert((0, 0));
            e.0 += 1;
            e.1 += if u < v { 1 } else { -1 };
        }
    }
    assert!(!edges.is_empty());
    assert!(edges.values().all(|e| *e == (2, 0)));
}
#[test]
fn beam_reaches_the_target() {
    let r = result();
    assert!((r.volume_share - 0.5).abs() < 0.02);
    assert!(
        r.compliance.last().unwrap() < &r.compliance[4],
        "{:?}",
        r.compliance
    );
    assert_eq!(r.pieces, 1);
    assert!((r.mass_g - r.volume_share * 120. * 20. * 40. * 1.24e-3).abs() < 1e-3);
}
#[test]
fn result_is_symmetric() {
    let r = result();
    let n = beam(5.).base.grid.n;
    let matched = r
        .density
        .iter()
        .enumerate()
        .filter(|(i, x)| {
            let j = i / n[0] * n[0] + n[0] - 1 - i % n[0];
            (**x - r.density[j]).abs() < 0.05
        })
        .count();
    assert!(matched as f64 >= 0.99 * r.density.len() as f64);
}
#[test]
fn kept_cells_stay_solid() {
    for i in beam(5.).keep {
        assert_eq!(result().density[i], 1.);
    }
}
#[test]
fn result_mesh_is_closed() {
    let r = result();
    closed(&r.mesh);
    let v = volume(&r.mesh);
    let target = 120. * 20. * 40. * r.volume_share;
    assert!((v - target).abs() < 0.05 * target, "{v} vs {target}");
}
#[test]
fn kept_regions_above_the_target_are_refused() {
    let mut c = beam(5.);
    let n = c.base.grid.fill.len();
    c.keep = (0..((n as f64 * 0.62).round() as usize)).collect();
    c.base.supports.clear();
    c.base.loads.clear();
    c.symmetry.clear();
    assert_eq!(
        optimise_shape(&c).unwrap_err(),
        "The kept regions are 62 % of the part. The target is 50 %."
    );
}
#[test]
fn loose_pieces_are_reported() {
    let mut c = beam(5.);
    let n = c.base.grid.n;
    for (i, f) in c.base.grid.fill.iter_mut().enumerate() {
        let y = i / n[0] % n[1];
        if y == 1 || y == 2 {
            *f = 0.;
        }
    }
    for support in &mut c.base.supports {
        if let Support::Fixed { cells } = support {
            cells.retain(|&i| c.base.grid.fill[i] > 0.);
        }
    }
    if let Load::Force { cells, .. } = &mut c.base.loads[0] {
        cells.retain(|&i| c.base.grid.fill[i] > 0.);
    }
    c.target_volume = 0.1;
    c.max_iterations = 6;
    let r = optimise_shape(&c).unwrap();
    assert!(r.pieces > 1);
    assert!(r.limits.iter().any(|s| s.contains("separate pieces")));
}
#[test]
fn iso_surface_of_a_sphere_field() {
    let n = [32usize; 3];
    let grid = Grid {
        origin: [-16.; 3],
        h: 1.,
        n,
        fill: vec![1.; n.iter().product()],
    };
    let f: Vec<_> = (0..grid.fill.len())
        .map(|i| {
            let p = DVec3::new((i % 32) as f64, (i / 32 % 32) as f64, (i / 1024) as f64)
                - DVec3::splat(15.5);
            (10. - p.length()) as f32
        })
        .collect();
    let mesh = iso_surface(&grid, &f, 0.);
    closed(&mesh);
    let v0 = 4. / 3. * std::f64::consts::PI * 1000.;
    assert!((volume(&mesh) / v0 - 1.).abs() < 0.02);
}

#[test]
#[ignore = "50 000 cell optimisation timing"]
fn shape_timing_50000_cells() {
    let n = [50, 40, 25];
    let mut case = beam(5.);
    case.base.grid = Grid {
        n,
        h: 2.,
        origin: [0.; 3],
        fill: vec![1.; 50_000],
    };
    let id = |x, y, z| x + n[0] * (y + n[1] * z);
    case.base.supports = [0, n[0] - 1]
        .map(|x| Support::Fixed {
            cells: (0..n[1]).map(|y| id(x, y, 0)).collect(),
        })
        .to_vec();
    case.base.loads = vec![Load::Force {
        cells: (0..n[1])
            .flat_map(|y| [id(24, y, 24), id(25, y, 24)])
            .collect(),
        newtons: [0., 0., -100.],
    }];
    case.symmetry = vec![Plane {
        origin: [50., 0., 0.],
        normal: [1., 0., 0.],
    }];
    case.keep.clear();
    case.min_member_mm = 6.;
    case.max_iterations = 80;
    let start = std::time::Instant::now();
    let result = faber_sim::optimise_shape_with_progress(&case, |i, _, c| {
        if i == 1 || i % 10 == 0 {
            eprintln!(
                "Shape iteration {i}, compliance {c:.6}, elapsed {:?}",
                start.elapsed()
            );
        }
        true
    })
    .unwrap();
    eprintln!(
        "50000 cells: {} iterations, {:.3} s, volume {:.5}, compliance {:.6} to {:.6}, {} pieces.",
        result.iterations,
        start.elapsed().as_secs_f64(),
        result.volume_share,
        result.compliance[0],
        result.compliance.last().unwrap(),
        result.pieces
    );
    assert!((result.volume_share - case.target_volume).abs() < 0.02);
}

#[test]
fn stopping_keeps_the_last_solved_density() {
    let case = beam(5.);
    let mut saved = Vec::new();
    let result = faber_sim::optimise_shape_with_progress(&case, |i, density, _| {
        if i == 3 {
            saved = density.to_vec();
            false
        } else {
            true
        }
    })
    .unwrap();
    assert_eq!(result.iterations, 3);
    assert_eq!(result.density, saved);
    closed(&result.mesh);
}

#[test]
fn the_first_elastic_solve_can_stop_before_building_a_preview() {
    let mut polls = 0;
    let err = faber_sim::shape::optimise_shape_with_poll(&beam(5.), |_, finished| {
        assert!(finished.is_none());
        polls += 1;
        polls < 2
    })
    .unwrap_err();
    assert_eq!(err, "Study stopped.");
    assert_eq!(polls, 2);
}
#[test]
fn stopping_during_a_later_elastic_solve_restores_the_last_complete_iteration() {
    let mut saved = Vec::new();
    let mut checks = 0;
    let result = faber_sim::shape::optimise_shape_with_poll(&beam(5.), |i, finished| {
        if let Some((s, _)) = finished {
            saved = s.to_vec();
        } else if i == 3 {
            checks += 1;
            if checks == 2 {
                return false;
            }
        }
        true
    })
    .unwrap();
    assert_eq!(result.iterations, 2);
    assert_eq!(result.density, saved);
    assert_eq!(checks, 2);
}

#[test]
fn the_extracted_surface_has_outward_vertex_normals() {
    let g = Grid {
        origin: [0.; 3],
        n: [3; 3],
        h: 1.,
        fill: vec![1.; 27],
    };
    let mesh = iso_surface(&g, &g.fill, 0.5);
    assert_eq!(mesh.normals.len(), mesh.vertices.len());
    for (&p, &n) in mesh.vertices.iter().zip(&mesh.normals) {
        let normal = DVec3::from(n);
        assert!((normal.length() - 1.).abs() < 1e-10);
        assert!((DVec3::from(p) - DVec3::splat(1.5)).dot(normal) > 0.);
    }
}

#[test]
fn exact_face_loads_and_fixtures_remain_solid_in_the_design() {
    let mut case = beam(5.);
    case.keep.clear();
    let pad = |x: f64, z: f64| {
        vec![
            [[x, 5., z], [x + 5., 5., z], [x + 5., 15., z]],
            [[x, 5., z], [x + 5., 15., z], [x, 15., z]],
        ]
    };
    let left = pad(5., 0.);
    let right = pad(110., 0.);
    let load = pad(57.5, 40.);
    case.base.supports = vec![
        Support::FixedFace {
            triangles: left.clone(),
        },
        Support::FixedFace {
            triangles: right.clone(),
        },
    ];
    case.base.loads = vec![Load::Traction {
        triangles: load.clone(),
        newtons: [0., 0., -100.],
    }];
    case.max_iterations = 4;
    let answer = optimise_shape(&case).unwrap();
    for patch in [left, right, load] {
        let kept = faber_sim::cells_near(&case.base.grid, &patch);
        assert!(!kept.is_empty());
        assert!(kept.into_iter().all(|i| answer.density[i] == 1.));
    }
}
