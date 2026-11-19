use faber_form::{Cage, FormOp, Select, apply};
use faber_model::{CageData, SymmetryPlane};
use glam::DVec3;
use std::collections::BTreeSet;

// A box with known indexes. Each layer has (nx + 1) x (ny + 1) vertices.
fn grid(nx: u32, ny: u32) -> Cage {
    let mut vertices = Vec::new();
    let mut faces = Vec::new();
    let id = |x, y, z| x + (nx + 1) * (y + (ny + 1) * z);
    for z in 0..=1 {
        for y in 0..=ny {
            for x in 0..=nx {
                vertices.push([
                    20. * f64::from(x) / f64::from(nx) - 10.,
                    20. * f64::from(y) / f64::from(ny) - 10.,
                    20. * f64::from(z) - 10.,
                ]);
            }
        }
    }
    for y in 0..ny {
        for x in 0..nx {
            let [a, b, c, d] = [
                id(x, y, 0),
                id(x + 1, y, 0),
                id(x + 1, y + 1, 0),
                id(x, y + 1, 0),
            ];
            let k = id(0, 0, 1);
            faces.push(vec![a, d, c, b]);
            faces.push(vec![a + k, b + k, c + k, d + k]);
        }
    }
    let k = id(0, 0, 1);
    let mut wall = |a, b| faces.push(vec![a, b, b + k, a + k]);
    for x in 0..nx {
        wall(id(x, 0, 0), id(x + 1, 0, 0));
        wall(id(x + 1, ny, 0), id(x, ny, 0));
    }
    for y in 0..ny {
        wall(id(0, y + 1, 0), id(0, y, 0));
        wall(id(nx, y, 0), id(nx, y + 1, 0));
    }
    Cage::from_data(&CageData {
        vertices,
        faces,
        creases: vec![],
    })
    .unwrap()
}
fn volume(c: &Cage) -> f64 {
    let d = c.data();
    let p = |id: u32| DVec3::from(d.vertices[id as usize]);
    d.faces
        .iter()
        .map(|f| {
            (1..f.len() - 1)
                .map(|i| p(f[0]).dot(p(f[i]).cross(p(f[i + 1]))) / 6.)
                .sum::<f64>()
        })
        .sum()
}
fn mirrored(c: &Cage) {
    let keys: BTreeSet<_> = c
        .data()
        .vertices
        .iter()
        .map(|p| p.map(|a| (a * 1e7).round() as i64))
        .collect();
    for [x, y, z] in &keys {
        assert!(keys.contains(&[-x, *y, *z]));
    }
}
#[test]
fn extrude_top_by_ten() {
    let c = grid(1, 1);
    let out = apply(
        &c,
        &[FormOp::Extrude {
            faces: vec![1],
            distance: 10.,
        }],
        &[],
    )
    .unwrap();
    assert_eq!(out.data().vertices.len(), 12);
    assert_eq!(out.data().vertices[..8], c.data().vertices);
    assert!((volume(&out) - 12000.).abs() < 1e-7);
    assert_eq!(out.euler(), 2);
}
#[test]
fn joined_faces_extrude_as_one() {
    let out = apply(
        &grid(2, 1),
        &[FormOp::Extrude {
            faces: vec![1, 3],
            distance: 10.,
        }],
        &[],
    )
    .unwrap();
    assert_eq!(out.data().faces.len(), 16);
    assert_eq!(out.data().vertices.len(), 18);
    assert!((volume(&out) - 12000.).abs() < 1e-7);
}
#[test]
fn inset_then_extrude_makes_a_boss() {
    let out = apply(
        &grid(1, 1),
        &[
            FormOp::Inset {
                faces: vec![1],
                distance: 2.,
            },
            FormOp::Extrude {
                faces: vec![1],
                distance: 5.,
            },
        ],
        &[],
    )
    .unwrap();
    assert!((volume(&out) - (8000. + 16. * 16. * 5.)).abs() < 1e-7);
    assert_eq!(out.euler(), 2);
    assert!(
        apply(
            &grid(1, 1),
            &[FormOp::Inset {
                faces: vec![1],
                distance: 11.
            }],
            &[]
        )
        .is_err()
    );
}

#[test]
fn joined_inset_keeps_the_grid_between_the_offset_corners() {
    let cage = grid(3, 3);
    let top: Vec<_> = (0..9).map(|i| 2 * i + 1).collect();
    let out = apply(
        &cage,
        &[
            FormOp::Inset {
                faces: top.clone(),
                distance: 8.,
            },
            FormOp::Extrude {
                faces: top.clone(),
                distance: 2.,
            },
        ],
        &[],
    )
    .unwrap();
    for f in top {
        for &v in &out.data().faces[f as usize] {
            let [x, y, z] = out.data().vertices[v as usize];
            assert!(x.abs() <= 2.0000001 && y.abs() <= 2.0000001 && z == 12.);
        }
    }
    assert!((volume(&out) - 8032.).abs() < 1e-7);
}
#[test]
fn insert_loop_keeps_the_shape_and_splits_its_crease() {
    let c = grid(1, 1);
    let out = apply(
        &c,
        &[
            FormOp::Crease {
                edges: vec![[0, 1]],
                sharpness: 3.,
            },
            FormOp::InsertLoop {
                edge: [1, 0],
                at: 0.25,
            },
        ],
        &[],
    )
    .unwrap();
    assert_eq!(out.data().vertices.len(), 12);
    assert_eq!(out.data().creases.len(), 2);
    assert!(
        out.data().vertices[8..]
            .iter()
            .all(|p| (p[0] - 5.).abs() < 1e-9)
    );
    assert!((volume(&out) - 8000.).abs() < 1e-7);
    let symmetric = apply(
        &c,
        &[FormOp::InsertLoop {
            edge: [1, 0],
            at: 0.25,
        }],
        &[SymmetryPlane::X],
    )
    .unwrap();
    assert_eq!(symmetric.data().vertices.len(), 16);
    mirrored(&symmetric);
}

#[test]
fn intersecting_mirror_loops_share_the_new_face_centre() {
    let mut data = grid(1, 1).to_data();
    let rotation = glam::DQuat::from_rotation_z(std::f64::consts::FRAC_PI_4);
    for p in &mut data.vertices {
        *p = (rotation * DVec3::from(*p)).into();
    }
    let c = Cage::from_data(&data).unwrap();
    let out = apply(
        &c,
        &[FormOp::InsertLoop {
            edge: [0, 1],
            at: 0.5,
        }],
        &[SymmetryPlane::X],
    )
    .unwrap();
    assert_eq!(out.data().vertices.len(), 18);
    assert_eq!(out.data().faces.len(), 16);
    assert!(
        out.data()
            .vertices
            .iter()
            .any(|p| DVec3::from(*p).distance(DVec3::Z * 10.) < 1e-9)
    );
    assert!((volume(&out) - 8000.).abs() < 1e-7);
    assert_eq!(out.euler(), 2);
}
#[test]
fn bridge_makes_a_hole() {
    let c = grid(3, 3);
    let out = apply(&c, &[FormOp::Bridge { a: 8, b: 9 }], &[]).unwrap();
    assert_eq!(c.euler(), 2);
    assert_eq!(out.euler(), 0);
    assert!((volume(&out) - 8000. * 8. / 9.).abs() < 1e-7);
}
#[test]
fn delete_is_refused() {
    let err = apply(&grid(1, 1), &[FormOp::Delete { faces: vec![1] }], &[]).unwrap_err();
    assert_eq!(err, "The cage is not closed after this operation.");
}
#[test]
fn symmetry_mirrors_a_move() {
    let c = grid(1, 1);
    let out = apply(
        &c,
        &[FormOp::Move {
            select: Select::Vertices(vec![7]),
            by: [2., 1., 0.],
        }],
        &[SymmetryPlane::X],
    )
    .unwrap();
    assert_eq!(out.data().vertices[7], [12., 11., 10.]);
    assert_eq!(out.data().vertices[6], [-12., 11., 10.]);
    mirrored(&out);
}
#[test]
fn move_keeps_plane_vertices_on_the_plane() {
    let c = grid(2, 1);
    let out = apply(
        &c,
        &[FormOp::Move {
            select: Select::Vertices(vec![1]),
            by: [4., 1., 0.],
        }],
        &[SymmetryPlane::X],
    )
    .unwrap();
    assert_eq!(out.data().vertices[1], [0., -9., -10.]);
}
#[test]
fn flatten_puts_vertices_on_the_plane() {
    let c = grid(1, 1);
    let out = apply(
        &c,
        &[FormOp::Flatten {
            select: Select::Faces(vec![1]),
            plane: [0., 0., 2., 12.],
        }],
        &[],
    )
    .unwrap();
    assert!(out.data().vertices[4..].iter().all(|p| p[2] == 6.));
}
#[test]
fn weld_collapses_an_edge_and_preserves_the_closed_fan() {
    let out = apply(&grid(1, 1), &[FormOp::Weld { a: 0, b: 1 }], &[]).unwrap();
    assert_eq!(out.data().vertices.len(), 7);
    assert_eq!(out.euler(), 2);
    assert_eq!(out.data().vertices[0], [0., -10., -10.]);
}

#[test]
fn a_weld_then_extrusion_keeps_the_identity_of_the_original_vertices() {
    let c = grid(1, 1);
    let ops = [
        FormOp::Weld { a: 0, b: 1 },
        FormOp::Extrude {
            faces: vec![1],
            distance: 2.,
        },
        FormOp::Move {
            select: Select::Vertices(vec![1]),
            by: [0., 1., 0.],
        },
    ];
    let (out, origins) = faber_form::edit::apply_with_origins(&c, &ops, &[]).unwrap();
    assert_eq!(origins.len(), 11);
    assert_eq!(
        origins[..7],
        [None, Some(2), Some(3), Some(4), Some(5), Some(6), Some(7)]
    );
    assert_eq!(origins[7..], [None; 4]);
    assert_eq!(out.data().vertices[1], [-10., 11., -10.]);
    assert_eq!(out.to_data(), apply(&c, &ops, &[]).unwrap().to_data());
}
#[test]
fn scale_and_rotate_reflect_the_transformed_points() {
    let c = grid(1, 1);
    let out = apply(
        &c,
        &[
            FormOp::Scale {
                select: Select::Edges(vec![[3, 7]]),
                factor: [1.2, 1., 1.],
                about: Some([0.; 3]),
            },
            FormOp::Rotate {
                select: Select::Vertices(vec![7]),
                axis: [0., 0., 1.],
                degrees: 30.,
                about: Some([0.; 3]),
            },
        ],
        &[SymmetryPlane::X],
    )
    .unwrap();
    mirrored(&out);
    assert!((out.data().vertices[7][0] - (12. * 30_f64.to_radians().cos() - 5.)).abs() < 1e-9);
}
#[test]
fn mirrored_caps_form_one_joined_patch_on_the_plane() {
    let out = apply(
        &grid(2, 1),
        &[FormOp::Extrude {
            faces: vec![1],
            distance: 3.,
        }],
        &[SymmetryPlane::X],
    )
    .unwrap();
    assert_eq!(out.data().faces.len(), 16);
    assert!((volume(&out) - 9200.).abs() < 1e-7);
    mirrored(&out);
}

#[test]
fn edit_inputs_refuse_bad_indexes_and_degenerate_directions() {
    let c = grid(1, 1);
    for raw in [
        serde_json::json!({"op":"rotate","select":{"faces":[1]},"axis":[0,0,0],"degrees":30}),
        serde_json::json!({"op":"move","select":{"edges":[[0,7]]},"by":[0,0,2]}),
        serde_json::json!({"op":"flatten","select":{"vertices":[]},"plane":[0,0,1,0]}),
        serde_json::json!({"op":"insert_loop","edge":[10,11],"at":0.5}),
        serde_json::json!({"op":"bridge","a":1,"b":500}),
        serde_json::json!({"op":"weld","a":99999,"b":1}),
        serde_json::json!({"op":"extrude","faces":[6],"distance":1}),
        serde_json::json!({"op":"crease","edges":[[0,1]],"sharpness":11}),
    ] {
        let op: FormOp = serde_json::from_value(raw).unwrap();
        assert!(apply(&c, &[op], &[SymmetryPlane::X]).is_err());
    }
    assert!(
        serde_json::from_value::<FormOp>(
            serde_json::json!({"op":"move","select":{"vertices":[1]},"by":[0,0,1],"unused":true})
        )
        .is_err()
    );
}

#[test]
fn the_transform_refuses_a_collapsed_edge_but_allows_a_mesh_crossing() {
    let cage = grid(1, 1);
    let moving = Select::Vertices(vec![7]);
    let collapse = FormOp::Move {
        select: moving.clone(),
        by: [-20., 0., 0.],
    };
    let err = apply(&cage, &[collapse], &[]).unwrap_err();
    assert!(err.contains("6") && err.contains("7"), "{err}");
    let invalid = FormOp::Move {
        select: moving.clone(),
        by: [f64::NAN, 0., 0.],
    };
    assert!(apply(&cage, &[invalid], &[]).is_err());
    let through = FormOp::Move {
        select: moving,
        by: [-10., -10., -30.],
    };
    assert!(apply(&cage, &[through], &[]).is_ok());
}
