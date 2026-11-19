use faber_core::surface::*;
use faber_geometry::bvh::Bvh;
use faber_kernel_occt::OcctKernel;
use faber_model::Placement;
use faber_ports::{Combine, CurvedMesh, FaceMesh, Kernel, Mesh, Shape};

const UP: [f64; 3] = [0., 0., 1.];

fn near(actual: f64, target: f64) {
    assert!(
        (actual - target).abs() < 1e-6,
        "{actual} differs from {target}"
    );
}

fn placed(shape: &Shape, translation: [f64; 3], rotation: [f64; 3]) -> Shape {
    OcctKernel
        .transform_shape(
            shape,
            &Placement {
                translation,
                rotation,
            },
        )
        .unwrap()
}

fn rounded_box(radius: f64) -> Shape {
    let block = OcctKernel.box_shape([10., 10., 12.]).unwrap();
    let vertical = OcctKernel
        .edges(&block)
        .unwrap()
        .into_iter()
        .filter(|e| e.direction.is_some_and(|d| d[2].abs() > 0.999))
        .map(|e| e.index)
        .collect::<Vec<_>>();
    assert_eq!(vertical.len(), 4);
    OcctKernel.fillet(&block, &vertical, radius).unwrap()
}

fn exact_report(shape: Shape) -> CurvatureReport {
    let mesh = OcctKernel.tessellate_curvature(&shape, 0.04).unwrap();
    curvature_report(&mesh, &OcctKernel.faces(&shape).unwrap())
}

fn pocket() -> CurvatureReport {
    let blank = OcctKernel.box_shape([20., 20., 10.]).unwrap();
    let cutter = placed(&rounded_box(1.5), [5., 5., 3.], [0.; 3]);
    exact_report(
        OcctKernel
            .combine(&blank, &cutter, Combine::Difference)
            .unwrap(),
    )
}

#[test]
fn pocket_corner_radius() {
    let r = pocket();
    near(r.min_concave_radius_mm.unwrap(), 1.5);
    assert_eq!(r.smallest_concave_faces.len(), 4);
}

#[test]
fn outer_fillet_is_convex() {
    let r = exact_report(rounded_box(2.));
    near(r.min_convex_radius_mm.unwrap(), 2.);
    assert!(r.min_concave_radius_mm.is_none());
}

#[test]
fn tool_three_does_not_fit_the_corner() {
    let r = pocket();
    let blocked = r.tool_faces(3.).unwrap();
    assert_eq!(blocked.len(), 4);
    for face in blocked {
        assert_eq!(face.kind, FaceKind::Cylinder);
        near(face.area, std::f64::consts::FRAC_PI_2 * 1.5 * 7.);
    }
    assert!(r.tool_faces(1.5).unwrap().is_empty());
    assert!(r.tool_faces(-1.).is_err());
}

#[test]
fn mesh_part_gets_an_estimate() {
    let sphere = OcctKernel.sphere_shape(10.).unwrap();
    let indexed = OcctKernel.tessellate_faces(&sphere, 0.05).unwrap().mesh;
    let mut soup = Mesh::default();
    for point in indexed.triangle_vertices() {
        soup.vertices
            .push([point[0] + 1000., point[1] - 50., point[2] + 900.]);
    }
    for i in (0..soup.vertices.len() as u32).step_by(3) {
        soup.triangles.push([i, i + 1, i + 2]);
    }
    soup.vertices.push([999999.; 3]);
    let curvature = checked_estimate_curvature(&soup).unwrap();
    assert_eq!(curvature.len(), soup.vertices.len());
    let mesh = CurvedMesh {
        curvature,
        kinds: Default::default(),
        faces: FaceMesh {
            face_of_triangle: vec![1; soup.triangles.len()],
            mesh: soup,
        },
    };
    let report = curvature_report(&mesh, &[]);
    let radius = report.min_convex_radius_mm.unwrap();
    assert!((radius - 10.).abs() < 0.5, "{radius}");
    assert!(report.min_concave_radius_mm.is_none());
    assert!(
        report
            .limits
            .iter()
            .any(|l| l == "Curvature of a mesh part is an estimate.")
    );
}

fn frustum(angle_deg: f64) -> FaceMesh {
    let top = 10. - 30. * angle_deg.to_radians().tan();
    let vertices = [(10., 0.), (top, 30.)]
        .into_iter()
        .flat_map(|(a, z)| [[-a, -a, z], [a, -a, z], [a, a, z], [-a, a, z]])
        .collect();
    let mut mesh = Mesh {
        vertices,
        ..Default::default()
    };
    for [a, b, c, d] in [
        [0, 3, 2, 1],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        mesh.triangles.extend([[a, b, c], [a, c, d]]);
    }
    FaceMesh {
        mesh,
        face_of_triangle: (1..=6).flat_map(|n| [n, n]).collect(),
    }
}

#[test]
fn plain_box_has_four_vertical_faces() {
    let r = draft_report(&frustum(0.), UP, 1.).unwrap();
    near(r.vertical_mm2, 4. * 20. * 30.);
    assert_eq!(
        r.faces
            .iter()
            .filter(|f| f.class == DraftClass::Vertical)
            .count(),
        4
    );
}

#[test]
fn drafted_box_passes() {
    let r = draft_report(&frustum(2.), UP, 1.).unwrap();
    near(r.vertical_mm2 + r.undercut_mm2 + r.below_minimum_mm2, 0.);
    near(r.faces[2].min_deg, 2.);
}

#[test]
fn one_degree_is_below_a_two_degree_minimum() {
    let r = draft_report(&frustum(1.), UP, 2.).unwrap();
    assert!(r.below_minimum_mm2 > 2200.);
    assert_eq!(r.triangle_classes[4], DraftClass::BelowMinimum);
}

#[test]
fn vertical_face_is_not_an_undercut() {
    let r = draft_report(&frustum(0.009), UP, 1.).unwrap();
    assert!(r.vertical_mm2 > 2300.);
    near(r.undercut_mm2, 0.);
}

#[test]
fn side_hole_is_an_undercut() {
    let block = OcctKernel.box_shape([10.; 3]).unwrap();
    let tool = placed(
        &OcctKernel.cylinder_shape(2., 12.).unwrap(),
        [-1., 5., 5.],
        [0., 90., 0.],
    );
    let cut = OcctKernel
        .combine(&block, &tool, Combine::Difference)
        .unwrap();
    let r = draft_report(&OcctKernel.tessellate_faces(&cut, 0.03).unwrap(), UP, 1.).unwrap();
    assert!(r.undercut_mm2 > 100.);
}

#[test]
fn zero_direction_is_refused() {
    assert_eq!(
        draft_report(&frustum(0.), [0.; 3], 1.).unwrap_err(),
        "The pull direction has zero length."
    );
}

#[test]
fn box_from_above_reaches_the_top_only() {
    let mesh = frustum(0.);
    let r = access_report(&mesh, &Bvh::new(&mesh.mesh).unwrap(), &[UP], None);
    near(r.reachable_mm2, 400.);
    assert_eq!(r.hidden_faces, vec![1, 3, 4, 5, 6]);
    assert!(
        r.limits
            .iter()
            .any(|l| l == "Side faces count as hidden for a direction along them.")
    );
}

#[test]
fn three_directions_reach_more() {
    let mesh = frustum(0.);
    let r = access_report(
        &mesh,
        &Bvh::new(&mesh.mesh).unwrap(),
        &[UP, [1., 0., 0.], [0., 1., 0.]],
        None,
    );
    near(r.reachable_mm2, 400. + 2. * 600.);
}

#[test]
fn t_slot_has_a_hidden_area() {
    let blank = OcctKernel.box_shape([20., 20., 20.]).unwrap();
    let slot = placed(
        &OcctKernel.box_shape([4., 22., 12.]).unwrap(),
        [8., -1., 9.],
        [0.; 3],
    );
    let channel = placed(
        &OcctKernel.box_shape([16., 22., 4.]).unwrap(),
        [2., -1., 8.],
        [0.; 3],
    );
    let tool = OcctKernel.combine(&slot, &channel, Combine::Union).unwrap();
    let part = OcctKernel
        .combine(&blank, &tool, Combine::Difference)
        .unwrap();
    let mesh = OcctKernel.tessellate_faces(&part, 0.05).unwrap();
    let r = access_report(&mesh, &Bvh::new(&mesh.mesh).unwrap(), &[UP], None);
    let floor = OcctKernel
        .faces(&part)
        .unwrap()
        .into_iter()
        .find(|f| (f.centre[2] - 8.).abs() < 1e-6 && f.normal.is_some_and(|n| n[2] > 0.99))
        .unwrap();
    assert!(r.hidden_faces.contains(&floor.index));
    assert!(r.reachable_mm2 >= 319.99);
}

#[test]
fn other_parts_block_access() {
    let mesh = frustum(0.);
    let mut cover = mesh.mesh.clone();
    for p in &mut cover.vertices {
        p[2] += 40.;
    }
    let r = access_report(
        &mesh,
        &Bvh::new(&mesh.mesh).unwrap(),
        &[UP],
        Some(&Bvh::new(&cover).unwrap()),
    );
    near(r.reachable_mm2, 0.);
    assert!(r.reachable.iter().all(|b| !b));
}

#[test]
fn malformed_geometry_and_directions_are_refused() {
    let mut mesh = frustum(0.);
    let tree = Bvh::new(&mesh.mesh).unwrap();
    assert!(checked_access_report(&mesh, &tree, &[], None).is_err());
    assert!(checked_access_report(&mesh, &tree, &[[f64::NAN, 0., 0.]], None).is_err());
    assert!(draft_report(&mesh, UP, -1.).is_err());
    mesh.face_of_triangle.pop();
    assert!(draft_report(&mesh, UP, 1.).is_err());
    mesh.mesh.triangles[0][0] = 9999;
    assert!(checked_estimate_curvature(&mesh.mesh).is_err());
}

#[test]
fn colours_keep_a_plane_neutral_with_a_curved_face_in_view() {
    let mut mesh = CurvedMesh {
        faces: frustum(0.),
        curvature: vec![[0.; 2]; 8],
        kinds: Default::default(),
    };
    mesh.curvature[1] = [-0.25, 0.];
    mesh.curvature[2] = [0., 0.25];
    assert_eq!(curvature_range(&mesh, CurvatureMode::Mean), [-0.125, 0.125]);
    let colours = curvature_colours(&mesh, CurvatureMode::Mean, None);
    assert_ne!(colours[0], colours[1]);
    assert_ne!(colours[1], colours[2]);
    let r = curvature_range(&mesh, CurvatureMode::MinRadius);
    assert_eq!(r, [4., 4.]);
    let colours = curvature_colours(&mesh, CurvatureMode::MinRadius, None);
    assert_ne!(colours[0], colours[1]);
    assert_eq!(colours[1], colours[2]);
}

#[test]
fn parting_plane_follows_a_single_flat_ring_between_opposed_tapers() {
    let vertices = [(8., 0.), (10., 10.), (8., 20.)]
        .into_iter()
        .flat_map(|(a, z)| [[-a, -a, z], [a, -a, z], [a, a, z], [-a, a, z]])
        .collect();
    let mut mesh = Mesh {
        vertices,
        ..Default::default()
    };
    let mut faces = vec![[0, 3, 2, 1], [8, 9, 10, 11]];
    for base in [0, 4] {
        for i in 0..4 {
            let a = base + i;
            let b = base + (i + 1) % 4;
            faces.push([a, b, b + 4, a + 4]);
        }
    }
    for [a, b, c, d] in faces {
        mesh.triangles.extend([[a, b, c], [a, c, d]]);
    }
    let mut faces = FaceMesh {
        face_of_triangle: (1..=10).flat_map(|n| [n, n]).collect(),
        mesh,
    };
    near(
        draft_report(&faces, UP, 1.).unwrap().parting_z.unwrap(),
        10.,
    );
    near(
        draft_report(&faces, [0., 0., -2.], 1.)
            .unwrap()
            .parting_z
            .unwrap(),
        10.,
    );
    assert!(
        draft_report(&faces, [0.4, 0., 1.], 1.)
            .unwrap()
            .parting_z
            .is_none()
    );
    assert!(
        draft_report(&frustum(0.), UP, 1.)
            .unwrap()
            .parting_z
            .is_none()
    );
    faces.mesh.vertices[5][2] += 0.5;
    assert!(draft_report(&faces, UP, 1.).unwrap().parting_z.is_none());
}

#[test]
fn moving_a_box_far_from_the_origin_keeps_its_draft_and_access() {
    let mut faces = frustum(0.);
    for p in &mut faces.mesh.vertices {
        for x in p {
            *x += 1e11;
        }
    }
    let bvh = Bvh::new(&faces.mesh).unwrap();
    let draft = draft_report(&faces, UP, 1.).unwrap();
    near(draft.undercut_mm2, 0.);
    let access = access_report(&faces, &bvh, &[UP], None);
    near(access.reachable_mm2, 400.);
}

#[test]
fn a_manual_curvature_scale_clips_values_to_both_limits() {
    for (value, extent, limit) in [(0.15, [0.2, 0.4], 0.2), (-0.15, [-0.4, -0.2], -0.2)] {
        assert_eq!(
            curvature_colour(value, CurvatureMode::Mean, extent),
            curvature_colour(limit, CurvatureMode::Mean, extent)
        );
    }
}
