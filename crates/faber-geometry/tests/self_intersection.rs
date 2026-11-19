use faber_geometry::bvh::Bvh;
use faber_ports::Mesh;

fn crossing(points: Vec<[f64; 3]>, faces: Vec<[u32; 3]>) -> bool {
    let mesh = Mesh {
        vertices: points,
        triangles: faces,
        normals: vec![],
    };
    Bvh::new(&mesh).unwrap().intersects_itself()
}
#[test]
fn flat_neighbours_share_only_an_edge() {
    let vertices = vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [1., 1., 0.]];
    assert!(!crossing(vertices.clone(), vec![[0, 1, 2], [1, 3, 2]]));
    assert!(crossing(vertices, vec![[0, 1, 2], [0, 1, 3]]));
}
#[test]
fn thin_neighbours_do_not_overlap_at_their_common_edge() {
    let p = vec![
        [-100., 0., 0.],
        [100., 0., 0.],
        [0., 0.001, 0.],
        [0., -0.001, 0.],
    ];
    assert!(!crossing(p, vec![[0, 1, 2], [1, 0, 3]]));
}
#[test]
fn crossing_faces_and_contact_at_a_single_corner() {
    let points = vec![
        [0., 0., 0.],
        [4., 0., 0.],
        [0., 4., 0.],
        [1., 1., -2.],
        [1., 1., 2.],
        [-1., -1., -1.],
    ];
    assert!(crossing(points.clone(), vec![[0, 1, 2], [0, 3, 4]]));
    assert!(!crossing(points.clone(), vec![[0, 1, 2], [0, 3, 5]]));
    assert!(crossing(points, vec![[0, 1, 2], [3, 4, 5]]));
}
