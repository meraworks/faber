use faber_kernel_occt::OcctKernel;
use faber_model::Placement;
use faber_ports::{Combine, Kernel};

#[test]
fn curved_surface_normals_are_radial_and_caps_are_planar() {
    let kernel = OcctKernel;
    let cylinder = kernel.cylinder_shape(10., 20.).unwrap();
    let mesh = kernel.tessellate(&cylinder, 0.03).unwrap();
    assert_eq!(mesh.normals.len(), mesh.vertices.len());
    let mut radial = 0;
    let mut caps = 0;
    for (p, n) in mesh.vertices.iter().zip(&mesh.normals) {
        if n[2].abs() < 1e-8 {
            assert!((n[0] - p[0] / 10.).abs() < 1e-6);
            assert!((n[1] - p[1] / 10.).abs() < 1e-6);
            radial += 1;
        } else {
            assert!(n[0].abs() < 1e-8 && n[1].abs() < 1e-8);
            assert!((n[2] - if p[2] < 1. { -1. } else { 1. }).abs() < 1e-8);
            caps += 1;
        }
    }
    assert!(radial > 50 && caps > 50);
}

#[test]
fn small_curved_surfaces_have_fine_angular_steps() {
    let kernel = OcctKernel;
    let cylinder = kernel.cylinder_shape(1., 2.).unwrap();
    let mesh = kernel.tessellate(&cylinder, 0.03).unwrap();
    let mut angles: Vec<_> = mesh
        .vertices
        .iter()
        .filter(|p| (p[0] * p[0] + p[1] * p[1] - 1.).abs() < 1e-7 && p[2].abs() < 1e-7)
        .map(|p| p[1].atan2(p[0]).rem_euclid(std::f64::consts::TAU))
        .collect();
    angles.sort_by(f64::total_cmp);
    angles.dedup_by(|a, b| (*a - *b).abs() < 1e-7);
    assert!(
        angles.len() >= 90,
        "Only {} segments around the cylinder",
        angles.len()
    );
}

#[test]
fn primitives_and_booleans_have_known_exact_volumes() {
    let kernel = OcctKernel;
    let a = kernel.box_shape([10., 20., 30.]).unwrap();
    let info = kernel.properties(&a).unwrap();
    assert!((info.volume - 6000.).abs() < 1e-8);
    assert_eq!(info.faces, 6);
    assert_eq!(info.edges, 12);
    assert!(info.valid);
    let b = kernel
        .transform_shape(
            &a,
            &Placement {
                translation: [5., 0., 0.],
                rotation: [0.; 3],
            },
        )
        .unwrap();
    for (operation, expected) in [
        (Combine::Union, 9000.),
        (Combine::Difference, 3000.),
        (Combine::Intersection, 3000.),
    ] {
        let result = kernel.combine(&a, &b, operation).unwrap();
        let info = kernel.properties(&result).unwrap();
        assert!((info.volume - expected).abs() < 1e-7);
        assert!(info.valid);
        assert_eq!(info.solids, 1);
    }
    let cylinder = kernel.cylinder_shape(5., 20.).unwrap();
    assert!(
        (kernel.properties(&cylinder).unwrap().volume - std::f64::consts::PI * 500.).abs() < 1e-7
    );
    let mesh = kernel.tessellate(&cylinder, 0.1).unwrap();
    assert!(mesh.triangles.len() > 20);
}

#[test]
fn exact_shape_round_trip_keeps_volume_and_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let kernel = OcctKernel;
    let shape = kernel.box_shape([10., 20., 30.]).unwrap();
    let path = directory.path().join("box.step");
    kernel.write_shape(&shape, &path).unwrap();
    let restored = kernel.read_shape(&path).unwrap();
    let info = kernel.properties(&restored).unwrap();
    assert!((info.volume - 6000.).abs() < 1e-6);
    for i in 0..3 {
        assert!(info.bounds[0][i].abs() < 1e-6);
        assert!((info.bounds[1][i] - [10., 20., 30.][i]).abs() < 1e-6);
    }
}

#[test]
fn invalid_input_returns_an_error_and_the_process_stays_usable() {
    let kernel = OcctKernel;
    assert!(kernel.box_shape([0., 20., 30.]).is_err());
    assert!(kernel.cylinder_shape(f64::NAN, 20.).is_err());
    assert!(kernel.box_shape([1., 2., 3.]).is_ok());
}

#[test]
fn face_queries_and_shell_keep_the_open_top_and_wall_thickness() {
    let kernel = OcctKernel;
    let shape = kernel.box_shape([10., 20., 30.]).unwrap();
    let faces = kernel.faces(&shape).unwrap();
    assert_eq!(faces.len(), 6);
    assert!((faces.iter().map(|f| f.area).sum::<f64>() - 2200.).abs() < 1e-6);
    let top: Vec<_> = faces
        .iter()
        .filter(|f| f.normal.is_some_and(|n| n[2] > 0.999))
        .map(|f| f.index)
        .collect();
    assert_eq!(top.len(), 1);
    let shell = kernel.shell(&shape, &top, 1., &[]).unwrap();
    let info = kernel.properties(&shell).unwrap();
    assert!(info.valid);
    assert_eq!(info.solids, 1);
    assert!((info.volume - 1824.).abs() < 1e-5, "{}", info.volume);
    assert!(kernel.shell(&shape, &[999], 1., &[]).is_err());
    assert!(kernel.shell(&shape, &top, -1., &[]).is_err());
}

#[test]
fn edge_queries_and_dress_up_operations_use_exact_geometry() {
    let kernel = OcctKernel;
    let shape = kernel.box_shape([10., 20., 30.]).unwrap();
    let edges = kernel.edges(&shape).unwrap();
    assert_eq!(edges.len(), 12);
    assert!((edges.iter().map(|edge| edge.length).sum::<f64>() - 240.).abs() < 1e-6);
    let vertical: Vec<_> = edges
        .iter()
        .filter(|edge| edge.direction.is_some_and(|d| d[2].abs() > 0.999))
        .map(|edge| edge.index)
        .collect();
    assert_eq!(vertical.len(), 4);
    let rounded = kernel.fillet(&shape, &vertical, 1.).unwrap();
    let expected = 6000. - (4. - std::f64::consts::PI) * 30.;
    assert!((kernel.properties(&rounded).unwrap().volume - expected).abs() < 1e-5);
    let chamfered = kernel.chamfer(&shape, &vertical, 1.).unwrap();
    assert!((kernel.properties(&chamfered).unwrap().volume - 5940.).abs() < 1e-5);
    assert!(kernel.fillet(&shape, &[9999], 1.).is_err());
    assert!(kernel.fillet(&shape, &vertical, 100.).is_err());
    assert!((kernel.properties(&shape).unwrap().volume - 6000.).abs() < 1e-6);
}

#[test]
fn curved_primitives_have_exact_volumes() {
    let kernel = OcctKernel;
    let pi = std::f64::consts::PI;
    for (shape, expected) in [
        (kernel.sphere_shape(3.).unwrap(), 36. * pi),
        (kernel.cone_shape(4., 0., 9.).unwrap(), 48. * pi),
        (kernel.cone_shape(4., 2., 9.).unwrap(), 84. * pi),
        (kernel.torus_shape(10., 2.).unwrap(), 80. * pi * pi),
    ] {
        let properties = kernel.properties(&shape).unwrap();
        assert!(properties.valid);
        assert_eq!(properties.solids, 1);
        assert!((properties.volume - expected).abs() < 1e-6);
        assert!(!kernel.tessellate(&shape, 0.1).unwrap().triangles.is_empty());
    }
    assert!(kernel.sphere_shape(-1.).is_err());
    assert!(kernel.cone_shape(0., 0., 10.).is_err());
    assert!(kernel.torus_shape(2., 3.).is_err());
}

#[test]
fn revolve_builds_full_and_partial_solids_about_an_axis() {
    let kernel = OcctKernel;
    let profile = faber_model::Profile {
        outer: vec![[2., 0.], [4., 0.], [4., 10.], [2., 10.]],
        holes: vec![],
    };
    for angle in [90., 180., 360.] {
        let shape = kernel
            .revolve(&profile, [0.; 3], [0., 1., 0.], angle)
            .unwrap();
        let properties = kernel.properties(&shape).unwrap();
        assert!(properties.valid);
        assert_eq!(properties.solids, 1);
        assert!((properties.volume - 120. * std::f64::consts::PI * angle / 360.).abs() < 1e-6);
    }
    assert!(kernel.revolve(&profile, [0.; 3], [0.; 3], 360.).is_err());
    assert!(
        kernel
            .revolve(&profile, [0.; 3], [0., 1., 0.], 361.)
            .is_err()
    );
}

#[test]
fn loft_preserves_sections_and_holes() {
    let kernel = OcctKernel;
    let square = |half: f64| vec![[-half, -half], [half, -half], [half, half], [-half, half]];
    let profile = |outer, hole| faber_model::Profile {
        outer: square(outer),
        holes: if hole > 0. {
            vec![square(hole)]
        } else {
            vec![]
        },
    };
    for holes in [false, true] {
        let sections = vec![
            (
                profile(5., if holes { 2. } else { 0. }),
                Placement::default(),
            ),
            (
                profile(10., if holes { 4. } else { 0. }),
                Placement {
                    translation: [0., 0., 30.],
                    rotation: [0.; 3],
                },
            ),
        ];
        for ruled in [false, true] {
            let shape = kernel.loft(&sections, ruled).unwrap();
            let info = kernel.properties(&shape).unwrap();
            let expected = if holes { 5880. } else { 7000. };
            assert!(info.valid);
            assert_eq!(info.solids, 1);
            assert!((info.volume - expected).abs() < 1e-5, "{}", info.volume);
        }
    }
    assert!(kernel.loft(&[], false).is_err());
    assert!(
        kernel
            .loft(&[(profile(5., 0.), Placement::default())], false)
            .is_err()
    );
}

#[test]
fn sweep_keeps_profile_area_along_straight_and_corner_paths() {
    let kernel = OcctKernel;
    let mut profile = faber_model::Profile {
        outer: vec![[-2., -2.], [2., -2.], [2., 2.], [-2., 2.]],
        holes: vec![],
    };
    let placement = Placement::default();
    for hollow in [false, true] {
        if hollow {
            profile
                .holes
                .push(vec![[-1., -1.], [1., -1.], [1., 1.], [-1., 1.]]);
        }
        for (path, length) in [
            (vec![[0., 0., 0.], [0., 0., 30.]], 30.),
            (vec![[0., 0., 0.], [0., 0., 30.], [20., 0., 30.]], 50.),
        ] {
            let shape = kernel.sweep(&profile, &placement, &path).unwrap();
            let info = kernel.properties(&shape).unwrap();
            assert!(info.valid);
            assert_eq!(info.solids, 1);
            let area = if hollow { 12. } else { 16. };
            assert!(
                (info.volume - area * length).abs() < 1e-5,
                "{}",
                info.volume
            );
        }
    }
    assert!(kernel.sweep(&profile, &placement, &[[0.; 3]]).is_err());
    assert!(
        kernel
            .sweep(&profile, &placement, &[[0.; 3], [0.; 3]])
            .is_err()
    );
}

#[test]
fn hexagonal_pocket_round_trip_keeps_the_floor_and_wall_volume() {
    use faber_model::{Geometry, Profile};
    let ring = |radius: f64| {
        (0..6)
            .map(|i| {
                let angle = f64::from(i) * std::f64::consts::PI / 3.;
                [radius * angle.cos(), radius * angle.sin()]
            })
            .collect()
    };
    let outer_radius = 150.;
    let inner_radius = outer_radius - 2.4 / (std::f64::consts::PI / 6.).cos();
    let area = |radius: f64| 3. * 3_f64.sqrt() * radius * radius / 2.;
    let expected = area(outer_radius) * 340. - area(inner_radius) * 336.;
    let kernel = OcctKernel;
    let shape = kernel
        .exact_shape(&Geometry::Pocket {
            profile: Profile {
                outer: ring(outer_radius),
                holes: vec![ring(inner_radius)],
            },
            height: 340.,
            floor: 4.,
        })
        .unwrap()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("bin.step");
    kernel.write_shape(&shape, &path).unwrap();
    let restored = kernel.read_shape(&path).unwrap();
    for solid in [&shape, &restored] {
        let info = kernel.properties(solid).unwrap();
        assert!(info.valid);
        assert_eq!(info.solids, 1);
        assert!((info.volume - expected).abs() < 1e-4);
        for (axis, expected_size) in [300., 150. * 3_f64.sqrt(), 340.].into_iter().enumerate() {
            let size = info.bounds[1][axis] - info.bounds[0][axis];
            assert!((size - expected_size).abs() < 1e-5);
        }
    }
}

#[test]
fn mass_properties_have_known_box_and_cylinder_values() {
    let kernel = OcctKernel;
    let a = kernel.box_shape([10., 20., 30.]).unwrap();
    let placed = kernel
        .transform_shape(
            &a,
            &Placement {
                translation: [5., 0., 0.],
                rotation: [0.; 3],
            },
        )
        .unwrap();
    let props = kernel.mass_properties(&placed).unwrap();
    assert!((props.volume_mm3 - 6000.).abs() < 1e-7);
    assert!((props.area_mm2 - 2200.).abs() < 1e-7);
    for (actual, expected) in props.centre_of_mass_mm.into_iter().zip([10., 10., 15.]) {
        assert!((actual - expected).abs() < 1e-7);
    }
    let inertia = props.inertia_unit_mm5;
    for (actual, expected) in [(0, 650000.), (1, 500000.), (2, 250000.)] {
        assert!((inertia[actual][actual] - expected).abs() < 1e-3);
    }
    for (row, column) in [(0, 1), (0, 2), (1, 2)] {
        assert!(inertia[row][column].abs() < 1e-3);
        assert!((inertia[column][row] - inertia[row][column]).abs() < 1e-9);
    }
    let cylinder = kernel.cylinder_shape(10., 20.).unwrap();
    let props = kernel.mass_properties(&cylinder).unwrap();
    assert!((props.volume_mm3 - std::f64::consts::PI * 2000.).abs() < 1e-6);
    let expected_area = 2. * std::f64::consts::PI * 100. + 2. * std::f64::consts::PI * 10. * 20.;
    assert!((props.area_mm2 - expected_area).abs() < 1e-6);
}

#[test]
fn intersection_reports_an_empty_common_part_explicitly() {
    let kernel = OcctKernel;
    let a = kernel.box_shape([10., 10., 10.]).unwrap();
    let far = kernel
        .transform_shape(
            &a,
            &Placement {
                translation: [20., 0., 0.],
                rotation: [0.; 3],
            },
        )
        .unwrap();
    assert!(kernel.intersect_solids(&a, &far).unwrap().is_none());
    let near = kernel
        .transform_shape(
            &a,
            &Placement {
                translation: [5., 0., 0.],
                rotation: [0.; 3],
            },
        )
        .unwrap();
    let common = kernel.intersect_solids(&a, &near).unwrap().unwrap();
    assert!((kernel.properties(&common).unwrap().volume - 500.).abs() < 1e-7);
}

#[test]
fn cylindrical_face_query_distinguishes_bore_and_shaft() {
    let kernel = OcctKernel;
    let shaft = kernel.cylinder_shape(6., 12.).unwrap();
    let bore = kernel.cylinder_shape(3., 12.).unwrap();
    let sleeve = kernel.combine(&shaft, &bore, Combine::Difference).unwrap();
    let cylinders: Vec<_> = kernel
        .faces(&sleeve)
        .unwrap()
        .into_iter()
        .filter_map(|f| f.cylinder)
        .collect();
    assert_eq!(cylinders.len(), 2);
    for surface in cylinders {
        assert_eq!(surface.radius, if surface.internal { 3. } else { 6. });
        assert_eq!(surface.start, [0.; 3]);
        assert_eq!(surface.axis, [0., 0., 1.]);
        assert!((surface.length - 12.).abs() < 1e-9);
    }
}
