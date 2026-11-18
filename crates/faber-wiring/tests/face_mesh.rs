use faber_core::{Project, face_triangles};
use faber_model::Scalar;
use serde_json::{Value, json};

fn project(geometry: Value) -> (tempfile::TempDir, Project) {
    let root = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(root.path(), true, Some("Face mesh".into())).unwrap();
    let part: faber_model::Part<Scalar> = serde_json::from_value(json!({
        "name":"Solid", "colour":[80,140,210],
        "geometry":{"op":"history","output":"base","features":[{"id":"base","geometry":geometry}]}
    }))
    .unwrap();
    project.put_part("solid".into(), part).unwrap();
    (root, project)
}

#[test]
fn box_has_two_triangles_for_each_face() {
    let (_root, project) = project(json!({"op":"box","size":[4,5,6]}));
    let body = &project.bodies[0];
    assert_eq!(body.face_of_triangle.len(), 12);
    let (_, faces) = project.topology("solid").unwrap();
    assert_eq!(faces.len(), 6);
    for face in faces {
        let triangles = face_triangles(body, face.index);
        assert_eq!(triangles.len(), 2);
        let axis = face
            .normal
            .unwrap()
            .iter()
            .position(|v| v.abs() > 0.99)
            .unwrap();
        assert!(
            triangles
                .into_iter()
                .flatten()
                .all(|p| (p[axis] - face.centre[axis]).abs() < 1e-10)
        );
    }
    assert!(face_triangles(body, u32::MAX).is_empty());
}

#[test]
fn cylinder_side_is_one_face() {
    let (_root, project) = project(json!({"op":"cylinder","radius":8,"height":12}));
    let body = &project.bodies[0];
    let side = project
        .topology("solid")
        .unwrap()
        .1
        .into_iter()
        .find(|f| f.cylinder.is_some())
        .unwrap()
        .index;
    assert_eq!(body.mesh.triangles.len(), body.face_of_triangle.len());
    assert!(face_triangles(body, side).len() > 8);
    for (triangle, face) in body.mesh.triangles.iter().zip(&body.face_of_triangle) {
        let z = triangle.map(|i| body.mesh.vertices[i as usize][2]);
        let spans_height =
            z.iter().any(|v| v.abs() < 1e-10) && z.iter().any(|v| (v - 12.).abs() < 1e-10);
        assert_eq!(spans_height, *face == side);
    }
}

#[test]
fn face_index_survives_a_move_and_a_reopen() {
    let (root, mut project) = project(json!({"op":"box","size":[4,5,6]}));
    let top = project
        .topology("solid")
        .unwrap()
        .1
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.99))
        .unwrap()
        .index;
    let before = project.bodies[0].face_of_triangle.clone();
    let mut part = project.data.parts["solid"].clone();
    part.placement.translation = [9., 8., 7.].map(Scalar::Number);
    project.put_part("solid".into(), part).unwrap();
    assert_eq!(project.bodies[0].face_of_triangle, before);
    drop(project);
    let project = faber_wiring::open(root.path(), false, None).unwrap();
    assert_eq!(project.bodies[0].face_of_triangle, before);
    let triangles = face_triangles(&project.bodies[0], top);
    assert_eq!(triangles.len(), 2);
    assert!(
        triangles
            .into_iter()
            .flatten()
            .all(|p| (p[2] - 13.).abs() < 1e-10)
    );
}

#[test]
fn mesh_only_part_has_no_face_index() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("surface.obj");
    std::fs::write(&source, "v 0 0 0\nv 8 0 0\nv 0 8 0\nf 1 2 3\n").unwrap();
    let mut project = faber_wiring::open(root.path(), true, None).unwrap();
    project
        .import_part("surface".into(), "Surface".into(), &source, false)
        .unwrap();
    let body = &project.bodies[0];
    assert_eq!(body.mesh.triangles.len(), 1);
    assert!(body.face_of_triangle.is_empty());
    assert!(face_triangles(body, 0).is_empty());
}
/// An adapter that supplies a solid and a mesh, without face indices.
struct Plain;
impl faber_ports::Kernel for Plain {
    fn build(&self, geometry: &faber_model::Geometry) -> faber_ports::Result<faber_ports::Mesh> {
        faber_kernel_occt::OcctKernel.build(geometry)
    }
    fn exact_shape(
        &self,
        geometry: &faber_model::Geometry,
    ) -> faber_ports::Result<Option<faber_ports::Shape>> {
        faber_kernel_occt::OcctKernel.exact_shape(geometry)
    }
    fn properties(
        &self,
        shape: &faber_ports::Shape,
    ) -> faber_ports::Result<faber_ports::Properties> {
        faber_kernel_occt::OcctKernel.properties(shape)
    }
    fn tessellate(
        &self,
        shape: &faber_ports::Shape,
        tolerance: f64,
    ) -> faber_ports::Result<faber_ports::Mesh> {
        faber_kernel_occt::OcctKernel.tessellate(shape, tolerance)
    }
    fn transform_shape(
        &self,
        shape: &faber_ports::Shape,
        placement: &faber_model::Placement,
    ) -> faber_ports::Result<faber_ports::Shape> {
        faber_kernel_occt::OcctKernel.transform_shape(shape, placement)
    }
}

#[test]
fn an_adapter_without_face_indices_still_builds_a_body() {
    let data = serde_json::from_value(json!({
        "schema":1,"revision":0,"name":"Plain adapter",
        "parts":{"block":{"name":"Block","colour":[0,0,0],"geometry":{"op":"box","size":[4,5,6]}}}
    }))
    .unwrap();
    let bodies = faber_core::build(&data, &Plain).unwrap();
    assert_eq!(bodies.len(), 1);
    assert_eq!(bodies[0].mesh.triangles.len(), 12);
    assert!(bodies[0].face_of_triangle.is_empty());
}

#[test]
fn foreign_face_index_in_the_cache_rebuilds_the_mesh() {
    use faber_ports::FeatureCache;
    let (root, project) = project(json!({"op":"box","size":[4,5,6]}));
    let original = project.bodies[0].face_of_triangle.clone();
    drop(project);
    let cache = faber_files::FileFeatureCache::new(root.path());
    let key = std::fs::read_dir(root.path().join(".faber/cache"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find_map(|p| {
            let name = p.file_stem()?.to_str()?;
            name.starts_with("mesh").then(|| name.to_owned())
        })
        .expect("The history output has no cached mesh.");
    let mut bytes = cache.get(&key).unwrap();
    let last = bytes.len() - 4;
    bytes[last..].copy_from_slice(&7u32.to_le_bytes());
    cache.put(&key, &bytes).unwrap();
    let reopened = faber_wiring::open(root.path(), false, None).unwrap();
    assert_eq!(reopened.bodies[0].face_of_triangle, original);
    assert_ne!(cache.get(&key).unwrap(), bytes);
}
