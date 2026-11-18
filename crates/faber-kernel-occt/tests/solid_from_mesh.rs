use faber_kernel_occt::OcctKernel;
use faber_ports::{Kernel, Mesh};

fn cube() -> Mesh {
    Mesh {
        vertices: vec![
            [0., 0., 0.],
            [10., 0., 0.],
            [10., 10., 0.],
            [0., 10., 0.],
            [0., 0., 10.],
            [10., 0., 10.],
            [10., 10., 10.],
            [0., 10., 10.],
        ],
        triangles: vec![
            [0, 2, 1],
            [0, 3, 2],
            [4, 5, 6],
            [4, 6, 7],
            [0, 1, 5],
            [0, 5, 4],
            [1, 2, 6],
            [1, 6, 5],
            [2, 3, 7],
            [2, 7, 6],
            [3, 0, 4],
            [3, 4, 7],
        ],
        normals: vec![],
    }
}

#[test]
fn cube_mesh_becomes_six_faces() {
    let shape = OcctKernel.solid_from_mesh(&cube(), true).unwrap();
    let props = OcctKernel.properties(&shape).unwrap();
    assert!(props.valid && props.solids == 1 && props.faces == 6);
    assert!((props.volume - 1000.).abs() < 1e-6);
}

#[test]
fn sphere_mesh_keeps_its_triangles() {
    let mut mesh = Mesh::default();
    let (around, bands) = (50u32, 20u32);
    mesh.vertices.push([0., 0., 10.]);
    for row in 1..=bands {
        let theta = std::f64::consts::PI * f64::from(row) / f64::from(bands + 1);
        for col in 0..around {
            let phi = std::f64::consts::TAU * f64::from(col) / f64::from(around);
            mesh.vertices.push([
                10. * theta.sin() * phi.cos(),
                10. * theta.sin() * phi.sin(),
                10. * theta.cos(),
            ]);
        }
    }
    let south = mesh.vertices.len() as u32;
    mesh.vertices.push([0., 0., -10.]);
    for i in 0..around {
        let next = (i + 1) % around;
        mesh.triangles.push([0, 1 + i, 1 + next]);
        for row in 0..bands - 1 {
            let a = 1 + row * around + i;
            let b = 1 + row * around + next;
            mesh.triangles
                .extend([[a, a + around, b + around], [a, b + around, b]]);
        }
        let last = 1 + (bands - 1) * around;
        mesh.triangles.push([last + next, last + i, south]);
    }
    assert_eq!(mesh.triangles.len(), 2000);
    let volume = mesh
        .triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| glam::DVec3::from(mesh.vertices[i as usize]));
            a.dot(b.cross(c)) / 6.
        })
        .sum::<f64>();
    let props = OcctKernel
        .properties(&OcctKernel.solid_from_mesh(&mesh, false).unwrap())
        .unwrap();
    assert!(props.valid && props.faces == 2000 && (props.volume - volume).abs() < 1e-6);
}

#[test]
fn open_mesh_is_refused() {
    let mut mesh = cube();
    mesh.triangles.pop();
    assert!(OcctKernel.solid_from_mesh(&mesh, true).is_err());
}

#[test]
fn large_mesh_is_refused_for_a_solid() {
    let mesh = Mesh {
        triangles: vec![[0, 1, 2]; 250000],
        ..Default::default()
    };
    assert_eq!(
        OcctKernel.solid_from_mesh(&mesh, true).err().unwrap(),
        "The mesh has 250000 triangles. The limit for a solid is 200000. Reduce the mesh first."
    );
}

#[test]
fn nested_inner_shell_preserves_the_cavity() {
    let mut m = cube();
    let mut cavity = cube();
    m.vertices
        .extend(cavity.vertices.iter().map(|v| v.map(|c| c * 0.4 + 3.)));
    for t in &mut cavity.triangles {
        t.swap(1, 2);
        m.triangles.push(t.map(|i| i + 8));
    }
    let shape = OcctKernel.solid_from_mesh(&m, true).unwrap();
    let p = OcctKernel.properties(&shape).unwrap();
    assert!(p.valid && p.solids == 1 && p.faces == 12 && (p.volume - 936.).abs() < 1e-6);
}

#[test]
fn disjoint_closed_shells_remain_two_solids() {
    let mut mesh = cube();
    let other = cube();
    mesh.vertices
        .extend(other.vertices.into_iter().map(|[x, y, z]| [x + 20., y, z]));
    mesh.triangles
        .extend(other.triangles.into_iter().map(|t| t.map(|i| i + 8)));
    let shape = OcctKernel.solid_from_mesh(&mesh, true).unwrap();
    let props = OcctKernel.properties(&shape).unwrap();
    assert!(
        props.valid
            && props.solids == 2
            && props.faces == 12
            && (props.volume - 2000.).abs() < 1e-6
    );
}

#[test]
fn nested_outward_shells_cannot_double_count_material() {
    let mut mesh = cube();
    let inner = cube();
    mesh.vertices
        .extend(inner.vertices.iter().map(|v| v.map(|c| c * 0.4 + 3.)));
    mesh.triangles
        .extend(inner.triangles.into_iter().map(|t| t.map(|i| i + 8)));
    assert!(OcctKernel.solid_from_mesh(&mesh, true).is_err());
}
