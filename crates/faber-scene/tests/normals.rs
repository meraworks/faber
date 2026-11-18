use faber_ports::Mesh;
use glam::DVec3;

#[test]
fn cylinder_seam_is_smooth_and_caps_stay_flat() {
    let mut mesh = Mesh::default();
    let count = 48;
    for i in 0..count {
        let a = i as f64 * std::f64::consts::TAU / count as f64;
        let b = (i + 1) as f64 * std::f64::consts::TAU / count as f64;
        let start = mesh.vertices.len() as u32;
        mesh.vertices.extend([
            [a.cos(), a.sin(), 0.],
            [b.cos(), b.sin(), 0.],
            [b.cos(), b.sin(), 2.],
            [a.cos(), a.sin(), 2.],
        ]);
        mesh.triangles
            .extend([[start, start + 1, start + 2], [start, start + 2, start + 3]]);
    }
    let top = mesh.vertices.len() as u32;
    mesh.vertices.extend([
        [0., 0., 2.],
        [1., 0., 2.],
        [
            (std::f64::consts::TAU / 48.).cos(),
            (std::f64::consts::TAU / 48.).sin(),
            2.,
        ],
    ]);
    mesh.triangles.push([top, top + 1, top + 2]);
    let normals = faber_scene::corner_normals(&mesh);
    for (triangle, normals) in mesh.triangles[..count * 2].iter().zip(&normals) {
        for (&index, normal) in triangle.iter().zip(normals) {
            let p = mesh.vertices[index as usize];
            let expected = DVec3::new(p[0], p[1], 0.);
            assert!(DVec3::from(normal.map(f64::from)).distance(expected) < 1e-5);
        }
    }
    for n in normals.last().unwrap() {
        assert_eq!(*n, [0., 0., 1.]);
    }
}

#[test]
fn perpendicular_faces_keep_their_edge() {
    let mesh = Mesh {
        normals: Vec::new(),
        vertices: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        triangles: vec![[0, 1, 2], [0, 3, 1]],
    };
    let normals = faber_scene::corner_normals(&mesh);
    assert_eq!(normals[0], [[0., 0., 1.]; 3]);
    assert_eq!(normals[1], [[0., 1., 0.]; 3]);
}

#[test]
fn surface_normals_preserve_shallow_face_boundaries() {
    let n = [0., 0.2f64.sin(), 0.2f64.cos()];
    let mesh = Mesh {
        vertices: vec![
            [0., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [0., 0., 0.],
            [1., 0., 0.],
            [0., -0.2f64.cos(), 0.2f64.sin()],
        ],
        triangles: vec![[0, 1, 2], [3, 5, 4]],
        normals: vec![[0., 0., 1.], [0., 0., 1.], [0., 0., 1.], n, n, n],
    };
    let normals = faber_scene::corner_normals(&mesh);
    assert_eq!(normals[0], [[0., 0., 1.]; 3]);
    for actual in normals[1] {
        for axis in 0..3 {
            assert!((actual[axis] as f64 - n[axis]).abs() < 1e-7);
        }
    }
    let cut = faber_scene::section(
        &mesh,
        faber_scene::Section {
            normal: [0., 0., 1.],
            offset: 10.,
            reverse: false,
        },
    )
    .unwrap();
    assert_eq!(cut.surface.normals, mesh.normals);
}

#[test]
fn intersecting_section_interpolates_surface_normals() {
    let mesh = Mesh {
        vertices: vec![[0., 0., 0.], [2., 0., 0.], [0., 2., 0.], [0., 0., 2.]],
        triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        normals: vec![[0., 0., 1.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    };
    let cut = faber_scene::section(
        &mesh,
        faber_scene::Section {
            normal: [1., 0., 0.],
            offset: 1.,
            reverse: false,
        },
    )
    .unwrap();
    assert!(!cut.cap.triangles.is_empty());
    assert_eq!(cut.surface.normals.len(), cut.surface.vertices.len());
    let mut checked = 0;
    for (p, n) in cut.surface.vertices.iter().zip(&cut.surface.normals) {
        if p == &[1., 0., 0.] {
            let expected = DVec3::new(1., 0., 1.).normalize();
            assert!(DVec3::from(*n).distance(expected) < 1e-10);
            checked += 1;
        }
    }
    assert!(checked > 0);
}
