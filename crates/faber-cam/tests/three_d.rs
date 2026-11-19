mod common;
use faber_cam::{
    HeightMap, Move, StockBox, height_map, hidden_area, parallel3d, rough3d, tool_surface, verify,
};
use faber_geometry::bvh::Bvh;
use faber_model::machining::{OperationKind, ToolKind};
use faber_ports::Mesh;
fn dome() -> HeightMap {
    let n = 100;
    let cell = 0.5;
    let z = (0..n * n)
        .map(|i| {
            let x = (i % n) as f64 * cell + 0.25 - 25.;
            let y = (i / n) as f64 * cell + 0.25 - 25.;
            (-20. + (400. - x * x - y * y).max(0.).sqrt()) as f32
        })
        .collect();
    HeightMap {
        origin: [0.; 2],
        cell,
        n: [n, n],
        z,
    }
}
fn stock() -> StockBox {
    StockBox {
        min: [0., 0., -25.],
        max: [50., 50., 0.],
    }
}
#[test]
fn tool_surface_never_gouges() {
    let map = dome();
    for kind in [ToolKind::Flat, ToolKind::Ball] {
        let mut tool = common::tool(6.);
        tool.kind = kind;
        let tip = tool_surface(&map, &tool, 0.).unwrap();
        for j in (6..94).step_by(3) {
            for i in (6..94).step_by(3) {
                for dy in -6i32..=6 {
                    for dx in -6i32..=6 {
                        let d2 = f64::from(dx * dx + dy * dy) * 0.25;
                        if d2 > 9. {
                            continue;
                        }
                        let h = if kind == ToolKind::Ball {
                            3. - (9. - d2).sqrt()
                        } else {
                            0.
                        };
                        let zi = tip.z[j * 100 + i] as f64 + h;
                        let pj = (j as i32 + dy) as usize;
                        let pi = (i as i32 + dx) as usize;
                        assert!(zi + 0.00001 >= map.z[pj * 100 + pi] as f64);
                    }
                }
            }
        }
    }
}
#[test]
fn roughing_leaves_at_most_a_step() {
    let mesh = half_sphere_on_a_base();
    let bvh = Bvh::new(&mesh).unwrap();
    let map = faber_cam::stock_height_map(&mesh, &bvh, &stock(), 0.25).unwrap();
    let target =
        faber_cam::height_map::verification_target(&mesh, &bvh, &stock(), 0.25, &[]).unwrap();
    let t = common::tool(4.);
    let mut o = common::op(OperationKind::Rough3d {
        stepover: 1.5,
        stock_to_leave: 0.,
    });
    o.heights.bottom = Some(-20.);
    o.stepdown = Some(2.);
    let p = faber_cam::three_d::rough3d_on_mesh(&mesh, &map, &stock(), &t, &o).unwrap();
    assert!(p.moves.len() > 20);
    let v = verify(&target, &stock(), &[(p, t)], 0.02).unwrap();
    assert!(v.gouge_mm < 0.02 && v.rapid_collisions.is_empty());
    assert!(
        v.max_excess_mm <= 2.1,
        "{} mm at the roughing boundary",
        v.max_excess_mm
    );
}
#[test]
fn parallel_scallop_height() {
    let map = HeightMap {
        origin: [0.; 2],
        cell: 0.125,
        n: [160, 160],
        z: vec![-2.; 25600],
    };
    let stock = StockBox {
        min: [0., 0., -5.],
        max: [20., 20., 0.],
    };
    let mut t = common::tool(6.);
    t.kind = ToolKind::Ball;
    let o = common::op(OperationKind::Parallel3d {
        stepover: 0.5,
        angle_deg: 0.,
    });
    let path = parallel3d(&map, &t, &o).unwrap();
    let v = verify(&map, &stock, &[(path, t)], 0.02).unwrap();
    assert!(v.gouge_mm < 1e-6);
    assert!(
        v.max_excess_mm <= 0.0104 + 0.005 && v.max_excess_mm > 0.002,
        "{}",
        v.max_excess_mm
    );
}
#[test]
fn parallel_lines_alternate() {
    let mut map = dome();
    map.z.fill(-2.);
    let mut t = common::tool(6.);
    t.kind = ToolKind::Ball;
    let p = parallel3d(
        &map,
        &t,
        &common::op(OperationKind::Parallel3d {
            stepover: 2.,
            angle_deg: 30.,
        }),
    )
    .unwrap();
    let mut runs = vec![];
    let mut prev = None;
    for m in p.moves {
        match m {
            Move::Rapid { .. } => prev = None,
            Move::Feed { to, .. } => {
                if let Some(a) = prev {
                    let d: f64 = to[0] - a;
                    if d.abs() > 0.01 && runs.last() != Some(&d.signum()) {
                        runs.push(d.signum());
                    }
                }
                prev = Some(to[0]);
            }
            _ => {}
        }
    }
    assert!(runs.len() >= 8);
}
#[test]
fn undercut_area_is_reported() {
    let lower = common::part(vec![], 0.);
    let mut mesh = lower.clone();
    let offset = mesh.vertices.len() as u32;
    mesh.vertices.extend(
        lower
            .vertices
            .iter()
            .map(|p| [p[0] * 1.5 - 5., p[1] * 1.5 - 5., p[2] + 10.]),
    );
    mesh.triangles
        .extend(lower.triangles.iter().map(|t| t.map(|i| i + offset)));
    let bvh = Bvh::new(&mesh).unwrap();
    assert!(hidden_area(&mesh, &bvh, [0., 0., 1.]) > 10.);
}
#[test]
fn too_many_cells_are_refused() {
    let m = Mesh {
        vertices: vec![[0.; 3], [5000., 0., 0.], [0., 5000., 0.]],
        triangles: vec![[0, 1, 2]],
        ..Default::default()
    };
    let b = Bvh::new(&m).unwrap();
    let e = height_map(&m, &b, 1., -1.).unwrap_err();
    assert!(e.contains("25000000") && e.contains("16000000"), "{e}");
}
#[test]
fn a_narrow_ridge_between_cell_centres_is_preserved() {
    let mesh = Mesh {
        vertices: vec![[0., 0., 1.], [0.09, 0., 1.], [0., 4., 1.]],
        triangles: vec![[0, 1, 2]],
        ..Default::default()
    };
    let map = height_map(&mesh, &Bvh::new(&mesh).unwrap(), 0.25, -1.).unwrap();
    assert!(map.z.iter().all(|z| *z >= 1.));
}

fn half_sphere_on_a_base() -> Mesh {
    let mut m = common::part(vec![], 0.);
    for p in &mut m.vertices {
        p[0] *= 50. / 40.;
        p[1] *= 50. / 30.;
        p[2] = p[2] * 0.5 - 20.;
    }
    let start = m.vertices.len() as u32;
    let sectors = 180u32;
    let bands = 45u32;
    for band in 0..=bands {
        let theta = std::f64::consts::FRAC_PI_2 * f64::from(band) / f64::from(bands);
        for k in 0..sectors {
            let phi = std::f64::consts::TAU * f64::from(k) / f64::from(sectors);
            m.vertices.push([
                25. + 20. * theta.sin() * phi.cos(),
                25. + 20. * theta.sin() * phi.sin(),
                -20. + 20. * theta.cos(),
            ]);
        }
    }
    for band in 0..bands {
        for k in 0..sectors {
            let a = start + band * sectors + k;
            let b = start + band * sectors + (k + 1) % sectors;
            if band != 0 {
                m.triangles.push([a, a + sectors, b]);
            }
            m.triangles.push([b, a + sectors, b + sectors]);
        }
    }
    m
}
#[test]
fn hemisphere_finishing_retains_the_inaccessible_foot_in_the_report() {
    use faber_cam::height_map::{stock_height_map, verification_target};
    let mesh = half_sphere_on_a_base();
    let bvh = Bvh::new(&mesh).unwrap();
    let s = stock();
    let envelope = stock_height_map(&mesh, &bvh, &s, 0.25).unwrap();
    let target = verification_target(&mesh, &bvh, &s, 0.25, &[]).unwrap();
    let mut t = common::tool(6.);
    t.kind = ToolKind::Ball;
    let mut o = common::op(OperationKind::Parallel3d {
        stepover: 0.5,
        angle_deg: 0.,
    });
    o.heights.bottom = Some(-20.);
    let path = parallel3d(&envelope, &t, &o).unwrap();
    let v = verify(&target, &s, &[(path, t)], 0.02).unwrap();
    let crown = v
        .stock
        .z
        .iter()
        .zip(&target.z)
        .enumerate()
        .filter(|(i, _)| {
            let x = (*i % 200) as f64 * 0.25 + 0.125 - 25.;
            let y = (*i / 200) as f64 * 0.25 + 0.125 - 25.;
            x * x + y * y < 4.
        })
        .map(|(_, (&a, &b))| f64::from(a - b))
        .fold(0., f64::max);
    assert!(v.gouge_mm < 0.02 && v.rapid_collisions.is_empty());
    assert!(
        crown <= 0.5 * 0.5 / (8. * 3.) + 0.05,
        "Crown excess {crown} mm"
    );
    assert!(
        v.max_excess_mm > 0.06,
        "The concave foot must retain stock."
    );
    eprintln!(
        "R20 hemisphere, 0.25 mm grid, D6 ball, 0.5 mm stepover: crown {crown:.5} mm, full-grid excess {:.5} mm",
        v.max_excess_mm
    );
}
#[test]
fn roughing_on_a_sampled_surface_preserves_its_height_field() {
    let map = dome();
    let t = common::tool(4.);
    let mut o = common::op(OperationKind::Rough3d {
        stepover: 1.5,
        stock_to_leave: 0.,
    });
    o.heights.bottom = Some(-20.);
    o.stepdown = Some(2.);
    let path = rough3d(&map, &stock(), &t, &o).unwrap();
    let v = verify(&map, &stock(), &[(path, t)], 0.02).unwrap();
    assert!(v.gouge_mm < 0.02 && v.rapid_collisions.is_empty() && v.max_excess_mm <= 2.1);
}
