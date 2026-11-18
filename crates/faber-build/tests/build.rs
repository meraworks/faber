use faber_build::JsonnetSource;
use faber_kernel_occt::OcctKernel;
use faber_model::{Geometry, Scalar};
use faber_ports::source::DesignSource;
use std::path::Path;
use std::sync::Arc;

fn source() -> JsonnetSource {
    JsonnetSource::new(Arc::new(OcctKernel))
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn read_file(root: &Path, rel: &str) -> Vec<u8> {
    std::fs::read(root.join(rel)).unwrap()
}

#[test]
fn builds_parts_and_params() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "design.jsonnet",
        r#"{
  name: 'Rack',
  params: { width: '40' },
  parts: {
    plate: { name: 'Plate', colour: [120, 130, 140], geometry: { op: 'box', size: ['width', 10, 5] } },
    pin: { name: 'Pin', colour: [200, 200, 200], geometry: { op: 'cylinder', radius: 2, height: 8 } },
  },
}"#,
    );
    let first = source().ensure_built(root).unwrap().unwrap();
    assert_eq!(first.parts, 2);
    assert_eq!(first.added_parts, ["pin", "plate"]);
    assert!(source().ensure_built(root).unwrap().is_none());
    let data = faber_files::read(root).unwrap();
    assert_eq!(data.name, "Rack");
    assert_eq!(data.params["width"], "40");
    assert_eq!(data.parts.len(), 2);
    assert!(root.join("parts/plate.json").is_file());
    assert!(root.join("parts/pin.json").is_file());
    assert!(root.join("faber.json").is_file());
}

#[test]
fn a_changed_import_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "lib/block.libsonnet",
        "{ plate(w):: { name: 'Plate', colour: [1, 2, 3], geometry: { op: 'box', size: [w, 10, 5] } } }",
    );
    write(
        root,
        "design.jsonnet",
        "local block = import 'block.libsonnet';\n{ parts: { plate: block.plate(20) } }",
    );
    source().ensure_built(root).unwrap().unwrap();
    write(
        root,
        "lib/block.libsonnet",
        "{ plate(w):: { name: 'Plate', colour: [1, 2, 3], geometry: { op: 'box', size: [w, 10, 6] } } }",
    );
    let rebuilt = source().ensure_built(root).unwrap().unwrap();
    assert!(rebuilt.inputs.iter().any(|p| p == "lib/block.libsonnet"));
    assert_eq!(rebuilt.written, ["parts/plate.json", "faber.json"]);
    let data = faber_files::read(root).unwrap();
    let Geometry::Box { size } = &data.parts["plate"].geometry else {
        panic!("plate is not a box");
    };
    assert_eq!(size[2], Scalar::Number(6.));
}

#[test]
fn function_block_used_twice_gives_two_parts() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "design.jsonnet",
        r#"local faber = import 'faber.libsonnet';
local block(w) = {
  parts: {
    block: faber.part('Block', [faber.box('body', [w, 10, 5])], 'body', [90, 90, 90]),
  },
};
faber.merge([faber.prefix(block(20), 'left_'), faber.prefix(block(40), 'right_')])
"#,
    );
    source().build(root).unwrap();
    let data = faber_files::read(root).unwrap();
    let ids: Vec<&str> = data.parts.keys().map(String::as_str).collect();
    assert_eq!(ids, ["left_block", "right_block"]);
}

#[test]
fn duplicate_part_id_is_refused_with_the_ids() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "design.jsonnet",
        r#"local faber = import 'faber.libsonnet';
local part = faber.part('P', [faber.box('b', [1, 1, 1])], 'b', [1, 1, 1]);
faber.merge([
  { parts: { x: part, y: part } },
  { parts: { y: part, x: part } },
])
"#,
    );
    let error = source().build(root).unwrap_err();
    assert!(
        error.contains("more than one fragment has these parts: x, y"),
        "{error}"
    );
    assert!(!root.join("faber.json").exists());
}

#[test]
fn jsonnet_error_names_file_and_line() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "design.jsonnet",
        "{\n  name: 'Bad',\n  parts: {\n    a: missing_variable,\n  },\n}\n",
    );
    let error = source().build(root).unwrap_err();
    assert!(error.contains("design.jsonnet"), "{error}");
    assert!(error.contains("design.jsonnet:4:"), "{error}");
    assert!(error.contains(":4"), "{error}");
}

#[test]
fn placement_survives_a_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let design = |width: &str| {
        format!(
            "{{ params: {{ width: '{width}' }}, parts: {{ a: {{ name: 'A', colour: [1, 1, 1], geometry: {{ op: 'box', size: ['width', 10, 5] }} }} }} }}"
        )
    };
    write(root, "design.jsonnet", &design("40"));
    source().build(root).unwrap();
    let mut data = faber_files::read(root).unwrap();
    data.parts.get_mut("a").unwrap().placement.translation =
        [Scalar::Number(5.), Scalar::Number(0.), Scalar::Number(0.)];
    faber_files::layout::write(root, &data).unwrap();
    write(root, "design.jsonnet", &design("50"));
    source().ensure_built(root).unwrap().unwrap();
    let data = faber_files::read(root).unwrap();
    assert_eq!(data.params["width"], "50");
    assert_eq!(
        data.parts["a"].placement.translation,
        [Scalar::Number(5.), Scalar::Number(0.), Scalar::Number(0.)]
    );
}

#[test]
fn removed_part_loses_its_placement() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "design.jsonnet",
        "{ parts: {\n  a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } },\n  b: { name: 'B', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } },\n} }",
    );
    source().build(root).unwrap();
    let mut data = faber_files::read(root).unwrap();
    data.parts.get_mut("b").unwrap().placement.translation =
        [Scalar::Number(9.), Scalar::Number(0.), Scalar::Number(0.)];
    faber_files::layout::write(root, &data).unwrap();
    write(
        root,
        "design.jsonnet",
        "{ parts: {\n  a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } },\n} }",
    );
    let report = source().ensure_built(root).unwrap().unwrap();
    assert_eq!(report.removed_parts, ["b"]);
    assert!(!root.join("parts/b.json").exists());
    let placement = std::fs::read_to_string(root.join("placement.json")).unwrap();
    assert!(!placement.contains("\"b\""), "{placement}");
}

#[test]
fn invalid_recipe_is_refused_and_files_stay() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "design.jsonnet",
        "{ parts: { a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } } } }",
    );
    source().build(root).unwrap();
    let before = [
        read_file(root, "faber.json"),
        read_file(root, "parts/a.json"),
        read_file(root, "placement.json"),
    ];
    write(
        root,
        "design.jsonnet",
        "{ parts: {\n  a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } },\n  c: { name: 'C', colour: [1, 1, 1], geometry: { op: 'box', size: [-1, 10, 10] } },\n} }",
    );
    let error = source().build(root).unwrap_err();
    assert!(error.contains("c:"), "{error}");
    assert!(!root.join("parts/c.json").exists());
    let after = [
        read_file(root, "faber.json"),
        read_file(root, "parts/a.json"),
        read_file(root, "placement.json"),
    ];
    assert_eq!(before, after);
}

#[test]
fn removed_part_in_a_motion_joint_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let two_parts = "{ parts: {\n  a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } },\n  b: { name: 'B', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } },\n} }";
    write(root, "design.jsonnet", two_parts);
    source().build(root).unwrap();
    let mut data = faber_files::read(root).unwrap();
    data.motion = serde_json::from_value(serde_json::json!({
        "joints": {"hinge": {"kind": "revolute", "parts": ["b"], "origin": [0, 0, 0], "axis": [0, 0, 1]}},
        "gears": {},
        "linkages": {}
    }))
    .unwrap();
    faber_files::layout::write(root, &data).unwrap();
    let names = [
        "faber.json",
        "parts/a.json",
        "parts/b.json",
        "placement.json",
        "assembly.json",
    ];
    let before: Vec<Vec<u8>> = names.iter().map(|n| read_file(root, n)).collect();
    write(
        root,
        "design.jsonnet",
        "{ parts: {\n  a: { name: 'A', colour: [1, 1, 1], geometry: { op: 'box', size: [10, 10, 10] } },\n} }",
    );
    let error = source().build(root).unwrap_err();
    assert!(
        error.contains("Part b is in motion joint hinge. Remove the joint first."),
        "{error}"
    );
    let after: Vec<Vec<u8>> = names.iter().map(|n| read_file(root, n)).collect();
    assert_eq!(before, after);
    let data = faber_files::read(root).unwrap();
    faber_core::build(&data, &OcctKernel).unwrap();
}

#[test]
fn changing_only_the_material_is_in_the_build_report() {
    let dir = tempfile::tempdir().unwrap();
    let src = |id: &str| {
        format!(
            "local f = import 'faber.libsonnet'; {{parts:{{pin:f.part('Pin',[f.box('base',[10,10,10])],'base',material='{id}')}}}}"
        )
    };
    write(dir.path(), "design.jsonnet", &src("pla"));
    source().build(dir.path()).unwrap();
    write(dir.path(), "design.jsonnet", &src("steel-s235"));
    let report = source().build(dir.path()).unwrap();
    assert!(report.written.contains(&"parts/pin.json".to_owned()));
    assert_eq!(
        faber_files::read(dir.path()).unwrap().parts["pin"]
            .material
            .as_deref(),
        Some("steel-s235")
    );
}

#[test]
fn source_study_helpers_write_the_load_and_keep_it_when_studies_are_omitted() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let source_text = r#"local f=import 'faber.libsonnet';
local face(n)={normal:n,tolerance:0.001,expect:1};
{parts:{bar:f.part('Bar',[f.box('beam',[20,4,4])],'beam',material='pla')},
 studies:{bend:f.study('bar',[f.fixed(face([-1,0,0]))],[f.force(face([1,0,0]),[0,0,-1])])}}"#;
    write(root, "design.jsonnet", source_text);
    let report = source().ensure_built(root).unwrap().unwrap();
    assert!(report.written.iter().any(|p| p == "analysis.json"));
    let saved = faber_files::read(root).unwrap().studies;
    assert_eq!(saved["bend"].part, "bar");
    let parts_only = source_text.split(",\n studies:").next().unwrap().to_owned() + "}";
    write(root, "design.jsonnet", &parts_only);
    source().ensure_built(root).unwrap().unwrap();
    assert_eq!(faber_files::read(root).unwrap().studies, saved);
}
