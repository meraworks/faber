use super::*;
use faber_ports::Mesh;

#[test]
fn interior_peak_on_wide_planar_face() {
    let body = Body {
        id: "plate".into(),
        name: "Plate".into(),
        group: String::new(),
        colour: [0; 3],
        volume: 64.,
        bounds: [[0.; 3], [8., 8., 1.]],
        mesh: Mesh {
            vertices: vec![[0., 0., 1.], [8., 0., 1.], [8., 8., 1.], [0., 8., 1.]],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            normals: vec![[0., 0., 1.]; 4],
        },
        face_of_triangle: vec![3, 7],
        edge_lines: Default::default(),
        shape: None,
    };
    let grid = Grid {
        origin: [0.; 3],
        h: 1.,
        n: [8, 8, 1],
        fill: vec![1.; 64],
    };
    let mut result = StaticResult {
        displacement: vec![[0.; 3]; 9 * 9 * 2],
        von_mises: vec![1.; 64],
        stress_mpa: Vec::new(),
        iterations: 0,
        residual: 0.,
        reaction: [0.; 3],
    };
    for z in 0..2 {
        for y in 0..9 {
            for x in 0..9 {
                result.displacement[grid.node(x, y, z)] = [(x * y) as f64 / 64., 0., 0.];
            }
        }
    }
    for x in 3..5 {
        for y in 3..5 {
            result.von_mises[8 * y + x] = 100.;
        }
    }
    let field = field(&body, &grid, &result).unwrap();
    assert_eq!(field.vertex_von_mises, vec![1.; 4]);
    let surface = field.surface.unwrap();
    assert!(surface.vertex_von_mises.contains(&100.));
    assert_eq!(&surface.mesh.vertices[..4], &body.mesh.vertices);
    assert_eq!(body.mesh.triangles.len(), 2);
    assert_eq!(surface.face_of_triangle.len(), surface.mesh.triangles.len());
    assert_eq!(surface.mesh.normals.len(), surface.mesh.vertices.len());
    for (triangle, &id) in surface
        .mesh
        .triangles
        .iter()
        .zip(surface.face_of_triangle.iter())
    {
        let [a, b, c] = triangle.map(|ix| DVec3::from(surface.mesh.vertices[ix as usize]));
        assert!((b - a).cross(c - a).z > 0.);
        let centre = (a + b + c) / 3.;
        assert_eq!(id, if centre.x > centre.y { 3 } else { 7 });
    }
    for (p, displacement) in surface
        .mesh
        .vertices
        .iter()
        .zip(&surface.vertex_displacement)
    {
        assert!((f64::from(displacement[0]) - p[0] * p[1] / 64.).abs() < 1e-6);
    }
}

#[test]
fn triangle_budget_keeps_shared_edges_closed() {
    let mesh = Mesh {
        vertices: vec![[0., 0., 0.], [10., 0., 0.], [10., 10., 0.], [0., 10., 0.]],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        normals: Vec::new(),
    };
    let (detail, _) = surface::refine(&mesh, &[], 0.01, 4).unwrap();
    assert_eq!(detail.triangles.len(), 4);
    assert_eq!(detail.vertices.len(), 5);
    assert_eq!(detail.vertices[4], [5., 5., 0.]);
    for tri in &detail.triangles {
        assert!(tri.contains(&4));
    }
}

#[test]
fn modal_display_budget_accounts_for_fields_and_cached_reads() {
    assert!(
        check_modal_display(2_000_000, 0, 0, 32)
            .unwrap_err()
            .contains("512 MiB")
    );
    assert!(check_modal_display(200_000, 0, 0, 32).is_ok());
    assert!(check_modal_display(200_000, 400_000, 120 * 1024 * 1024, 32).is_err());
}
