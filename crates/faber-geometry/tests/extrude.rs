use faber_geometry::NativeKernel;
use faber_model::{Geometry, Profile};
use faber_ports::{Kernel, Mesh};
use glam::DVec3;
use std::collections::BTreeMap;

fn rectangle(w: f64, h: f64) -> Vec<[f64; 2]> {
    vec![[0., 0.], [w, 0.], [w, h], [0., h]]
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
    let mut edges = BTreeMap::<(u32, u32), (u32, i32)>::new();
    for [a, b, c] in &mesh.triangles {
        for (u, v) in [(*a, *b), (*b, *c), (*c, *a)] {
            let e = edges.entry((u.min(v), u.max(v))).or_default();
            e.0 += 1;
            e.1 += if u < v { 1 } else { -1 };
        }
    }
    assert!(
        edges.values().all(|e| *e == (2, 0)),
        "Open or reversed edge: {edges:?}"
    );
}

#[test]
fn box_has_known_volume_and_closed_edges() {
    let mesh = NativeKernel
        .build(&Geometry::Extrude {
            profile: Profile {
                outer: rectangle(10., 20.),
                holes: vec![],
            },
            height: 30.,
        })
        .unwrap();
    assert!((volume(&mesh) - 6000.).abs() < 1e-8);
    closed(&mesh);
}

#[test]
fn hole_stays_open_with_either_winding() {
    let hole = vec![[2., 2.], [8., 2.], [8., 18.], [2., 18.]];
    for reverse in [false, true] {
        let mut outer = rectangle(10., 20.);
        if reverse {
            outer.reverse();
        }
        let mesh = NativeKernel
            .build(&Geometry::Extrude {
                profile: Profile {
                    outer,
                    holes: vec![hole.clone()],
                },
                height: 30.,
            })
            .unwrap();
        assert!((volume(&mesh) - 3120.).abs() < 1e-8);
        closed(&mesh);
    }
}

#[test]
fn malformed_profiles_are_refused() {
    for profile in [
        Profile {
            outer: vec![[0., 0.], [1., 1.], [0., 1.], [1., 0.]],
            holes: vec![],
        },
        Profile {
            outer: rectangle(10., 20.),
            holes: vec![rectangle(30., 30.)],
        },
        Profile {
            outer: vec![[f64::NAN, 0.], [1., 0.], [0., 1.]],
            holes: vec![],
        },
    ] {
        assert!(
            NativeKernel
                .build(&Geometry::Extrude {
                    profile,
                    height: 2.
                })
                .is_err()
        );
    }
}

#[test]
fn blind_pocket_has_a_floor_and_closed_edges() {
    let mesh = NativeKernel
        .build(&Geometry::Pocket {
            profile: Profile {
                outer: rectangle(10., 20.),
                holes: vec![vec![[2., 2.], [8., 2.], [8., 18.], [2., 18.]]],
            },
            height: 30.,
            floor: 4.,
        })
        .unwrap();
    assert!((volume(&mesh) - (6000. - 96. * 26.)).abs() < 1e-8);
    closed(&mesh);
}

#[test]
fn a_point_on_a_straight_edge_does_not_open_the_solid() {
    let mesh = NativeKernel
        .build(&Geometry::Extrude {
            profile: Profile {
                outer: vec![[10., 10.], [0., 10.], [0., 0.], [10., 0.], [10., 5.]],
                holes: vec![vec![[2., 2.], [8., 2.], [8., 8.], [2., 8.]]],
            },
            height: 10.,
        })
        .unwrap();
    closed(&mesh);
    assert!((volume(&mesh) - 640.).abs() < 1e-8);
}
