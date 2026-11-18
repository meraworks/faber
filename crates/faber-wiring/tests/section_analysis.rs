use faber_model::{Edit, Part, Scalar, SectionAnalysis};
use serde_json::{Value, json};

fn project(dir: &tempfile::TempDir) -> faber_core::Project {
    faber_wiring::open(dir.path(), true, Some("Sections".into())).unwrap()
}

fn part(value: Value) -> Part<Scalar> {
    serde_json::from_value(value).unwrap()
}

fn section(value: Value) -> SectionAnalysis<Scalar> {
    serde_json::from_value(value).unwrap()
}

fn set(project: &mut faber_core::Project, id: &str, value: Value) -> Result<(), String> {
    project.batch(vec![Edit::SetSection {
        id: id.into(),
        section: section(value),
    }])
}

fn block(project: &mut faber_core::Project) {
    project
        .put_part(
            "block".into(),
            part(json!({
                "name":"Block","colour":[100,140,170],
                "geometry":{"op":"box","size":[10,20,30]}
            })),
        )
        .unwrap();
}

fn assert_near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-6,
        "expected {expected}, found {actual}"
    );
}

#[test]
fn plane_cut_of_a_box_gives_exact_area_and_perimeter() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    set(
        &mut project,
        "mid",
        json!({"plane":{"kind":"plane","origin":[0,0,15],"normal":[0,0,1]}}),
    )
    .unwrap();
    let report = project.section_info("mid").unwrap();
    assert_eq!(report.parts.len(), 1);
    let cut = &report.parts[0];
    assert_eq!(cut.id, "block");
    assert_near(cut.area_mm2, 200.);
    assert_near(cut.perimeter_mm, 60.);
    assert_eq!(cut.regions, 1);
    assert_eq!(cut.holes, 0);
    assert_eq!(cut.min_wall_mm, None);
    let bounds = cut.bounds_plane_mm.unwrap();
    assert_near(bounds[1][0] - bounds[0][0], 10.);
    assert_near(bounds[1][1] - bounds[0][1], 20.);
}

#[test]
fn hollow_tube_cut_gives_the_ring_area_and_one_hole() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    project
        .put_part(
            "tube".into(),
            part(json!({
                "name":"Tube","colour":[100,140,170],
                "geometry":{"op":"history","output":"cut","features":[
                    {"id":"outer","geometry":{"op":"box","size":[10,20,30]}},
                    {"id":"bore","geometry":{"op":"box","size":[6,16,40]}},
                    {"id":"tool","geometry":{"op":"transform","source":"bore","placement":{"translation":[2,2,-5]}}},
                    {"id":"cut","geometry":{"op":"difference","left":"outer","right":"tool"}}
                ]}
            })),
        )
        .unwrap();
    set(
        &mut project,
        "mid",
        json!({"plane":{"kind":"plane","origin":[0,0,15],"normal":[0,0,1]}}),
    )
    .unwrap();
    let cut = &project.section_info("mid").unwrap().parts[0];
    assert_near(cut.area_mm2, 200. - 6. * 16.);
    assert_near(cut.perimeter_mm, 60. + 2. * (6. + 16.));
    assert_eq!(cut.regions, 1);
    assert_eq!(cut.holes, 1);
    // The wall is 2 mm thick on each side.
    assert_near(cut.min_wall_mm.unwrap(), 2.);
}

#[test]
fn reference_section_uses_the_plane_feature_and_its_offset() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    project
        .put_part(
            "frame".into(),
            part(json!({
                "name":"Frame","colour":[100,140,170],
                "geometry":{"op":"history","output":"base","features":[
                    {"id":"base","geometry":{"op":"box","size":[10,20,30]}},
                    {"id":"mid","geometry":{"op":"plane","placement":{"translation":[0,0,12],"rotation":[0,0,0]}}}
                ]}
            })),
        )
        .unwrap();
    set(
        &mut project,
        "ref",
        json!({"plane":{"kind":"reference","part":"frame","feature":"mid","offset":3}}),
    )
    .unwrap();
    let report = project.section_info("ref").unwrap();
    assert_near(report.origin_mm[2], 15.);
    assert_near(report.parts[0].area_mm2, 200.);
}

#[test]
fn face_section_offsets_along_the_outward_normal() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    set(
        &mut project,
        "under_top",
        json!({"plane":{"kind":"face","part":"block",
            "face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001},"offset":-5}}),
    )
    .unwrap();
    let report = project.section_info("under_top").unwrap();
    assert_near(report.origin_mm[2], 25.);
    assert_near(report.normal[2], 1.);
    assert_near(report.parts[0].area_mm2, 200.);
}

#[test]
fn parameter_change_moves_the_section_origin() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    project
        .set_params(std::collections::BTreeMap::from([(
            "h".into(),
            "15".into(),
        )]))
        .unwrap();
    set(
        &mut project,
        "mid",
        json!({"plane":{"kind":"plane","origin":[0,0,"h"],"normal":[0,0,1]}}),
    )
    .unwrap();
    assert_near(project.section_info("mid").unwrap().origin_mm[2], 15.);
    project
        .set_params(std::collections::BTreeMap::from([(
            "h".into(),
            "20".into(),
        )]))
        .unwrap();
    let report = project.section_info("mid").unwrap();
    assert_near(report.origin_mm[2], 20.);
    assert_near(report.parts[0].area_mm2, 200.);
}

#[test]
fn plane_outside_the_part_gives_zero_area() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    set(
        &mut project,
        "far",
        json!({"plane":{"kind":"plane","origin":[0,0,50],"normal":[0,0,1]}}),
    )
    .unwrap();
    let cut = &project.section_info("far").unwrap().parts[0];
    assert_near(cut.area_mm2, 0.);
    assert_near(cut.perimeter_mm, 0.);
    assert_eq!(cut.regions, 0);
}

#[test]
fn malformed_and_unknown_references_are_refused_and_the_file_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    let before = faber_wiring::project_fingerprint(dir.path()).unwrap();
    let refused = [
        json!({"plane":{"kind":"plane","origin":[0,0,0],"normal":[0,0,0]}}),
        json!({"plane":{"kind":"face","part":"missing",
            "face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001}}}),
        json!({"plane":{"kind":"reference","part":"block","feature":"nothing"}}),
        json!({"plane":{"kind":"plane","origin":[0,0,0],"normal":[0,0,1]},"parts":["missing"]}),
    ];
    for value in refused {
        assert!(set(&mut project, "bad", value).is_err());
    }
    assert!(project.data.sections.is_empty());
    assert_eq!(
        faber_wiring::project_fingerprint(dir.path()).unwrap(),
        before
    );
}

#[test]
fn undo_removes_a_new_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    set(
        &mut project,
        "mid",
        json!({"plane":{"kind":"plane","origin":[0,0,15],"normal":[0,0,1]}}),
    )
    .unwrap();
    assert_eq!(project.data.sections.len(), 1);
    project.undo().unwrap();
    assert!(project.data.sections.is_empty());
}

#[test]
fn remove_section_deletes_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    set(
        &mut project,
        "mid",
        json!({"plane":{"kind":"plane","origin":[0,0,15],"normal":[0,0,1]}}),
    )
    .unwrap();
    project
        .batch(vec![Edit::RemoveSection { id: "mid".into() }])
        .unwrap();
    assert!(project.data.sections.is_empty());
    assert!(project.section_info("mid").is_err());
}

#[test]
fn removing_a_part_refuses_when_a_plane_uses_it_and_prunes_part_lists() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    project
        .put_part(
            "other".into(),
            part(json!({
                "name":"Other","colour":[100,140,170],
                "geometry":{"op":"box","size":[10,10,10]}
            })),
        )
        .unwrap();
    set(
        &mut project,
        "mid",
        json!({"plane":{"kind":"plane","origin":[0,0,5],"normal":[0,0,1]},
            "parts":["block","other"]}),
    )
    .unwrap();
    set(
        &mut project,
        "face",
        json!({"plane":{"kind":"face","part":"block",
            "face":{"normal":[0,0,1],"expect":1,"tolerance":0.000001}}}),
    )
    .unwrap();
    assert!(project.remove_part("block").is_err());
    project.remove_part("other").unwrap();
    assert_eq!(
        project.data.sections["mid"].parts,
        vec!["block".to_string()]
    );
}

#[test]
fn project_without_sections_writes_no_analysis_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = project(&dir);
    block(&mut project);
    drop(project);
    assert!(!dir.path().join("analysis.json").exists());
    let data = faber_files::read(dir.path()).unwrap();
    assert!(data.sections.is_empty());
}
