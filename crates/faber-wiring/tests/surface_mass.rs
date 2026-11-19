use faber_ports::{Body, Mesh};
use faber_wiring::surface::centre_of_mass;

fn tetrahedron() -> Body {
    Body {
        id: "sample".into(),
        name: "Sample".into(),
        group: String::new(),
        colour: [80; 3],
        mesh: Mesh {
            vertices: vec![
                [100., 10., 5.],
                [104., 10., 5.],
                [100., 18., 5.],
                [100., 10., 17.],
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            normals: vec![],
        },
        face_of_triangle: vec![],
        edge_lines: Default::default(),
        volume: 64.,
        bounds: [[100., 10., 5.], [104., 18., 17.]],
        shape: None,
    }
}

#[test]
fn marker_uses_the_mass_centre_of_a_placed_mesh_and_rejects_an_open_shell() {
    let mut part = tetrahedron();
    assert_eq!(centre_of_mass(&part).unwrap(), [101., 12., 8.]);
    part.mesh.triangles.pop();
    assert!(centre_of_mass(&part).unwrap_err().contains("closed"));
}

#[test]
fn a_new_pull_direction_releases_the_previous_colour_map() {
    use faber_wiring::surface::{Request, cached};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Pull test".into())).unwrap();
    p.put_part::<f64>(
        "block".into(),
        serde_json::from_value(serde_json::json!({
            "name":"Block","colour":[60,120,150],"geometry":{"op":"box","size":[1,2,3]}
        }))
        .unwrap(),
    )
    .unwrap();
    let direction = |pull| Request::Draft { pull, min_deg: 1. };
    let first = cached(&p, "block", &direction([0., 0., 1.]), None).unwrap();
    assert!(Arc::ptr_eq(
        &first,
        &cached(&p, "block", &direction([0., 0., 1.]), None).unwrap()
    ));
    let reference = Arc::downgrade(&first);
    drop(first);
    let second = cached(&p, "block", &direction([1., 0., 0.]), None).unwrap();
    assert!(reference.upgrade().is_none());
    assert_eq!(second.report["vertical_mm2"], 10.);
}
