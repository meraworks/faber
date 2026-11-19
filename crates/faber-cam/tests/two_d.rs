mod common;
use common::*;
use faber_cam::*;
use faber_model::machining::*;
use std::f64::consts::{PI, TAU};

fn check_safe(path: &Toolpath) {
    assert!(matches!(path.moves.first(),Some(Move::Rapid{to}) if to[2]==8.));
    let mut prev = None;
    for m in &path.moves {
        if let (Some(a), Move::Rapid { to: b }) = (prev, m) {
            let a: [f64; 3] = a;
            if a[..2] != b[..2] {
                assert!(a[2] >= 4. && b[2] >= 4., "Rapid from {a:?} to {b:?}");
            }
        }
        if let Some(b) = m.target() {
            prev = Some(b);
        }
    }
}
#[test]
fn contour_of_a_rectangle_and_depth_passes() {
    let mesh = faber_geometry::NativeKernel
        .build(&faber_model::Geometry::Extrude {
            profile: faber_model::Profile {
                outer: rect(0., 0., 40., 20.),
                holes: vec![],
            },
            height: 10.,
        })
        .unwrap();
    use faber_ports::Kernel;
    let mut mesh = mesh;
    for p in &mut mesh.vertices {
        p[2] -= 10.;
    }
    let p = contour(
        &mesh,
        &tool(6.),
        &op(OperationKind::Contour {
            z: Selection::Z(-5.),
            side: Side::Outside,
            stock_to_leave: 0.,
        }),
    )
    .unwrap();
    check_safe(&p);
    for depth in [-3., -6., -9., -10.] {
        let length = p
            .moves
            .windows(2)
            .filter_map(|w| match (&w[0], &w[1]) {
                (Move::Feed { to: a, .. }, Move::Feed { to: b, .. })
                    if a[2] == depth && b[2] == depth =>
                {
                    Some((b[0] - a[0]).hypot(b[1] - a[1]))
                }
                _ => None,
            })
            .sum::<f64>();
        assert!((length - 120. - TAU * 3.).abs() < 0.5, "{depth}: {length}");
    }
}
#[test]
fn inside_contour_of_a_hole() {
    let ring = (0..192)
        .map(|i| {
            let t = f64::from(i) * TAU / 192.;
            [20. + 10. * t.cos(), 15. + 10. * t.sin()]
        })
        .collect();
    let p = contour(
        &part(vec![ring], 0.),
        &tool(6.),
        &op(OperationKind::Contour {
            z: Selection::Z(-5.),
            side: Side::Inside,
            stock_to_leave: 0.,
        }),
    )
    .unwrap();
    let pts = p
        .moves
        .iter()
        .filter_map(|m| {
            if let Move::Feed { to, .. } = m {
                Some(*to)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(pts.len() > 20);
    for pt in pts {
        assert!(((pt[0] - 20.).hypot(pt[1] - 15.) - 7.).abs() < 0.012);
    }
}
#[test]
fn face_covers_the_stock_and_refuses_depth_below_it() {
    let s = StockBox {
        min: [0., 0., -25.],
        max: [50., 50., 0.],
    };
    let mut o = op(OperationKind::Face { stepover: 6. });
    o.heights.bottom = Some(-1.);
    let p = face(&s, &tool(10.), &o).unwrap();
    check_safe(&p);
    for x in 0..=50 {
        for y in 0..=50 {
            assert!(p.moves.windows(2).any(|w|{let (a,b)=(w[0].target(),w[1].target());matches!((a,b),(Some(a),Some(b)) if a[2]==-1. && b[2]==-1. && distance([x as f64,y as f64],a,b)<=5.+1e-6)}));
        }
    }
    o.heights.bottom = Some(-30.);
    assert_eq!(
        face(&s, &tool(10.), &o).unwrap_err(),
        "Bottom -30 mm is below the stock bottom -25 mm."
    );
}
#[test]
fn cut2d_offsets_by_half_the_kerf_and_cuts_holes_first() {
    let p = cut2d(
        &part(vec![rect(8., 8., 12., 10.)], 0.),
        &tool(1.),
        &op(OperationKind::Cut2d {
            kerf: 0.6,
            lead_in: 1.,
            power: 200.,
        }),
    )
    .unwrap();
    check_safe(&p);
    let loops = p
        .moves
        .split(|m| matches!(m, Move::Rapid { .. }))
        .filter(|g| !g.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(loops.len(), 2);
    let extents = |i: usize| {
        loops[i]
            .iter()
            .skip(2)
            .filter_map(Move::target)
            .map(|pt| pt[0])
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), v| {
                (a.min(v), b.max(v))
            })
    };
    let (a, b) = extents(0);
    assert!((a - 8.3).abs() < 0.011 && (b - 19.7).abs() < 0.011);
    let (a, b) = extents(1);
    assert!((a + 0.3).abs() < 0.011 && (b - 40.3).abs() < 0.011);
}
#[test]
fn drill_finds_four_holes_and_retracts_each_peck() {
    use faber_ports::{CylinderFace, FaceInfo};
    let faces = (0..5)
        .map(|x| FaceInfo {
            index: x,
            area: 50. * PI,
            centre: [f64::from(x) * 10., 0., -5.],
            normal: None,
            created_by: None,
            cylinder: Some(CylinderFace {
                start: [f64::from(x) * 10., 0., -10.],
                axis: [0., 0., 1.],
                radius: 2.5,
                length: 10.,
                internal: x < 4,
            }),
        })
        .collect::<Vec<_>>();
    let holes = find_holes(&faces, [0., 0., 1.]);
    assert_eq!(holes.len(), 4);
    let mut t = tool(5.);
    t.kind = ToolKind::Drill;
    let p = drill(
        &holes,
        &t,
        &op(OperationKind::Drill {
            holes: Holes::Auto,
            peck: Some(2.),
        }),
    )
    .unwrap();
    check_safe(&p);
    for hole in holes {
        let depths = p
            .moves
            .iter()
            .filter_map(|m| match m {
                Move::Feed { to, .. } if to[..2] == hole.centre && to[2] < 0. => Some(to[2]),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(depths, [-2., -4., -6., -8., -10.]);
    }
}

#[test]
fn contour_on_a_selected_floor_uses_only_its_outline() {
    let mesh = part(vec![rect(2., 3., 14., 20.), rect(24., 3., 14., 20.)], 5.);
    let selected = faber_ports::Mesh {
        vertices: vec![
            [2., 3., -5.],
            [16., 3., -5.],
            [16., 23., -5.],
            [2., 23., -5.],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        normals: vec![],
    };
    let o = op(OperationKind::Contour {
        z: Selection::Z(-5.),
        side: Side::Inside,
        stock_to_leave: 0.,
    });
    let path = contour_on_face(&mesh, &tool(4.), &o, &selected).unwrap();
    for p in path.moves.iter().filter_map(Move::target) {
        assert!((3.99..=14.01).contains(&p[0]) && (4.99..=21.01).contains(&p[1]));
    }
}

#[test]
fn drill_recognises_split_bores_and_rejects_open_scallops() {
    use faber_ports::{CylinderFace, FaceInfo};
    let half = |id, x: f64, side: f64, backwards: bool| FaceInfo {
        index: id,
        area: 25. * PI,
        centre: [x + side * 5. / PI, 2., -5.],
        normal: None,
        created_by: None,
        cylinder: Some(CylinderFace {
            start: [x, 2., if backwards { 0. } else { -10. }],
            axis: [0., 0., if backwards { -1. } else { 1. }],
            radius: 2.5,
            length: 10.,
            internal: true,
        }),
    };
    let faces = [
        half(1, 10., 1., false),
        half(2, 30., 1., false),
        half(3, 10., -1., true),
        half(4, 50., -1., false),
        half(5, 50., -1., true),
    ];
    let holes = find_holes(&faces, [0., 0., 1.]);
    assert_eq!(
        holes,
        [Hole {
            centre: [10., 2.],
            diameter: 5.,
            top: 0.,
            bottom: -10.
        }]
    );
}

#[test]
fn a_kerf_wider_than_the_gap_cannot_merge_two_parts() {
    use faber_ports::Mesh;
    let mesh = Mesh {
        vertices: vec![
            [0., 0., 0.],
            [10., 0., 0.],
            [10., 10., 0.],
            [0., 10., 0.],
            [10.4, 0., 0.],
            [20.4, 0., 0.],
            [20.4, 10., 0.],
            [10.4, 10., 0.],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
        normals: vec![],
    };
    let o = op(OperationKind::Cut2d {
        kerf: 0.8,
        lead_in: 0.,
        power: 10.,
    });
    assert!(cut2d(&mesh, &tool(0.8), &o).unwrap_err().contains("kerf"));
}
