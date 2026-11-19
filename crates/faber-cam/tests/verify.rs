mod common;
use common::tool;
use faber_cam::{HeightMap, Move, StockBox, Toolpath, stock_mesh, verify};
use faber_model::machining::Tool;
fn plane(z: f32) -> HeightMap {
    HeightMap {
        origin: [0.; 2],
        n: [40, 40],
        cell: 0.25,
        z: vec![z; 1600],
    }
}
fn stock() -> StockBox {
    StockBox {
        min: [0., 0., -10.],
        max: [10., 10., 0.],
    }
}
fn path(moves: Vec<Move>) -> Toolpath {
    Toolpath {
        operation: "facing".into(),
        tool: "t1".into(),
        moves,
    }
}
fn face(z: f64) -> (Toolpath, Tool) {
    let mut ms = vec![
        Move::Rapid { to: [0., 0., 5.] },
        Move::Feed {
            to: [0., 0., z],
            feed: 50.,
        },
    ];
    for i in 0..=10 {
        if i > 0 {
            ms.push(Move::Feed {
                to: [if i % 2 == 0 { 0. } else { 10. }, f64::from(i), z],
                feed: 100.,
            });
        }
        ms.push(Move::Feed {
            to: [if i % 2 == 0 { 10. } else { 0. }, f64::from(i), z],
            feed: 100.,
        });
    }
    (path(ms), tool(2.))
}
#[test]
fn clean_face_pass_removes_the_top_layer() {
    let v = verify(&plane(-1.), &stock(), &[face(-1.)], 0.02).unwrap();
    assert!(v.gouge_mm < 1e-8 && v.max_excess_mm < 1e-8);
    assert!((v.removed_mm3 - 100.).abs() < 1.);
    let mesh = stock_mesh(&v);
    assert!(!mesh.triangles.is_empty());
    assert!(
        mesh.vertices
            .iter()
            .all(|p| p[2] <= -1. + 1e-7 && p[2] >= -10.)
    );
}
#[test]
fn gouge_is_found_and_located() {
    let v = verify(&plane(-1.), &stock(), &[face(-2.)], 0.02).unwrap();
    assert!((v.gouge_mm - 1.).abs() < 1e-6);
    let [x, y, z] = v.gouge_at.unwrap();
    assert!((0. ..=10.).contains(&x) && (0. ..=10.).contains(&y));
    assert!((z + 2.).abs() < 1e-6);
}
#[test]
fn rapid_through_material_is_found() {
    let p = path(vec![
        Move::Rapid { to: [0., 0., 5.] },
        Move::Rapid {
            to: [10., 10., -1.],
        },
    ]);
    let v = verify(&plane(-3.), &stock(), &[(p, tool(1.))], 0.02).unwrap();
    assert!(!v.rapid_collisions.is_empty());
    assert_eq!(v.removed_mm3, 0.);
}
#[test]
fn short_flute_warns() {
    let mut t = tool(2.);
    t.flute_length = 8.;
    let mut s = stock();
    s.min[2] = -15.;
    let p = path(vec![
        Move::Rapid { to: [5., 5., 2.] },
        Move::Feed {
            to: [5., 5., -12.],
            feed: 50.,
        },
    ]);
    let v = verify(&plane(-14.), &s, &[(p, t)], 0.02).unwrap();
    assert!(
        v.warnings
            .iter()
            .any(|w| w == "Cut depth 12 mm is more than the flute length 8 mm of tool t1."),
        "{:?}",
        v.warnings
    );
}
#[test]
fn time_is_length_over_feed() {
    let p = path(vec![
        Move::Rapid { to: [0., 0., 1.] },
        Move::Feed {
            to: [0., 0., -1.],
            feed: 20.,
        },
        Move::Feed {
            to: [10., 0., -1.],
            feed: 10.,
        },
        Move::Dwell { seconds: 3. },
    ]);
    let v = verify(&plane(-2.), &stock(), &[(p, tool(1.))], 0.02).unwrap();
    assert!((v.minutes - 1.15).abs() < 1e-9);
}
#[test]
fn a_new_operation_retracts_before_it_crosses_the_stock() {
    let first = path(vec![
        Move::Rapid { to: [1., 1., 5.] },
        Move::Feed {
            to: [1., 1., -3.],
            feed: 60.,
        },
    ]);
    let second = path(vec![Move::Rapid { to: [9., 1., 5.] }]);
    let (total, reports) = faber_cam::verify_operations(
        &plane(-5.),
        &stock(),
        &[(first, tool(1.)), (second, tool(1.))],
        0.02,
    )
    .unwrap();
    assert!(
        total.rapid_collisions.is_empty(),
        "{:?}",
        total.rapid_collisions
    );
    assert!((reports[1].minutes - 16. / 3000.).abs() < 1e-12);
}
#[test]
fn count_colliding_rapids_after_the_marker_limit() {
    let mut ms = vec![Move::Rapid { to: [1., 1., 5.] }];
    for i in 0..150 {
        ms.push(Move::Rapid {
            to: [1. + (i % 2) as f64, 1., -1.],
        });
    }
    let (total, ops) =
        faber_cam::verify_operations(&plane(-3.), &stock(), &[(path(ms), tool(1.))], 0.02).unwrap();
    assert_eq!(total.rapid_collision_count, 150);
    assert_eq!(ops[0].rapid_collision_count, 150);
    assert_eq!(total.rapid_collisions.len(), 128);
}
#[test]
fn the_exact_bore_removes_the_chord_error_without_removing_a_cover() {
    use faber_cam::{Hole, height_map::verification_target};
    use faber_geometry::bvh::Bvh;
    let bore = Hole {
        centre: [5., 5.],
        diameter: 5.,
        top: 0.,
        bottom: -10.,
    };
    let polygon = (0..8)
        .map(|i| {
            let angle = i as f64 * std::f64::consts::TAU / 8.;
            [5. + 2.5 * angle.cos(), 5. + 2.5 * angle.sin()]
        })
        .collect();
    let mut mesh = common::part(vec![polygon], 0.);
    let coords = [5., 5.];
    let drill = path(vec![
        Move::Rapid {
            to: [coords[0], coords[1], 4.],
        },
        Move::Feed {
            to: [5., 5., -10.],
            feed: 50.,
        },
    ]);
    let bvh = Bvh::new(&mesh).unwrap();
    let plain = verification_target(&mesh, &bvh, &stock(), 0.1, &[]).unwrap();
    assert!(
        verify(&plain, &stock(), &[(drill.clone(), tool(5.))], 0.02)
            .unwrap()
            .gouge_mm
            > 9.
    );
    let target = verification_target(&mesh, &bvh, &stock(), 0.1, &[bore]).unwrap();
    let checked = verify(&target, &stock(), &[(drill.clone(), tool(5.))], 0.02).unwrap();
    if let Some([x, y, _]) = checked.gouge_at {
        let hits = bvh
            .hits([x, y, 10.], [0., 0., -1.])
            .iter()
            .map(|h| mesh.triangles[h.triangle as usize].map(|i| mesh.vertices[i as usize]))
            .collect::<Vec<_>>();
        panic!("{} mm at {x},{y}, triangles {hits:?}", checked.gouge_mm);
    }
    let first = mesh.vertices.len() as u32;
    mesh.vertices
        .extend([[4., 4., 1.], [6., 4., 1.], [6., 6., 1.], [4., 6., 1.]]);
    mesh.triangles
        .extend([[first, first + 1, first + 2], [first, first + 2, first + 3]]);
    let target =
        verification_target(&mesh, &Bvh::new(&mesh).unwrap(), &stock(), 0.1, &[bore]).unwrap();
    assert!(
        verify(&target, &stock(), &[(drill, tool(5.))], 0.02)
            .unwrap()
            .gouge_mm
            >= 11.
    );
}
