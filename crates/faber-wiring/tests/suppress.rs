use faber_core::Project;
use faber_model::{Part, Scalar};
use serde_json::json;
use std::f64::consts::PI;

const BOX: f64 = 6000.;
// A vertical fillet with radius 1 on a 30 mm high block removes 30 * (4 - pi).
const ROUND: f64 = BOX - 30. * (4. - PI);
// A hole with diameter 4 and depth 10 removes 40 * pi.
const DRILL: f64 = ROUND - 40. * PI;

// The base is a box. "round" fillets it. "drill" holes "round". "side" depends on "drill".
// "out" is a separate box. The output id is a parameter.
fn recipe(output: &str) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Suppress","colour":[100,140,170],"geometry":{"op":"history","output":output,"features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,"height"]}},
        {"id":"round","geometry":{"op":"fillet","source":"base","edges":{"parallel":[0,0,1],"expect":4,"tolerance":0.000001},"radius":1}},
        {"id":"drill","geometry":{"op":"hole","source":"round","diameter":4,"depth":10,"head":null,"placement":{"translation":[5,10,30],"rotation":[180,0,0]}}},
        {"id":"side","geometry":{"op":"transform","source":"drill","placement":{"translation":[30,0,0]}}},
        {"id":"out","geometry":{"op":"box","size":[10,20,"height"]}}
    ]}})).unwrap()
}

fn open(directory: &std::path::Path, output: &str) -> Project {
    let mut project = faber_wiring::open(directory, true, Some("Suppress".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project.put_part("block".into(), recipe(output)).unwrap();
    project
}

fn suppressed_ids(project: &Project) -> Vec<String> {
    let faber_model::Geometry::History { features, .. } = &project.data.parts["block"].geometry
    else {
        panic!("Part has no history");
    };
    features
        .iter()
        .filter(|feature| feature.suppressed)
        .map(|feature| feature.id.clone())
        .collect()
}

fn parts_of(root: &std::path::Path) -> serde_json::Value {
    let file = std::fs::read(root.join("faber.json")).unwrap();
    serde_json::from_slice::<serde_json::Value>(&file).unwrap()["parts"].clone()
}

#[track_caller]
fn assert_volume(project: &Project, expected: f64) {
    let actual = project.bodies[0].volume;
    assert!((actual - expected).abs() < 1e-4, "{actual} != {expected}");
}

#[test]
fn suppressing_the_output_hole_shows_the_fillet_and_unsuppress_restores_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "drill");
    assert_volume(&project, DRILL);
    project
        .set_suppressed("block", "drill", true, false)
        .unwrap();
    assert_volume(&project, ROUND);
    project
        .set_suppressed("block", "drill", false, false)
        .unwrap();
    assert_volume(&project, DRILL);
}

#[test]
fn suppressing_the_output_fillet_shows_the_box_and_unsuppress_restores_it() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "round");
    assert_volume(&project, ROUND);
    project
        .set_suppressed("block", "round", true, false)
        .unwrap();
    assert_volume(&project, BOX);
    project
        .set_suppressed("block", "round", false, false)
        .unwrap();
    assert_volume(&project, ROUND);
}

#[test]
fn suppressed_middle_hole_leaves_the_later_fillet_on_the_undrilled_solid() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Middle".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Middle","colour":[100,140,170],"geometry":{"op":"history","output":"round","features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,"height"]}},
        {"id":"drill","geometry":{"op":"hole","source":"base","diameter":4,"depth":10,"head":null,"placement":{"translation":[5,10,30],"rotation":[180,0,0]}}},
        {"id":"round","geometry":{"op":"fillet","source":"drill","edges":{"parallel":[1,0,0],"expect":4,"tolerance":0.000001},"radius":1}}
    ]}}))
    .unwrap();
    project.put_part("block".into(), part).unwrap();
    // Four edges along x, each 10 mm long, lose (1 - pi / 4) mm2 per mm.
    let edges = 4. * (1. - PI / 4.) * 10.;
    let full = BOX - 40. * PI - edges;
    assert_volume(&project, full);
    project
        .set_suppressed("block", "drill", true, false)
        .unwrap();
    assert_volume(&project, BOX - edges);
    project
        .set_suppressed("block", "drill", false, false)
        .unwrap();
    assert_volume(&project, full);
}

#[test]
fn suppressed_box_with_a_dependant_is_refused_without_cascade() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "drill");
    let before = parts_of(directory.path());
    let error = project
        .set_suppressed("block", "base", true, false)
        .unwrap_err();
    assert!(error.contains("round"), "{error}");
    assert!(error.contains("cascade"), "{error}");
    assert!(suppressed_ids(&project).is_empty());
    assert_eq!(before, parts_of(directory.path()));
}

#[test]
fn cascade_suppresses_the_dependants_of_a_box() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "out");
    project.set_suppressed("block", "base", true, true).unwrap();
    assert_eq!(
        suppressed_ids(&project),
        vec![
            "base".to_owned(),
            "round".to_owned(),
            "drill".to_owned(),
            "side".to_owned()
        ]
    );
    assert_volume(&project, BOX);
}

#[test]
fn difference_with_suppressed_right_operand_is_refused_without_cascade() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Cut".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Cut","colour":[100,140,170],"geometry":{"op":"history","output":"cut","features":[
        {"id":"base","geometry":{"op":"box","size":[10,20,"height"]}},
        {"id":"cutter","geometry":{"op":"box","size":[2,2,40]}},
        {"id":"cut","geometry":{"op":"difference","left":"base","right":"cutter"}}
    ]}}))
    .unwrap();
    project.put_part("block".into(), part).unwrap();
    assert_volume(&project, BOX - 2. * 2. * 30.);
    let error = project
        .set_suppressed("block", "cutter", true, false)
        .unwrap_err();
    assert!(error.contains("cut"), "{error}");
    assert!(error.contains("cascade"), "{error}");
    assert!(suppressed_ids(&project).is_empty());
    project
        .set_suppressed("block", "cutter", true, true)
        .unwrap();
    assert_eq!(
        suppressed_ids(&project),
        vec!["cutter".to_owned(), "cut".to_owned()]
    );
    assert_volume(&project, BOX);
}

#[test]
fn chain_of_two_suppressed_modifiers_shows_the_box() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "drill");
    // The hole still builds on the box when its fillet is suppressed.
    project
        .set_suppressed("block", "round", true, false)
        .unwrap();
    assert_volume(&project, BOX - 40. * PI);
    project
        .set_suppressed("block", "drill", true, false)
        .unwrap();
    assert_volume(&project, BOX);
    assert_eq!(
        suppressed_ids(&project),
        vec!["round".to_owned(), "drill".to_owned()]
    );
}

#[test]
fn output_without_an_input_cannot_be_suppressed() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "out");
    let before = parts_of(directory.path());
    let error = project
        .set_suppressed("block", "out", true, false)
        .unwrap_err();
    assert!(
        error.contains("output feature cannot be suppressed"),
        "{error}"
    );
    let error = project
        .set_suppressed("block", "out", true, true)
        .unwrap_err();
    assert!(
        error.contains("output feature cannot be suppressed"),
        "{error}"
    );
    assert_eq!(before, parts_of(directory.path()));
}

#[test]
fn unsuppressing_a_feature_with_a_source_without_solid_is_refused() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "out");
    project.set_suppressed("block", "base", true, true).unwrap();
    let error = project
        .set_suppressed("block", "round", false, false)
        .unwrap_err();
    assert!(error.contains("base"), "{error}");
    assert_eq!(suppressed_ids(&project).len(), 4);
}

#[test]
fn suppressed_flag_and_aliases_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "drill");
    project
        .set_suppressed("block", "drill", true, false)
        .unwrap();
    drop(project);
    let mut reopened = faber_wiring::open(directory.path(), false, None).unwrap();
    assert_eq!(suppressed_ids(&reopened), vec!["drill".to_owned()]);
    assert_volume(&reopened, ROUND);
    reopened
        .set_suppressed("block", "drill", false, false)
        .unwrap();
    assert_volume(&reopened, DRILL);
}

#[test]
fn suppressed_flag_is_saved_only_where_it_is_set() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "out");
    let before = parts_of(directory.path());
    assert!(!before.to_string().contains("suppressed"));
    project
        .set_suppressed("block", "side", true, false)
        .unwrap();
    let saved = std::fs::read(directory.path().join("faber.json")).unwrap();
    assert_eq!(
        String::from_utf8_lossy(&saved)
            .matches("\"suppressed\"")
            .count(),
        1
    );
    drop(project);
    let mut reopened = faber_wiring::open(directory.path(), false, None).unwrap();
    assert_eq!(suppressed_ids(&reopened), vec!["side".to_owned()]);
    reopened
        .set_suppressed("block", "side", false, false)
        .unwrap();
    assert_eq!(before, parts_of(directory.path()));
}

#[test]
fn suppress_is_one_undo_step() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "out");
    project.set_suppressed("block", "base", true, true).unwrap();
    assert_eq!(suppressed_ids(&project).len(), 4);
    project.undo().unwrap();
    assert!(suppressed_ids(&project).is_empty());
}

#[test]
fn dangling_feature_suppress_keeps_the_output_volume() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = open(directory.path(), "out");
    project
        .set_suppressed("block", "side", true, false)
        .unwrap();
    assert_volume(&project, BOX);
    project
        .set_suppressed("block", "side", false, false)
        .unwrap();
    assert_volume(&project, BOX);
}
