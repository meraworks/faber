use faber_core::{Project, Reading};
use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn fixture() -> (tempfile::TempDir, Project) {
    let d = tempfile::tempdir().unwrap();
    let p = faber_wiring::open(d.path(), true, Some("Surface test".into())).unwrap();
    (d, p)
}
fn feature(id: &str, g: Value) -> Value {
    json!({"id":id,"geometry":g})
}
fn part(mut operations: Vec<Value>, output: &str) -> Part<Scalar> {
    operations.insert(0, feature("stock", json!({"op":"box","size":[4,4,4]})));
    serde_json::from_value(json!({"name":"Surfaces","colour":[30,80,120],
        "geometry":{"op":"history","features":operations,"output":output}}))
    .unwrap()
}
fn square() -> Vec<Value> {
    vec![
        feature(
            "base",
            json!({"op":"sketch","profile":{"outer":{"kind":"rectangle","origin":[0,0],"size":[20,20]}}}),
        ),
        feature(
            "skin",
            json!({"op":"surface_extrude","profile":"base","distance":10}),
        ),
    ]
}
fn check(mut ops: Vec<Value>, area: f64, open: u32) {
    let (_dir, mut p) = fixture();
    p.put_part("test".into(), part(std::mem::take(&mut ops), "stock"))
        .unwrap();
    let Reading::Surface {
        area_mm2,
        open_edges,
        ..
    } = p.feature_info("test", "skin").unwrap().reading
    else {
        panic!("Expected a surface")
    };
    assert!(
        (area_mm2 - area).abs() < 1e-5 * area,
        "{area_mm2} != {area}"
    );
    assert_eq!(open_edges, open);
}
#[test]
fn extruded_square_wire() {
    check(square(), 800., 8);
}
#[test]
fn extruded_profile_keeps_its_inner_wire() {
    let annulus = feature(
        "base",
        json!({"op":"sketch","profile":{
        "outer":{"kind":"circle","centre":[0,0],"radius":10},
        "holes":[{"kind":"circle","centre":[0,0],"radius":4}]}}),
    );
    check(
        vec![
            annulus,
            feature(
                "skin",
                json!({"op":"surface_extrude","profile":"base","distance":-5}),
            ),
        ],
        140. * std::f64::consts::PI,
        4,
    );
}
#[test]
fn display_reads_only_the_selected_occurrence_and_its_surface_inputs() {
    let (dir, mut p) = fixture();
    p.put_part("owner".into(), part(square(), "stock")).unwrap();
    let mut data = p.data.clone();
    let copy = serde_json::from_value(json!({"name":"Moved","colour":[100,2,3],"placement":{
        "translation":[100,50,0],"rotation":[0,0,90]
    },"geometry":{"op":"instance","of":"owner"}}))
    .unwrap();
    data.parts.insert("placed".into(), copy);
    let faber_model::Geometry::History { features, .. } =
        &mut data.parts.get_mut("owner").unwrap().geometry
    else {
        unreachable!()
    };
    features.insert(
        0,
        serde_json::from_value(feature(
            "unused",
            json!({"op":"imported","asset":"missing"}),
        ))
        .unwrap(),
    );
    let view = faber_wiring::surface_feature::display(dir.path(), &data, "placed", "skin").unwrap();
    assert_eq!(view.body.id, "placed");
    assert_eq!(view.body.colour, [100, 2, 3]);
    assert_eq!(view.boundaries.ranges.len(), 8);
    for (point, expected) in view
        .body
        .bounds
        .iter()
        .zip([[80., 50., 0.], [100., 70., 10.]])
    {
        for (value, expected) in point.iter().zip(expected) {
            assert!((value - expected).abs() < 1e-5, "{point:?}")
        }
    }
    assert_eq!(
        view.body.face_of_triangle.len(),
        view.body.mesh.triangles.len()
    );
    assert!((view.area_mm2 - 800.).abs() < 1e-6);
}
#[test]
fn revolved_line_is_a_cone_surface() {
    let line = feature(
        "line",
        json!({"op":"curve","path":{"start":[10,0,0],"segments":[{"kind":"line","end":[5,0,20]}]}}),
    );
    let axis = feature(
        "axis",
        json!({"op":"axis","origin":[0,0,0],"direction":[0,0,1]}),
    );
    check(
        vec![
            line,
            axis,
            feature(
                "skin",
                json!({"op":"surface_revolve","profile":"line","axis":"axis","angle_deg":360}),
            ),
        ],
        std::f64::consts::PI * 15. * 425f64.sqrt(),
        2,
    );
}
#[test]
fn ruled_loft_between_two_circles() {
    let circle = |name: &str, r: f64, z: f64| {
        feature(
            name,
            json!({"op":"sketch","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":r}},"placement":{"translation":[0,0,z],"rotation":[0,0,0]}}),
        )
    };
    check(
        vec![
            circle("a", 10., 0.),
            circle("b", 5., 20.),
            feature(
                "skin",
                json!({"op":"surface_loft","sections":["a","b"],"ruled":true}),
            ),
        ],
        std::f64::consts::PI * 15. * 425f64.sqrt(),
        2,
    );
}
#[test]
fn sweep_of_an_arc_along_a_line() {
    let arc = feature(
        "arc",
        json!({"op":"curve","path":{"start":[5,0,0],"segments":[{"kind":"arc","via":[0,5,0],"end":[-5,0,0]}]}}),
    );
    let spine = feature(
        "spine",
        json!({"op":"curve","path":{"start":[0,0,0],"segments":[{"kind":"line","end":[0,0,12]}]}}),
    );
    check(
        vec![
            arc,
            spine,
            feature(
                "skin",
                json!({"op":"surface_sweep","profile":"arc","path":"spine"}),
            ),
        ],
        60. * std::f64::consts::PI,
        4,
    );
}
#[test]
fn surface_output_is_refused() {
    let (_d, mut p) = fixture();
    let before = p.data.revision;
    let error = p
        .put_part("test".into(), part(square(), "skin"))
        .unwrap_err();
    assert_eq!(
        error,
        "The output of part test is a surface. Stitch or thicken it to make a solid."
    );
    assert_eq!(p.data.revision, before);
}
