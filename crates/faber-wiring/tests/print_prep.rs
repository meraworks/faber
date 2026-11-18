use faber_wiring::print::{apply_orientation, orientations};

#[test]
fn an_unused_mesh_vertex_cannot_hold_the_part_above_the_bed() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("part.obj");
    std::fs::write(
        &file,
        "v 0 0 0\nv 8 0 0\nv 0 8 0\nv 0 0 4\nv 0 0 -500\nf 1 3 2\nf 1 2 4\nf 1 4 3\nf 2 3 4\n",
    )
    .unwrap();
    let mut project = faber_wiring::create(dir.path(), "Mesh print".into()).unwrap();
    project
        .import_part("part".into(), "Part".into(), &file, false)
        .unwrap();
    let ranked = orientations(&project, "part", None).unwrap();
    let base = ranked.iter().find(|c| c.down == [0., 0., -1.]).unwrap();
    apply_orientation(&mut project, "part", base).unwrap();
    let mesh = &project.bodies[0].mesh;
    let low = mesh
        .triangles
        .iter()
        .flatten()
        .map(|&i| mesh.vertices[i as usize][2])
        .fold(f64::INFINITY, f64::min);
    assert!(
        low.abs() < 1e-6,
        "The printed faces are {low} mm above the bed."
    );
    assert!((project.bodies[0].bounds[0][2] - low).abs() < 1e-6);
}
