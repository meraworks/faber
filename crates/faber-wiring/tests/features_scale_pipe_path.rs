use faber_model::{Part, Scalar};
use serde_json::{Value, json};

fn recipe(features: Value, output: &str) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Part","colour":[100,140,170],"geometry":{"op":"history","output":output,"features":features}})).unwrap()
}

fn cube() -> Value {
    json!({"id":"base","geometry":{"op":"box","size":[10,10,10]}})
}

fn line(end: Value) -> Value {
    json!({"start":[0,0,0],"segments":[{"kind":"line","end":end}]})
}

fn close(actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected}"
    );
}

fn bounds(actual: [[f64; 3]; 2], min: [f64; 3], max: [f64; 3]) {
    for axis in 0..3 {
        close(actual[0][axis], min[axis], 1e-5);
        close(actual[1][axis], max[axis], 1e-5);
    }
}

fn saved(directory: &std::path::Path) -> (std::time::SystemTime, u64) {
    faber_wiring::project_fingerprint(directory).unwrap()
}

#[test]
fn scale_uniform_about_a_point_scales_volume_and_bounds() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Scale".into())).unwrap();
    let scaled = |about: Value| {
        recipe(
            json!([cube(), {"id":"big","geometry":{"op":"scale","source":"base","factor":2,"about":about}}]),
            "big",
        )
    };
    project
        .put_part("cube".into(), scaled(json!([0, 0, 0])))
        .unwrap();
    close(project.bodies[0].volume, 8000., 1e-5);
    bounds(project.bodies[0].bounds, [0.; 3], [20.; 3]);
    project
        .put_part("cube".into(), scaled(json!([5, 5, 5])))
        .unwrap();
    close(project.bodies[0].volume, 8000., 1e-5);
    bounds(project.bodies[0].bounds, [-5.; 3], [15.; 3]);
}

#[test]
fn scale_non_uniform_uses_one_factor_for_each_axis() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Scale".into())).unwrap();
    project
        .put_part(
            "cube".into(),
            recipe(
                json!([cube(), {"id":"stretch","geometry":{"op":"scale","source":"base","factor":[2,1,0.5],"about":[5,5,5]}}]),
                "stretch",
            ),
        )
        .unwrap();
    close(project.bodies[0].volume, 1000., 1e-5);
    bounds(project.bodies[0].bounds, [-5., 0., 2.5], [15., 10., 7.5]);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    close(bodies[0].volume, 1000., 1e-5);
}

#[test]
fn scale_follows_a_parameter_and_refuses_bad_factors() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Scale".into())).unwrap();
    project
        .set_params([("k".into(), "2".into())].into())
        .unwrap();
    project
        .put_part(
            "cube".into(),
            recipe(
                json!([cube(), {"id":"big","geometry":{"op":"scale","source":"base","factor":"k","about":[0,0,0]}}]),
                "big",
            ),
        )
        .unwrap();
    close(project.bodies[0].volume, 8000., 1e-5);
    project
        .set_params([("k".into(), "3".into())].into())
        .unwrap();
    close(project.bodies[0].volume, 27000., 1e-4);
    let before = saved(directory.path());
    for factor in ["0", "-1", "-0.5"] {
        assert!(
            project
                .set_params([("k".into(), factor.into())].into())
                .is_err(),
            "factor {factor} was accepted"
        );
        assert_eq!(saved(directory.path()), before);
    }
    for factor in [json!(0), json!(-2), json!([1, 0, 1]), json!([1, -1, 1])] {
        let invalid = recipe(
            json!([cube(), {"id":"big","geometry":{"op":"scale","source":"base","factor":factor,"about":[0,0,0]}}]),
            "big",
        );
        assert!(project.put_part("cube".into(), invalid).is_err());
        assert_eq!(saved(directory.path()), before);
    }
}

#[test]
fn scale_keeps_face_origins_for_uniform_and_non_uniform_factors() {
    for factor in [json!(2), json!([2, 2, 1])] {
        let directory = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(directory.path(), true, Some("Scale faces".into())).unwrap();
        project
            .put_part(
                "cube".into(),
                recipe(
                    json!([cube(), {"id":"big","geometry":{"op":"scale","source":"base","factor":factor,"about":[0,0,0]}},
                        {"id":"cap","geometry":{"op":"thicken","source":"big","thickness":1,"face":{"created_by":"base","normal":[0,0,1],"expect":1,"tolerance":0.000001}}}]),
                    "cap",
                ),
            )
            .unwrap();
        close(project.bodies[0].volume, 400., 1e-4);
    }
}

#[test]
fn pipe_makes_a_round_bar_and_a_tube_along_a_line() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Pipe".into())).unwrap();
    let pipe = |wall: Value| {
        recipe(
            json!([{"id":"pipe","geometry":{"op":"pipe","path":line(json!([100, 0, 0])),"diameter":10,"wall":wall}}]),
            "pipe",
        )
    };
    project.put_part("pipe".into(), pipe(Value::Null)).unwrap();
    close(
        project.bodies[0].volume,
        std::f64::consts::PI * 25. * 100.,
        1e-2,
    );
    bounds(project.bodies[0].bounds, [0., -5., -5.], [100., 5., 5.]);
    project.put_part("pipe".into(), pipe(json!(1))).unwrap();
    close(
        project.bodies[0].volume,
        std::f64::consts::PI * (25. - 16.) * 100.,
        1e-2,
    );
    bounds(project.bodies[0].bounds, [0., -5., -5.], [100., 5., 5.]);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    close(bodies[0].volume, std::f64::consts::PI * 9. * 100., 1e-2);
}

#[test]
fn pipe_follows_a_curve_feature_and_a_parameter() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Pipe".into())).unwrap();
    project
        .set_params([("len".into(), "100".into()), ("d".into(), "10".into())].into())
        .unwrap();
    project
        .put_part(
            "pipe".into(),
            recipe(
                json!([
                    {"id":"path","geometry":{"op":"curve","path":{"start":[0,0,0],"segments":[{"kind":"line","end":["len",0,0]}]}}},
                    {"id":"pipe","geometry":{"op":"pipe","path":"path","diameter":"d"}}
                ]),
                "pipe",
            ),
        )
        .unwrap();
    close(
        project.bodies[0].volume,
        std::f64::consts::PI * 25. * 100.,
        1e-2,
    );
    project
        .set_params([("len".into(), "50".into())].into())
        .unwrap();
    close(
        project.bodies[0].volume,
        std::f64::consts::PI * 25. * 50.,
        1e-2,
    );
    project
        .set_params([("d".into(), "4".into())].into())
        .unwrap();
    close(
        project.bodies[0].volume,
        std::f64::consts::PI * 4. * 50.,
        1e-2,
    );
    let before = saved(directory.path());
    assert!(
        project
            .set_params([("d".into(), "0".into())].into())
            .is_err()
    );
    assert_eq!(saved(directory.path()), before);
}

#[test]
fn pipe_sweeps_along_a_helix() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Pipe".into())).unwrap();
    project
        .put_part(
            "coil".into(),
            recipe(
                json!([
                    {"id":"path","geometry":{"op":"helix","radius":10,"pitch":10,"turns":1}},
                    {"id":"pipe","geometry":{"op":"pipe","path":"path","diameter":2}}
                ]),
                "pipe",
            ),
        )
        .unwrap();
    let length = ((20. * std::f64::consts::PI).powi(2) + 100.).sqrt();
    close(
        project.bodies[0].volume,
        std::f64::consts::PI * length,
        1e-2,
    );
}

#[test]
fn pipe_refuses_bad_size_wall_and_path() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Pipe".into())).unwrap();
    let good = || {
        recipe(
            json!([{"id":"pipe","geometry":{"op":"pipe","path":line(json!([100, 0, 0])),"diameter":10}}]),
            "pipe",
        )
    };
    project.put_part("pipe".into(), good()).unwrap();
    let before = saved(directory.path());
    let cases = [
        json!([{"id":"pipe","geometry":{"op":"pipe","path":line(json!([100, 0, 0])),"diameter":0}}]),
        json!([{"id":"pipe","geometry":{"op":"pipe","path":line(json!([100, 0, 0])),"diameter":10,"wall":5}}]),
        json!([{"id":"pipe","geometry":{"op":"pipe","path":line(json!([100, 0, 0])),"diameter":10,"wall":6}}]),
        json!([{"id":"pipe","geometry":{"op":"pipe","path":line(json!([100, 0, 0])),"diameter":10,"wall":0}}]),
        json!([cube(), {"id":"pipe","geometry":{"op":"pipe","path":"base","diameter":10}}]),
    ];
    for features in cases {
        assert!(
            project
                .put_part("pipe".into(), recipe(features, "pipe"))
                .is_err()
        );
        assert_eq!(saved(directory.path()), before);
    }
}

#[test]
fn path_pattern_spaces_five_cubes_evenly_along_a_line() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Path".into())).unwrap();
    let row = |count: Value, spacing: Value| {
        let mut geometry = json!({"op":"path_pattern","source":"base","path":line(json!([100, 0, 0])),"count":count});
        if !spacing.is_null() {
            geometry["spacing"] = spacing;
        }
        recipe(json!([cube(), {"id":"row","geometry":geometry}]), "row")
    };
    project
        .put_part("row".into(), row(json!(5), Value::Null))
        .unwrap();
    close(project.bodies[0].volume, 5000., 1e-4);
    bounds(project.bodies[0].bounds, [0.; 3], [110., 10., 10.]);
    project
        .put_part("row".into(), row(json!(5), json!(20)))
        .unwrap();
    close(project.bodies[0].volume, 5000., 1e-4);
    bounds(project.bodies[0].bounds, [0.; 3], [90., 10., 10.]);
    project
        .put_part("row".into(), row(json!(1), Value::Null))
        .unwrap();
    close(project.bodies[0].volume, 1000., 1e-4);
    let before = saved(directory.path());
    for (count, spacing) in [
        (json!(5), json!(30)),
        (json!(5), json!(0)),
        (json!(0), Value::Null),
        (json!(2.5), Value::Null),
    ] {
        assert!(project.put_part("row".into(), row(count, spacing)).is_err());
        assert_eq!(saved(directory.path()), before);
    }
}

#[test]
fn path_pattern_orient_turns_copies_to_the_path_tangent() {
    let bent = json!({"start":[0,0,0],"segments":[
        {"kind":"line","end":[100,0,0]},
        {"kind":"line","end":[100,100,0]}
    ]});
    let build = |orient: bool| {
        recipe(
            json!([
                {"id":"bar","geometry":{"op":"box","size":[10,2,2]}},
                {"id":"row","geometry":{"op":"path_pattern","source":"bar","path":bent,"count":2,"spacing":150,"orient":orient}}
            ]),
            "row",
        )
    };
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Orient".into())).unwrap();
    project.put_part("row".into(), build(true)).unwrap();
    close(project.bodies[0].volume, 80., 1e-4);
    bounds(project.bodies[0].bounds, [0.; 3], [100., 60., 2.]);
    project.put_part("row".into(), build(false)).unwrap();
    close(project.bodies[0].volume, 80., 1e-4);
    bounds(project.bodies[0].bounds, [0.; 3], [110., 52., 2.]);
}

#[test]
fn path_pattern_rebuilds_with_a_parameter_and_refuses_a_non_curve_path() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Path".into())).unwrap();
    project
        .set_params([("n".into(), "3".into())].into())
        .unwrap();
    project
        .put_part(
            "row".into(),
            recipe(
                json!([
                    cube(),
                    {"id":"track","geometry":{"op":"curve","path":{"start":[0,0,0],"segments":[{"kind":"line","end":[100,0,0]}]}}},
                    {"id":"row","geometry":{"op":"path_pattern","source":"base","path":"track","count":"n"}}
                ]),
                "row",
            ),
        )
        .unwrap();
    close(project.bodies[0].volume, 3000., 1e-4);
    project
        .set_params([("n".into(), "1".into())].into())
        .unwrap();
    close(project.bodies[0].volume, 1000., 1e-4);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    close(bodies[0].volume, 1000., 1e-4);
    let before = saved(directory.path());
    let invalid = recipe(
        json!([cube(), {"id":"row","geometry":{"op":"path_pattern","source":"base","path":"base","count":3}}]),
        "row",
    );
    assert!(project.put_part("row".into(), invalid).is_err());
    assert_eq!(saved(directory.path()), before);
}
