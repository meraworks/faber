use faber_kernel_occt::OcctKernel;
use faber_model::Placement;
use faber_ports::{Combine, CurvedMesh, Kernel, Shape, SurfaceKind};

fn measure(shape: &Shape) -> CurvedMesh {
    let kernel = OcctKernel;
    let sample = kernel.tessellate_curvature(shape, 0.08).unwrap();
    let reference = kernel.tessellate_faces(shape, 0.08).unwrap();
    assert_eq!(sample.faces.mesh.vertices, reference.mesh.vertices);
    assert_eq!(sample.faces.mesh.normals, reference.mesh.normals);
    assert_eq!(sample.faces.mesh.triangles, reference.mesh.triangles);
    assert_eq!(sample.faces.face_of_triangle, reference.face_of_triangle);
    assert_eq!(sample.curvature.len(), reference.mesh.vertices.len());
    assert_eq!(sample.kinds.len(), kernel.faces(shape).unwrap().len());
    for &[low, high] in &sample.curvature {
        assert!(low.is_finite() && high.is_finite() && low <= high);
    }
    sample
}

fn near(values: [f64; 2], expected: [f64; 2]) {
    for (a, b) in values.into_iter().zip(expected) {
        assert!((a - b).abs() < 1e-9, "{values:?} differs from {expected:?}");
    }
}

#[test]
fn plane_has_zero_curvature() {
    let data = measure(&OcctKernel.box_shape([11., 12., 13.]).unwrap());
    assert!(data.kinds.values().all(|kind| *kind == SurfaceKind::Plane));
    assert!(data.curvature.iter().all(|k| *k == [0., 0.]));
}

#[test]
fn sphere_is_uniform_including_both_poles() {
    let data = measure(&OcctKernel.sphere_shape(10.).unwrap());
    assert!(data.kinds.values().all(|kind| *kind == SurfaceKind::Sphere));
    for z in [-10., 10.] {
        assert!(
            data.faces
                .mesh
                .vertices
                .iter()
                .any(|p| (p[2] - z).abs() < 1e-9)
        );
    }
    for k in data.curvature {
        near(k, [0.1, 0.1]);
    }
}

fn cylinder_samples(shape: &Shape, curvature: f64) {
    let data = measure(shape);
    let cyl = OcctKernel
        .faces(shape)
        .unwrap()
        .into_iter()
        .find(|f| {
            f.cylinder
                .as_ref()
                .is_some_and(|c| (c.radius - curvature.abs().recip()).abs() < 1e-9)
        })
        .unwrap()
        .index;
    assert_eq!(data.kinds[&cyl], SurfaceKind::Cylinder);
    let mut count = 0;
    for (&face, tri) in data
        .faces
        .face_of_triangle
        .iter()
        .zip(&data.faces.mesh.triangles)
    {
        let expected = if face == cyl {
            count += 1;
            [curvature.min(0.), curvature.max(0.)]
        } else if data.kinds[&face] == SurfaceKind::Plane {
            [0., 0.]
        } else {
            continue;
        };
        for &vertex in tri {
            near(data.curvature[vertex as usize], expected);
        }
    }
    assert!(count > 8);
}

#[test]
fn cylinder_has_one_direction() {
    cylinder_samples(&OcctKernel.cylinder_shape(5., 20.).unwrap(), 0.2);
}

#[test]
fn hole_is_concave() {
    let shell = OcctKernel.box_shape([16., 16., 12.]).unwrap();
    let bore = OcctKernel.cylinder_shape(2., 18.).unwrap();
    let bore = OcctKernel
        .transform_shape(
            &bore,
            &Placement {
                translation: [8., 8., -2.],
                rotation: [0.; 3],
            },
        )
        .unwrap();
    cylinder_samples(
        &OcctKernel
            .combine(&shell, &bore, Combine::Difference)
            .unwrap(),
        -0.5,
    );
}

#[test]
fn fillet_radius_is_found() {
    let block = OcctKernel.box_shape([20.; 3]).unwrap();
    cylinder_samples(&OcctKernel.fillet(&block, &[1], 2.).unwrap(), 0.5);
}

#[test]
fn torus_has_convex_and_saddle_regions_on_the_same_face() {
    let data = measure(&OcctKernel.torus_shape(8., 2.).unwrap());
    assert!(data.kinds.values().all(|kind| *kind == SurfaceKind::Torus));
    for (p, k) in data.faces.mesh.vertices.iter().zip(data.curvature) {
        let radius = p[0].hypot(p[1]);
        near(k, [(radius - 8.) / (2. * radius), 0.5]);
    }
}

#[test]
fn sharp_cone_tip_uses_a_finite_interior_sample() {
    let data = measure(&OcctKernel.cone_shape(5., 0., 10.).unwrap());
    let cosine = 10. / 5.0_f64.hypot(10.);
    let mut apex = false;
    for (&face, tri) in data
        .faces
        .face_of_triangle
        .iter()
        .zip(&data.faces.mesh.triangles)
    {
        if data.kinds[&face] != SurfaceKind::Cone {
            continue;
        }
        for &vertex in tri {
            let i = vertex as usize;
            let [x, y, _] = data.faces.mesh.vertices[i];
            let radius = x.hypot(y);
            if radius > 1e-6 {
                near(data.curvature[i], [0., cosine / radius]);
            } else {
                apex = true;
                assert!(data.curvature[i][1] > cosine / 5.);
            }
        }
    }
    assert!(apex);
}
