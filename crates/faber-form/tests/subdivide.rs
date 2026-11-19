use faber_form::*;
fn cube() -> Cage {
    Cage::from_data(&form_box([20.; 3], [1; 3])).unwrap()
}
fn vol(c: &Cage, l: u32) -> f64 {
    faber_meshtools::report(&subdivide(c, l).unwrap())
        .volume_mm3
        .unwrap()
}
#[test]
fn level_zero_is_the_cage() {
    let c = cube();
    assert_eq!(subdivide(&c, 0).unwrap().triangles.len(), 12);
    assert!((vol(&c, 0) - 8000.).abs() < 1e-6);
}
#[test]
fn face_count_grows_by_four() {
    assert_eq!(
        subdivide(&cube(), 3).unwrap().triangles.len(),
        2 * 6 * 4usize.pow(3)
    );
}
#[test]
fn result_is_closed() {
    for l in 1..=4 {
        let r = faber_meshtools::report(&subdivide(&cube(), l).unwrap());
        assert_eq!(r.open_edges, 0);
        assert!(r.watertight);
        assert_eq!(mesh_euler(&subdivide(&cube(), l).unwrap()), 2);
    }
}
#[test]
fn result_stays_inside_the_cage() {
    for p in subdivide(&cube(), 3).unwrap().vertices {
        assert!(
            p[0].abs() <= 10. + 1e-9
                && p[1].abs() <= 10. + 1e-9
                && (-1e-9..=20. + 1e-9).contains(&p[2])
        );
    }
}
#[test]
fn result_is_symmetric() {
    let vertices = subdivide(&cube(), 2).unwrap().vertices;
    for &p in &vertices {
        for axis in 0..3 {
            let mut q = p;
            q[axis] = if axis == 2 { 20. - p[2] } else { -p[axis] };
            assert!(
                vertices
                    .iter()
                    .any(|v| (0..3).all(|i| (v[i] - q[i]).abs() < 1e-9))
            );
        }
    }
}
#[test]
fn fully_creased_cube_stays_a_cube() {
    let c = cube();
    let mut d = c.to_data();
    d.creases = c
        .edges()
        .into_iter()
        .map(|edge| Crease {
            edge,
            sharpness: 10.,
        })
        .collect();
    let c = Cage::from_data(&d).unwrap();
    assert!((vol(&c, 3) - 8000.).abs() < 1e-6);
    let m = subdivide(&c, 3).unwrap();
    for n in m.normals {
        assert_eq!(n.iter().filter(|x| x.abs() > 1e-8).count(), 1);
    }
}
#[test]
fn one_creased_loop_keeps_a_flat_end() {
    let c = cube();
    let mut d = c.to_data();
    d.creases = c
        .edges()
        .into_iter()
        .filter(|e| e.iter().all(|&v| d.vertices[v as usize][2] == 20.))
        .map(|edge| Crease {
            edge,
            sharpness: 10.,
        })
        .collect();
    let m = subdivide(&Cage::from_data(&d).unwrap(), 3).unwrap();
    assert!(
        m.vertices
            .iter()
            .filter(|p| (p[2] - 20.).abs() < 1e-9)
            .count()
            >= 81
    );
    assert!(m.vertices.iter().all(|p| p[2] > 0.));
}
#[test]
fn volume_falls_with_each_level_and_converges() {
    let c = cube();
    let v = (0..=4).map(|l| vol(&c, l)).collect::<Vec<_>>();
    assert!(v.windows(2).all(|p| p[1] < p[0]));
    assert!((v[3] - v[4]) / v[3] < 0.01, "{v:?}");
}
#[test]
fn torus_keeps_euler_zero() {
    let c = Cage::from_data(&form_torus(6., 2., 12, 8)).unwrap();
    let m = subdivide(&c, 2).unwrap();
    assert_eq!(mesh_euler(&m), 0);
}
#[test]
fn too_many_faces_are_refused() {
    let c = Cage::from_data(&form_box([20.; 3], [16; 3])).unwrap();
    assert!(subdivide(&c, 4).unwrap_err().contains("400000"));
    assert!(subdivide(&cube(), 5).is_err());
}

fn mesh_euler(mesh: &faber_ports::Mesh) -> i64 {
    use std::collections::BTreeSet;
    let (v, map) = mesh.welded_vertices(1e-9).unwrap();
    let mut edges = BTreeSet::new();
    for tri in &mesh.triangles {
        let [a, b, c] = tri.map(|i| map[i as usize]);
        for (u, v) in [(a, b), (b, c), (c, a)] {
            edges.insert((u.min(v), u.max(v)));
        }
    }
    v.len() as i64 - edges.len() as i64 + mesh.triangles.len() as i64
}

#[test]
fn one_step_uses_the_catmull_clark_stencils() {
    let c = cube();
    let data = c.data();
    let out = subdivide_once(&c).unwrap();
    let result = out.data();
    let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-10);
    let centre = [0., 0., 10.];
    for (&p, &q) in data.vertices.iter().zip(result.vertices.iter()) {
        assert!(near(
            q,
            std::array::from_fn(|k| centre[k] + (p[k] - centre[k]) * 5. / 9.)
        ));
    }
    let edges = c.edges();
    for (i, [a, b]) in edges.iter().copied().enumerate() {
        let mid = std::array::from_fn::<_, 3, _>(|k| {
            0.5 * (data.vertices[a as usize][k] + data.vertices[b as usize][k])
        });
        assert!(near(
            result.vertices[data.vertices.len() + i],
            std::array::from_fn(|k| centre[k] + (mid[k] - centre[k]) * 0.75)
        ));
    }
    for (i, f) in data.faces.iter().enumerate() {
        let expected = std::array::from_fn(|k| {
            f.iter().map(|&j| data.vertices[j as usize][k]).sum::<f64>() / 4.
        });
        assert!(near(
            result.vertices[data.vertices.len() + edges.len() + i],
            expected
        ));
    }
    let mut sharp = data.clone();
    sharp.creases.push(Crease {
        edge: edges[0],
        sharpness: 2.5,
    });
    let out = subdivide_once(&Cage::from_data(&sharp).unwrap()).unwrap();
    assert_eq!(out.data().creases.len(), 2);
    assert!(out.data().creases.iter().all(|c| c.sharpness == 1.5));
}
#[test]
fn torus_samples_preserve_the_mirror_planes() {
    use std::collections::BTreeSet;
    let cage = Cage::from_data(&form_torus(8., 2., 12, 8)).unwrap();
    let mesh = subdivide(&cage, 2).unwrap();
    let samples = mesh
        .vertices
        .iter()
        .map(|p| p.map(|x| (x * 1e8).round() as i64))
        .collect::<BTreeSet<_>>();
    for &p in &samples {
        for i in 0..3 {
            let mut q = p;
            q[i] = -q[i];
            assert!(samples.contains(&q));
        }
    }
}

#[test]
fn semisharp_edges_blend_and_then_end() {
    let c = cube();
    let edge = c.edges()[0];
    let midpoint = std::array::from_fn::<_, 3, _>(|j| {
        (c.data().vertices[edge[0] as usize][j] + c.data().vertices[edge[1] as usize][j]) * 0.5
    });
    let id = c.data().vertices.len();
    let smooth = subdivide_once(&c).unwrap().data().vertices[id];
    let mut d = c.to_data();
    d.creases.push(Crease {
        edge,
        sharpness: 0.5,
    });
    let next = subdivide_once(&Cage::from_data(&d).unwrap()).unwrap();
    assert!(next.data().creases.is_empty());
    for j in 0..3 {
        assert!((next.data().vertices[id][j] - (smooth[j] + midpoint[j]) * 0.5).abs() < 1e-9);
    }
}

#[test]
fn creased_mesh_and_shared_topology_have_identical_triangles() {
    let original = cube();
    let mut d = original.to_data();
    d.creases = original
        .edges()
        .iter()
        .map(|&edge| Crease {
            edge,
            sharpness: 10.,
        })
        .collect();
    let cage = Cage::from_data(&d).unwrap();
    for levels in [0, 2] {
        let display = subdivide(&cage, levels).unwrap();
        let joined = subdivide_topology(&cage, levels).unwrap();
        assert!(display.vertices.len() > joined.vertices.len());
        assert_eq!(
            display.triangle_vertices().collect::<Vec<_>>(),
            joined.triangle_vertices().collect::<Vec<_>>()
        );
        assert!((faber_meshtools::report(&joined).volume_mm3.unwrap() - 8000.).abs() < 1e-6);
        let mut edges = std::collections::BTreeMap::new();
        for &[a, b, c] in &joined.triangles {
            for (v, w) in [(a, b), (b, c), (c, a)] {
                *edges.entry([v.min(w), v.max(w)]).or_insert(0) += 1;
            }
        }
        assert!(edges.values().all(|&count| count == 2));
    }
}
