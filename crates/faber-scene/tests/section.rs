use faber_geometry::NativeKernel;
use faber_model::{Geometry, Profile};
use faber_ports::Kernel;
use faber_scene::{Section, section};
use glam::DVec3;

#[test]
fn plane_tangent_to_a_retained_edge_does_not_add_a_cap() {
    let mesh = NativeKernel
        .build(&Geometry::Extrude {
            profile: Profile {
                outer: vec![[0., 0.], [10., 0.], [10., 10.], [0., 10.]],
                holes: vec![],
            },
            height: 10.,
        })
        .unwrap();
    let cut = section(
        &mesh,
        Section {
            normal: [1., 1., 0.],
            offset: 0.,
            reverse: true,
        },
    )
    .unwrap();
    assert!(cut.cap.triangles.is_empty());
    assert_eq!(cut.surface.triangles.len(), mesh.triangles.len());
}

#[test]
fn section_fills_the_wall_but_keeps_the_hole() {
    let mesh = NativeKernel
        .build(&Geometry::Extrude {
            profile: Profile {
                outer: vec![[-10., -10.], [10., -10.], [10., 10.], [-10., 10.]],
                holes: vec![vec![[-8., -8.], [8., -8.], [8., 8.], [-8., 8.]]],
            },
            height: 30.,
        })
        .unwrap();
    for reverse in [false, true] {
        let cut = section(
            &mesh,
            Section {
                normal: [0., 0., 1.],
                offset: 15.,
                reverse,
            },
        )
        .unwrap();
        let mut area = 0.;
        for t in &cut.cap.triangles {
            let [a, b, c] = t.map(|i| DVec3::from(cut.cap.vertices[i as usize]));
            let n = (b - a).cross(c - a);
            area += n.length() / 2.;
            assert_eq!(n.z > 0., !reverse);
            let p = (a + b + c) / 3.;
            assert!(p.x.abs() >= 8. - 1e-8 || p.y.abs() >= 8. - 1e-8);
        }
        assert!((area - 144.).abs() < 1e-7, "Cap area: {area}");
        assert!(cut.surface.vertices.iter().all(|p| if reverse {
            p[2] >= 15. - 1e-8
        } else {
            p[2] <= 15. + 1e-8
        }));
    }
    assert_eq!(mesh.vertices.len(), 16);
    let outside = section(
        &mesh,
        Section {
            normal: [0., 0., 1.],
            offset: -1.,
            reverse: false,
        },
    )
    .unwrap();
    assert!(outside.surface.triangles.is_empty());
}

#[test]
fn oblique_section_points_lie_on_the_plane() {
    let mesh = NativeKernel
        .build(&Geometry::Extrude {
            profile: Profile {
                outer: vec![[0., 0.], [10., 0.], [10., 10.], [0., 10.]],
                holes: vec![],
            },
            height: 10.,
        })
        .unwrap();
    let n = DVec3::new(1., 1., 1.).normalize();
    let cut = section(
        &mesh,
        Section {
            normal: n.to_array(),
            offset: 8.,
            reverse: false,
        },
    )
    .unwrap();
    assert!(!cut.cap.triangles.is_empty());
    assert!(
        cut.cap
            .vertices
            .iter()
            .all(|p| (n.dot(DVec3::from(*p)) - 8.).abs() < 1e-7)
    );
}

#[test]
fn vertical_cut_of_a_hexagonal_bin_keeps_its_floor() {
    let ring = |r: f64| {
        (0..6)
            .map(|i| {
                let a = (i as f64 * 60.).to_radians();
                [r * a.cos(), r * a.sin()]
            })
            .collect()
    };
    let mesh = NativeKernel
        .build(&Geometry::Pocket {
            profile: Profile {
                outer: ring(150.),
                holes: vec![ring(150. - 2.4 / (std::f64::consts::PI / 6.).cos())],
            },
            height: 340.,
            floor: 4.,
        })
        .unwrap();
    for reverse in [false, true] {
        let cut = section(
            &mesh,
            Section {
                normal: [0., 1., 0.],
                offset: 0.,
                reverse,
            },
        )
        .unwrap();
        assert!(!cut.cap.triangles.is_empty());
        let area: f64 = cut
            .cap
            .triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| DVec3::from(cut.cap.vertices[i as usize]));
                (b - a).cross(c - a).length() / 2.
            })
            .sum();
        let expected = 300. * 340. - 2. * (150. - 2.4 / (std::f64::consts::PI / 6.).cos()) * 336.;
        assert!((area - expected).abs() < 1e-6, "{area} != {expected}");
    }
}
