use faber_model::{Part, Scalar};
use serde_json::{Value, json};
use std::f64::consts::PI;

fn part(features: Value, output: &str) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Reference","colour":[100,140,170],"geometry":{"op":"history","output":output,"features":features}})).unwrap()
}

fn check_bounds(actual: [[f64; 3]; 2], expected: [[f64; 3]; 2]) {
    for (a, e) in actual.iter().flatten().zip(expected.iter().flatten()) {
        assert!(
            (a - e).abs() < 1e-5,
            "Bounds: {actual:?}, expected {expected:?}"
        );
    }
}

fn check_volume(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-5,
        "Volume: {actual}, expected {expected}"
    );
}

/// A unit disk on the plane, extruded along the vector.
fn disk_on(plane: &str, vector: [f64; 3]) -> Vec<Value> {
    vec![
        json!({"id":"disk","geometry":{"op":"sketch_on_plane","plane":plane,"profile":{"outer":{"kind":"circle","centre":[0,0],"radius":1}}}}),
        json!({"id":"solid","geometry":{"op":"extrude_sketch","source":"disk","vector":vector}}),
    ]
}

fn with_disk(mut head: Vec<Value>, tail: Vec<Value>) -> Value {
    head.extend(tail);
    Value::Array(head)
}

fn file(directory: &std::path::Path) -> (std::time::SystemTime, u64) {
    faber_wiring::project_fingerprint(directory).unwrap()
}

#[test]
fn plane_at_angle_from_xy_about_x_has_the_known_normal() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Angle plane".into())).unwrap();
    // Normal (0, -sin 30, cos 30). The disk spans y in [-cos 30, cos 30] and z in [-0.5, 0.5].
    let c = 3f64.sqrt() / 2.;
    let features = with_disk(
        vec![
            json!({"id":"axis","geometry":{"op":"axis","origin":[0,0,0],"direction":[1,0,0]}}),
            json!({"id":"tilt","geometry":{"op":"plane_at_angle","axis":"axis","from":"xy","angle":30}}),
        ],
        disk_on("tilt", [0., -1., 3f64.sqrt()]),
    );
    project
        .put_part("pad".into(), part(features, "solid"))
        .unwrap();
    check_volume(project.bodies[0].volume, 2. * PI);
    check_bounds(
        project.bodies[0].bounds,
        [[-1., -c - 1., -0.5], [1., c, 0.5 + 3f64.sqrt()]],
    );
}

#[test]
fn plane_at_angle_follows_its_parameter_and_refuses_an_axis_outside_the_plane() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Angle parameter".into())).unwrap();
    project
        .set_params([("tilt".into(), "30".into())].into())
        .unwrap();
    let features = with_disk(
        vec![
            json!({"id":"axis","geometry":{"op":"axis","origin":[0,0,0],"direction":[1,0,0]}}),
            json!({"id":"tilt_plane","geometry":{"op":"plane_at_angle","axis":"axis","from":"xy","angle":"tilt"}}),
        ],
        // A fixed vector along Z gives a prism of area times the cosine of the angle.
        disk_on("tilt_plane", [0., 0., 2.]),
    );
    project
        .put_part("pad".into(), part(features.clone(), "solid"))
        .unwrap();
    check_volume(project.bodies[0].volume, 3f64.sqrt() * PI);
    project
        .set_params([("tilt".into(), "60".into())].into())
        .unwrap();
    check_volume(project.bodies[0].volume, PI);
    let before = file(directory.path());
    for axis in [
        json!({"id":"axis","geometry":{"op":"axis","origin":[0,0,0],"direction":[0,0,1]}}),
        json!({"id":"axis","geometry":{"op":"axis","origin":[0,0,5],"direction":[1,0,0]}}),
    ] {
        let mut invalid = features.clone();
        invalid[0] = axis;
        let error = project
            .put_part("pad".into(), part(invalid, "solid"))
            .unwrap_err();
        assert!(error.contains("must lie in the reference plane"), "{error}");
        assert_eq!(file(directory.path()), before);
    }
}

#[test]
fn midplane_of_box_top_and_bottom_sits_at_mid_height_and_follows_height() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Midplane".into())).unwrap();
    project
        .set_params([("h".into(), "20".into())].into())
        .unwrap();
    let face = |normal: [f64; 3]| json!({"normal":normal,"expect":1,"tolerance":0.000001});
    let features = with_disk(
        vec![
            json!({"id":"base","geometry":{"op":"box","size":[10,10,"h"]}}),
            json!({"id":"top","geometry":{"op":"plane_on_face","source":"base","face":face([0.,0.,1.]),"offset":0}}),
            json!({"id":"bottom","geometry":{"op":"plane_on_face","source":"base","face":face([0.,0.,-1.]),"offset":0}}),
            json!({"id":"side","geometry":{"op":"plane_on_face","source":"base","face":face([1.,0.,0.]),"offset":0}}),
            json!({"id":"mid","geometry":{"op":"midplane","first":"top","second":"bottom"}}),
        ],
        disk_on("mid", [0., 0., 2.]),
    );
    project
        .put_part("pad".into(), part(features.clone(), "solid"))
        .unwrap();
    check_volume(project.bodies[0].volume, 2. * PI);
    check_bounds(project.bodies[0].bounds, [[4., 4., 10.], [6., 6., 12.]]);
    project
        .set_params([("h".into(), "40".into())].into())
        .unwrap();
    check_bounds(project.bodies[0].bounds, [[4., 4., 20.], [6., 6., 22.]]);

    let before = file(directory.path());
    let mut side = features;
    side[4] = json!({"id":"mid","geometry":{"op":"midplane","first":"top","second":"side"}});
    let error = project
        .put_part("pad".into(), part(side, "solid"))
        .unwrap_err();
    assert!(error.contains("must be parallel"), "{error}");
    assert_eq!(file(directory.path()), before);
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    check_bounds(bodies[0].bounds, [[4., 4., 20.], [6., 6., 22.]]);
}

#[test]
fn plane_through_points_uses_point_features_and_refuses_collinear_points() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Three points".into())).unwrap();
    // The normal of the three points is (0, -1, 2) / sqrt(5).
    let (a, b) = (1. / 5f64.sqrt(), 2. / 5f64.sqrt());
    let features = with_disk(
        vec![
            json!({"id":"p1","geometry":{"op":"point","position":[0,0,0]}}),
            json!({"id":"p2","geometry":{"op":"point","position":[10,0,0]}}),
            json!({"id":"p3","geometry":{"op":"point","position":[0,10,5]}}),
            json!({"id":"tri","geometry":{"op":"plane_through_points","points":["p1","p2","p3"]}}),
        ],
        disk_on("tri", [0., -2. * a, 2. * b]),
    );
    project
        .put_part("pad".into(), part(features.clone(), "solid"))
        .unwrap();
    check_volume(project.bodies[0].volume, 2. * PI);
    check_bounds(
        project.bodies[0].bounds,
        [[-1., -b - 2. * a, -a], [1., b, a + 2. * b]],
    );
    let before = file(directory.path());
    let mut collinear = features.clone();
    collinear[3] = json!({"id":"tri","geometry":{"op":"plane_through_points","points":[[0,0,0],[5,0,0],[10,0,0]]}});
    let error = project
        .put_part("pad".into(), part(collinear, "solid"))
        .unwrap_err();
    assert!(error.contains("not on one line"), "{error}");
    let mut wrong = features;
    wrong[3] =
        json!({"id":"tri","geometry":{"op":"plane_through_points","points":["p1","axis","p3"]}});
    wrong.as_array_mut().unwrap().insert(
        3,
        json!({"id":"axis","geometry":{"op":"axis","origin":[0,0,0],"direction":[1,0,0]}}),
    );
    let error = project
        .put_part("pad".into(), part(wrong, "solid"))
        .unwrap_err();
    assert!(error.contains("is not a reference point"), "{error}");
    assert_eq!(file(directory.path()), before);
}

#[test]
fn axis_from_cylinder_drives_a_revolve_on_the_cylinder_axis() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Cylinder axis".into())).unwrap();
    let features = json!([
        {"id":"cyl","geometry":{"op":"cylinder","radius":5,"height":20}},
        {"id":"axis","geometry":{"op":"axis_from_cylinder","source":"cyl","face":{"cylinder_radius":5,"expect":1,"tolerance":0.000001}}},
        {"id":"profile","geometry":{"op":"sketch","placement":{"rotation":[90,0,0]},"profile":{"outer":{"kind":"rectangle","origin":[2,0],"size":[2,5]}}}},
        {"id":"ring","geometry":{"op":"revolve_around","source":"profile","axis":"axis","angle":360}}
    ]);
    project
        .put_part("ring".into(), part(features.clone(), "ring"))
        .unwrap();
    // The rectangle lies at radii 2 to 4 from the Z axis and spans 5 mm in height.
    check_volume(project.bodies[0].volume, 60. * PI);
    check_bounds(project.bodies[0].bounds, [[-4., -4., 0.], [4., 4., 5.]]);
    let before = file(directory.path());
    let mut planar = features;
    planar[1]["geometry"]["face"] = json!({"normal":[0,0,1],"expect":1,"tolerance":0.000001});
    let error = project
        .put_part("ring".into(), part(planar, "ring"))
        .unwrap_err();
    assert!(error.contains("must be a cylinder"), "{error}");
    assert_eq!(file(directory.path()), before);
}

#[test]
fn axis_from_planes_drives_a_revolve_and_refuses_parallel_planes() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Plane axis".into())).unwrap();
    let features = json!([
        {"id":"floor","geometry":{"op":"plane","placement":{}}},
        {"id":"wall","geometry":{"op":"plane","placement":{"translation":[5,0,0],"rotation":[0,90,0]}}},
        {"id":"axis","geometry":{"op":"axis_from_planes","first":"floor","second":"wall"}},
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":{"kind":"rectangle","origin":[7,0],"size":[1,2]}}}},
        {"id":"ring","geometry":{"op":"revolve_around","source":"profile","axis":"axis","angle":360}}
    ]);
    project
        .put_part("ring".into(), part(features.clone(), "ring"))
        .unwrap();
    // The axis runs along Y through x = 5 and z = 0. The rectangle spans radii 2 to 3.
    check_volume(project.bodies[0].volume, 10. * PI);
    check_bounds(project.bodies[0].bounds, [[2., 0., -3.], [8., 2., 3.]]);
    let before = file(directory.path());
    let mut parallel = features;
    parallel.as_array_mut().unwrap().insert(
        2,
        json!({"id":"ceiling","geometry":{"op":"plane","placement":{"translation":[0,0,3]}}}),
    );
    parallel[3]["geometry"]["second"] = json!("ceiling");
    let error = project
        .put_part("ring".into(), part(parallel, "ring"))
        .unwrap_err();
    assert!(error.contains("must not be parallel"), "{error}");
    assert_eq!(file(directory.path()), before);
}

#[test]
fn point_at_centre_places_a_solid_on_a_full_circle() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Circle centre".into())).unwrap();
    let features = json!([
        {"id":"cyl","geometry":{"op":"cylinder","radius":5,"height":20}},
        {"id":"rim","geometry":{"op":"point_at_centre","source":"cyl","edge":{"radius":5,"near_centre":[0,0,20],"tolerance":0.5,"expect":1}}},
        {"id":"block","geometry":{"op":"cylinder","radius":1,"height":1}},
        {"id":"placed","geometry":{"op":"place_at_point","source":"block","point":"rim"}}
    ]);
    project
        .put_part("placed".into(), part(features.clone(), "placed"))
        .unwrap();
    // The top circle has its centre at z = 20.
    check_volume(project.bodies[0].volume, PI);
    check_bounds(project.bodies[0].bounds, [[-1., -1., 20.], [1., 1., 21.]]);
    let before = file(directory.path());
    let mut ambiguous = features.clone();
    ambiguous[1]["geometry"]["edge"] = json!({"radius":5,"tolerance":0.5,"expect":2});
    let error = project
        .put_part("placed".into(), part(ambiguous, "placed"))
        .unwrap_err();
    assert!(
        error.contains("Select exactly one circular edge"),
        "{error}"
    );
    let mut line = features;
    line[0] = json!({"id":"cyl","geometry":{"op":"box","size":[10,10,20]}});
    line[1]["geometry"]["edge"] = json!({"near_centre":[0,0,10],"tolerance":0.5,"expect":1});
    let error = project
        .put_part("placed".into(), part(line, "placed"))
        .unwrap_err();
    assert!(error.contains("must be a circle"), "{error}");
    assert_eq!(file(directory.path()), before);
}

#[test]
fn construction_references_refuse_missing_forward_and_wrong_type_inputs() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Refusals".into())).unwrap();
    let base = json!([
        {"id":"solid","geometry":{"op":"box","size":[10,10,10]}},
        {"id":"axis","geometry":{"op":"axis","origin":[0,0,0],"direction":[1,0,0]}},
        {"id":"plane","geometry":{"op":"plane","placement":{}}}
    ]);
    project
        .put_part("pad".into(), part(base.clone(), "solid"))
        .unwrap();
    let before = file(directory.path());
    let cases = [
        (
            json!({"id":"test","geometry":{"op":"plane_at_angle","axis":"missing","from":"xy","angle":10}}),
            "Unknown previous feature",
        ),
        (
            json!({"id":"test","geometry":{"op":"plane_at_angle","axis":"plane","from":"xy","angle":10}}),
            "is not a reference axis",
        ),
        (
            json!({"id":"test","geometry":{"op":"plane_at_angle","axis":"axis","from":{"plane":"axis"},"angle":10}}),
            "is not a reference plane",
        ),
        (
            json!({"id":"test","geometry":{"op":"midplane","first":"plane","second":"axis"}}),
            "is not a reference plane",
        ),
        (
            json!({"id":"test","geometry":{"op":"midplane","first":"plane","second":"later"}}),
            "Unknown previous feature",
        ),
        (
            json!({"id":"test","geometry":{"op":"axis_from_planes","first":"plane","second":"plane"}}),
            "must not be parallel",
        ),
    ];
    for (feature, text) in cases {
        let mut features = base.clone();
        features.as_array_mut().unwrap().push(feature);
        let error = project
            .put_part("pad".into(), part(features, "solid"))
            .unwrap_err();
        assert!(error.contains(text), "{error}");
        assert_eq!(file(directory.path()), before);
    }
}
