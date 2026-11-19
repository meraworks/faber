use faber_form::{Cage, subdivide};
use faber_model::Geometry;
use glam::DVec3;
use std::{collections::BTreeMap, path::Path};

fn key(p: [f64; 3]) -> [i64; 3] {
    p.map(|x| (x * 1e6).round() as i64)
}

#[test]
fn exact_form_handle_keeps_smooth_grip_normals_and_sharp_socket_edges() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/form-handle");
    let p = faber_wiring::read_snapshot(&root).unwrap();
    let geometry = faber_core::resolve_recipe(&p.data, &p.data.parts["handle"].geometry).unwrap();
    let Geometry::History { features, .. } = geometry else {
        panic!()
    };
    let Geometry::Form { cage, levels, .. } = &features[0].geometry else {
        panic!()
    };
    let source = subdivide(&Cage::from_data(cage).unwrap(), levels.unwrap()).unwrap();
    let originals: BTreeMap<_, _> = source
        .vertices
        .iter()
        .zip(&source.normals)
        .map(|(&p, &n)| (key(p), DVec3::from(n)))
        .collect();
    let body = &p.bodies[0];
    let mesh = &body.mesh;
    assert_eq!(body.face_of_triangle.len(), mesh.triangles.len());
    let mut grip = 0;
    let mut flat_top = 0;
    let mut edge = 0;
    for t in &mesh.triangles {
        let coords = t.map(|i| DVec3::from(mesh.vertices[i as usize]));
        let normals = t.map(|i| DVec3::from(mesh.normals[i as usize]));
        if coords.iter().all(|p| p.z > 3. && p.z < 50.) {
            for (point, n) in coords.into_iter().zip(normals) {
                if let Some(expected) = originals.get(&key(point.to_array())) {
                    assert!(
                        n.dot(*expected) > 0.99999,
                        "At {point:?} the corner normal is {n:?}, expected {expected:?}"
                    );
                    grip += 1;
                }
            }
        }
        if coords.iter().all(|p| (p.z - 71.).abs() < 1e-6) {
            assert!(normals.iter().all(|n| n.z > 0.99999));
            flat_top += 1;
        } else if coords.iter().any(|p| (p.z - 71.).abs() < 1e-6)
            && coords.iter().all(|p| p.z > 50.)
        {
            assert!(normals.iter().all(|n| n.z.abs() < 1e-5));
            edge += 1;
        }
    }
    assert!(
        grip > 100 && flat_top > 5 && edge > 5,
        "{grip} grip, {flat_top} top, {edge} side"
    );
}

#[test]
fn crease_normals_of_an_exact_cube_stay_perpendicular_to_each_side() {
    use faber_model::{Crease, Part, Scalar};
    use serde_json::json;
    let root = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(root.path(), true, Some("Cube".into())).unwrap();
    let mut d = faber_form::form_box([10.; 3], [1; 3]);
    d.creases = Cage::from_data(&d)
        .unwrap()
        .edges()
        .into_iter()
        .map(|edge| Crease {
            edge,
            sharpness: 10.,
        })
        .collect();
    let part:Part<Scalar>=serde_json::from_value(json!({"name":"Cube","colour":[100,100,100],"geometry":{"op":"form","as_solid":true,"levels":2,"cage":d}})).unwrap();
    p.put_part("cube".into(), part).unwrap();
    let b = &p.bodies[0];
    assert!((b.volume - 1000.).abs() < 1e-7);
    for &t in &b.mesh.triangles {
        let [a, bp, c] = t.map(|i| DVec3::from(b.mesh.vertices[i as usize]));
        let n = (bp - a).cross(c - a).normalize();
        for id in t {
            assert!(n.dot(b.mesh.normals[id as usize].into()) > 1. - 1e-8);
        }
    }
}
