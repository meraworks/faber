use faber_kernel_occt::OcctKernel;
use faber_model::{Part, Scalar};
use faber_ports::Kernel;
use serde_json::{Value, json};

fn text(value: &str, at: [f64; 2], align: &str, angle: f64) -> Value {
    json!({"kind":"text","text":value,"height":10,"at":at,"align":align,"angle_deg":angle})
}

fn label(outer: Value, vector: [f64; 3]) -> Value {
    json!({"name":"Label","colour":[100,140,170],"geometry":{"op":"history","output":"solid","features":[
        {"id":"profile","geometry":{"op":"sketch","profile":{"outer":outer}}},
        {"id":"solid","geometry":{"op":"extrude_sketch","source":"profile","vector":vector}}
    ]}})
}

macro_rules! put {
    ($project:expr, $recipe:expr) => {
        $project
            .put_part(
                "label".into(),
                serde_json::from_value::<Part<Scalar>>($recipe).unwrap(),
            )
            .unwrap()
    };
}

fn size(bounds: [[f64; 3]; 2]) -> [f64; 3] {
    std::array::from_fn(|axis| bounds[1][axis] - bounds[0][axis])
}

fn solids(bodies: &[faber_ports::Body]) -> u32 {
    let shape = bodies[0].shape.as_ref().unwrap();
    OcctKernel.properties(shape).unwrap().solids
}

#[test]
fn capital_i_extrudes_to_its_stroke_rectangle() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Text I".into())).unwrap();
    put!(
        project,
        label(text("I", [0., 0.], "left", 0.), [0., 0., 1.])
    );
    let [width, height, depth] = size(project.bodies[0].bounds);
    assert!((height - 10.).abs() < 1e-6, "height {height}");
    assert!((depth - 1.).abs() < 1e-6);
    // The stroke of "I" is one rectangle, so the volume is its area.
    assert!((project.bodies[0].volume - width * height).abs() < 1e-3);
}

#[test]
fn o_has_a_counter_inside_its_outline() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Text O".into())).unwrap();
    put!(
        project,
        label(text("O", [0., 0.], "left", 0.), [0., 0., 1.])
    );
    let [width, height, _] = size(project.bodies[0].bounds);
    let filled = std::f64::consts::FRAC_PI_4 * width * height;
    let volume = project.bodies[0].volume;
    // A filled outline would be close to the ellipse area. The counter removes most of it.
    assert!(volume < 0.5 * filled, "volume {volume}, filled {filled}");
    assert!(volume > 0.1 * filled, "volume {volume}, filled {filled}");
    assert_eq!(solids(&project.bodies), 1);
}

#[test]
fn two_letters_with_separate_outlines_make_separate_solids() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Text Hi".into())).unwrap();
    put!(
        project,
        label(text("H", [0., 0.], "left", 0.), [0., 0., 1.])
    );
    let h_volume = project.bodies[0].volume;
    put!(
        project,
        label(text("i", [0., 0.], "left", 0.), [0., 0., 1.])
    );
    let i_volume = project.bodies[0].volume;
    assert_eq!(solids(&project.bodies), 2, "the dot and the stem of i");
    put!(
        project,
        label(text("Hi", [0., 0.], "left", 0.), [0., 0., 1.])
    );
    assert_eq!(solids(&project.bodies), 3);
    assert!((project.bodies[0].volume - (h_volume + i_volume)).abs() < 1e-3);
}

#[test]
fn alignment_moves_the_text_about_its_anchor() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Text align".into())).unwrap();
    put!(
        project,
        label(text("I", [10., 0.], "left", 0.), [0., 0., 1.])
    );
    let left = project.bodies[0].bounds;
    put!(
        project,
        label(text("I", [10., 0.], "centre", 0.), [0., 0., 1.])
    );
    let centre = project.bodies[0].bounds;
    put!(
        project,
        label(text("I", [10., 0.], "right", 0.), [0., 0., 1.])
    );
    let right = project.bodies[0].bounds;
    // The centre of the bounds stays near the anchor.
    assert!(((centre[0][0] + centre[1][0]) / 2. - 10.).abs() < 1.);
    // Left starts at the anchor. Right ends near it. Centre moves left of left.
    assert!(left[0][0] > 10. && left[0][0] < 11.5, "left {left:?}");
    assert!(centre[0][0] < left[0][0] - 0.5, "centre {centre:?}");
    assert!(right[1][0] < 10.5 && right[1][0] > 8.5, "right {right:?}");
}

#[test]
fn rotation_turns_the_text_about_its_anchor() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Text turn".into())).unwrap();
    put!(
        project,
        label(text("I", [0., 0.], "left", 0.), [0., 0., 1.])
    );
    let [width, height, _] = size(project.bodies[0].bounds);
    put!(
        project,
        label(text("I", [0., 0.], "left", 90.), [0., 0., 1.])
    );
    let [turned_x, turned_y, _] = size(project.bodies[0].bounds);
    assert!((turned_x - height).abs() < 1e-6, "{turned_x}");
    assert!((turned_y - width).abs() < 1e-6, "{turned_y}");
}

#[test]
fn invalid_text_is_refused_and_the_file_is_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Text refuse".into())).unwrap();
    put!(
        project,
        label(text("I", [0., 0.], "left", 0.), [0., 0., 1.])
    );
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let mut zero_height = text("I", [0., 0.], "left", 0.);
    zero_height["height"] = json!(0);
    let mut negative_height = text("I", [0., 0.], "left", 0.);
    negative_height["height"] = json!(-2);
    let mut unknown_font = text("I", [0., 0.], "left", 0.);
    unknown_font["font"] = json!("no-such-font");
    let cases = [
        text("", [0., 0.], "left", 0.),
        text("a\nb", [0., 0.], "left", 0.),
        text("I", [0., 0.], "justified", 0.),
        zero_height,
        negative_height,
        unknown_font,
    ];
    for case in cases {
        let recipe = label(case.clone(), [0., 0., 1.]);
        // A bad enum value fails while the file is read. The build refuses the rest.
        let refused = match serde_json::from_value::<Part<Scalar>>(recipe) {
            Err(_) => true,
            Ok(part) => project.put_part("label".into(), part).is_err(),
        };
        assert!(refused, "accepted {case}");
        assert_eq!(
            faber_wiring::project_fingerprint(directory.path()).unwrap(),
            before
        );
    }
}

#[test]
fn height_parameter_rebuilds_the_text() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Text height".into())).unwrap();
    project
        .set_params([("h".into(), "10".into())].into())
        .unwrap();
    let mut outer = text("I", [0., 0.], "left", 0.);
    outer["height"] = json!("h");
    put!(project, label(outer, [0., 0., 1.]));
    let [_, first, _] = size(project.bodies[0].bounds);
    let first_volume = project.bodies[0].volume;
    project
        .set_params([("h".into(), "20".into())].into())
        .unwrap();
    let [_, second, _] = size(project.bodies[0].bounds);
    assert!((first - 10.).abs() < 1e-6);
    assert!((second - 20.).abs() < 1e-6);
    // Doubling the height doubles both sides of the rectangle.
    assert!((project.bodies[0].volume - 4. * first_volume).abs() < 1e-3);
}

#[test]
fn raised_and_engraved_text_on_a_face_plane() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Text emboss".into())).unwrap();
    let block = |vector: f64, operation: &str| {
        json!({"name":"Tag","colour":[100,140,170],"geometry":{"op":"history","output":"tag","features":[
            {"id":"base","geometry":{"op":"box","size":[20,20,5]}},
            {"id":"top","geometry":{"op":"plane_on_face","source":"base","face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"offset":0}},
            {"id":"letters","geometry":{"op":"sketch_on_plane","plane":"top","profile":{"outer":text("I", [-1., -5.], "left", 0.)}}},
            {"id":"cut","geometry":{"op":"extrude_sketch","source":"letters","vector":[0,0,vector]}},
            {"id":"tag","geometry":{"op":operation,"left":"base","right":"cut"}}
        ]}})
    };
    put!(project, block(1., "union"));
    let raised = project.bodies[0].volume;
    put!(project, block(-2., "difference"));
    let engraved = project.bodies[0].volume;
    // The raised letters add one millimetre of their area. The engraving removes two millimetres.
    let area = raised - 2000.;
    assert!(area > 1., "no raised letters: {raised}");
    assert!((engraved - (2000. - 2. * area)).abs() < 1e-2, "{engraved}");
}

#[test]
fn text_survives_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Text reopen".into())).unwrap();
    put!(
        project,
        label(text("Hi", [0., 0.], "centre", 15.), [0., 0., 1.])
    );
    let volume = project.bodies[0].volume;
    drop(project);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - volume).abs() < 1e-3);
}

#[test]
fn documented_raised_and_engraved_examples_build() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Text docs".into())).unwrap();
    let base = r#"{"id":"base","geometry":{"op":"box","size":[40,20,5]}},
  {"id":"top","geometry":{"op":"plane_on_face","source":"base","face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"offset":0}},
  {"id":"letters","geometry":{"op":"sketch_on_plane","plane":"top","profile":{"outer":{"kind":"text","text":"LAB-01","height":6,"at":[0,-3],"align":"centre"}}}},"#;
    let raised: Value = serde_json::from_str(&format!(
        r#"{{"name":"Tag","colour":[100,140,170],"geometry":{{"op":"history","output":"tag","features":[
  {base}
  {{"id":"raised","geometry":{{"op":"extrude_sketch","source":"letters","vector":[0,0,1]}}}},
  {{"id":"tag","geometry":{{"op":"union","left":"base","right":"raised"}}}}
]}}}}"#
    ))
    .unwrap();
    let engraved: Value = serde_json::from_str(&format!(
        r#"{{"name":"Tag","colour":[100,140,170],"geometry":{{"op":"history","output":"tag","features":[
  {base}
  {{"id":"cut","geometry":{{"op":"extrude_sketch","source":"letters","vector":[0,0,-1]}}}},
  {{"id":"tag","geometry":{{"op":"difference","left":"base","right":"cut"}}}}
]}}}}"#
    ))
    .unwrap();
    put!(project, raised);
    let raised_volume = project.bodies[0].volume;
    assert!(raised_volume > 4000.);
    put!(project, engraved);
    let engraved_volume = project.bodies[0].volume;
    // Both examples use the same letters at the same depth.
    assert!((raised_volume + engraved_volume - 8000.).abs() < 1e-3);
}
