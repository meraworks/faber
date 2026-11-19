use faber_form::*;
fn cube() -> CageData<f64> {
    form_box([20.; 3], [1; 3])
}
#[test]
fn box_cage_counts() {
    let c = Cage::from_data(&cube()).unwrap();
    assert_eq!(
        (
            c.data().vertices.len(),
            c.data().faces.len(),
            c.edges().len(),
            c.euler()
        ),
        (8, 6, 12, 2)
    );
}
#[test]
fn torus_euler_is_zero() {
    let c = Cage::from_data(&form_torus(6., 2., 12, 8)).unwrap();
    assert_eq!(c.euler(), 0);
}
#[test]
fn round_trip_keeps_the_order() {
    let mut d = cube();
    d.creases.push(Crease {
        edge: [d.faces[0][1], d.faces[0][0]],
        sharpness: 2.5,
    });
    assert_eq!(Cage::from_data(&d).unwrap().to_data(), d);
}
#[test]
fn bad_cage_is_refused() {
    let mut d = cube();
    d.faces[3] = vec![5, 2, 5];
    assert_eq!(
        Cage::from_data(&d).unwrap_err(),
        "Face 3 names vertex 5 two times."
    );
    let mut d = cube();
    d.vertices.push([3.; 3]);
    assert_eq!(Cage::from_data(&d).unwrap_err(), "Vertex 8 is not used.");
    let mut d = cube();
    let face = d.faces.remove(0);
    let e = Cage::from_data(&d).unwrap_err();
    assert!(e.contains("has 1 face. The cage must be closed."), "{e}");
    d.faces.push(face.clone());
    d.faces.push(face);
    let e = Cage::from_data(&d).unwrap_err();
    assert!(e.contains("has 3 faces."), "{e}");
}
#[test]
fn crease_on_a_missing_edge_is_refused() {
    let mut d = cube();
    let f = &d.faces[0];
    d.creases.push(Crease {
        edge: [f[0], f[2]],
        sharpness: 1.,
    });
    assert!(Cage::from_data(&d).unwrap_err().contains("edge"));
}
#[test]
fn inconsistent_winding_and_a_pinched_vertex_are_refused() {
    let mut d = cube();
    d.faces[0].reverse();
    assert!(Cage::from_data(&d).is_err());
    let a = cube();
    let mut p = a.clone();
    let n = p.vertices.len() as u32 - 1;
    let c = a.vertices[0];
    p.vertices.extend(
        a.vertices
            .iter()
            .skip(1)
            .map(|v| std::array::from_fn(|i| 2. * c[i] - v[i])),
    );
    p.faces.extend(a.faces.iter().map(|f| {
        f.iter()
            .rev()
            .map(|&id| if id == 0 { 0 } else { id + n })
            .collect()
    }));
    let e = Cage::from_data(&p).unwrap_err();
    assert!(e.contains("Vertex 0"), "{e}");
}
#[test]
fn closed_primitives_have_outward_faces_and_bounded_inputs() {
    for data in [
        form_cylinder(5., 12., 7, 3),
        form_sphere(10., 3),
        form_torus(10., 3., 10, 6),
        form_box([5., 4., 3.], [3, 2, 4]),
    ] {
        let c = Cage::from_data(&data).unwrap();
        assert!(
            faber_meshtools::report(&subdivide(&c, 0).unwrap())
                .volume_mm3
                .unwrap()
                > 0.
        );
    }
    assert!(try_form_box([1.; 3], [u32::MAX; 3]).is_err());
    assert!(try_form_sphere(-2., 3).is_err());
    assert!(try_form_cylinder(1., 2., 2, 1).is_err());
    assert!(try_form_torus(1., 2., 6, 8).is_err());
}
