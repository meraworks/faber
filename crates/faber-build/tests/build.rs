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
fn plastic_helpers_preserve_the_recipe_fields() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"
local f = import 'faber.libsonnet';
local l = [[[0,1],[3,4]]];
local s = {normal:[0,0,1],tolerance:0.001,expect:1};
{ parts: { box: f.part('Box', [
    f.box('base',[10,10,2]),
    f.rib('r','base','floor',l,'wall',side='left',draft_deg=1),
    f.web('w','r','floor',l,2),
    f.boss('b','w','floor',[4,4],8,6,hole_diameter=3,hole_depth=4,
           draft_deg=2,ribs={count:'n+2',thickness:1,length:2,height:3},fillet=0.4),
    f.snap_hook('h','b','floor',[4,4],90,15,5,2,1.5,3,tip_thickness=1,
                lead_deg=40,return_deg=80,root_fillet=0.5),
    f.lip('l','h',s,'inner','outside',1,2),
    f.groove('g','l',s,'outer','inside',1,2,0.15)
], 'g') } }"#,
    );
    let current = serde_json::from_value(serde_json::json!({
        "schema":2,"revision":0,"name":"Test","params":{},"parts":{}
    }))
    .unwrap();
    let data = faber_build::evaluate(dir.path(), &current).unwrap().data;
    let Geometry::History { features, .. } = &data.parts["box"].geometry else {
        panic!()
    };
    let recipes: Vec<_> = features
        .iter()
        .map(|f| serde_json::to_value(&f.geometry).unwrap())
        .collect();
    assert_eq!(recipes[1]["thickness"], "wall");
    assert_eq!(recipes[1]["side"], "left");
    assert_eq!(recipes[2]["op"], "web");
    assert!(recipes[2].get("draft_deg").is_none());
    assert_eq!(recipes[3]["ribs"]["count"], "n+2");
    assert_eq!(recipes[3]["hole_depth"], 4.);
    assert_eq!(recipes[4]["return_deg"], 80.);
    assert_eq!(recipes[4]["root_fillet"], 0.5);
    assert_eq!(recipes[5]["loop"], "inner");
    assert_eq!(recipes[6]["clearance"], 0.15);
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

#[test]
fn jsonnet_drawing_helper_preserves_sheet_and_model_references() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"
local f=import 'faber.libsonnet';
{parts:{block:f.part('Block',[f.box('base',[30,20,10])],'base')},
 drawings:{front:f.drawing('Block',views={front:{kind:'base',direction:'front',parts:['block'],at:[80,100]}},
 dimensions={width:{kind:'linear',view:'front',offset:8,along:'horizontal',
   a:{kind:'vertex',part:'block',near:[0,0,0]},b:{kind:'vertex',part:'block',near:[30,0,0]}}})}}
"#,
    );
    source().build(dir.path()).unwrap();
    let d: serde_json::Value =
        serde_json::from_slice(&read_file(dir.path(), "drawings/front.json")).unwrap();
    assert_eq!(d["title"], "Block");
    assert_eq!(d["dimensions"]["width"]["b"]["part"], "block");
    let manual = dir.path().join("drawings/manual.json");
    std::fs::copy(dir.path().join("drawings/front.json"), &manual).unwrap();
    write(dir.path(), "design.jsonnet", "{parts:{}} ");
    source().build(dir.path()).unwrap();
    let after = faber_files::read(dir.path()).unwrap();
    assert_eq!(after.drawings.keys().collect::<Vec<_>>(), ["manual"]);
    assert!(manual.is_file());
    assert!(!dir.path().join("drawings/front.json").exists());
}

#[test]
fn generated_project_takes_configurations_from_the_source() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"
local f=import 'faber.libsonnet';
{ local width=std.parseInt(self.params.width), name:'Stand',params:{width:'20'},
  components:{base:f.component('Base'),left:f.component('Left','base')},
  configurations:{wide:f.configuration({width:'65'})},
  parts:{bar:f.part('Bar',[f.box('block',[width,5,8])],'block',component='base'),
         bar2:f.instance('Left copy','bar',component='left')} }
"#,
    );
    source().build(dir.path()).unwrap();
    let mut data = faber_files::read(dir.path()).unwrap();
    let saved = serde_json::to_value(&data).unwrap();
    assert_eq!(saved["components"]["left"]["parent"], "base");
    assert_eq!(
        saved["parts"]["bar2"]["geometry"],
        serde_json::json!({"op":"instance","of":"bar"})
    );
    let mut json = saved;
    json["active_configuration"] = serde_json::json!("wide");
    data = serde_json::from_value(json).unwrap();
    let built =
        serde_json::to_value(faber_build::evaluate(dir.path(), &data).unwrap().data).unwrap();
    assert_eq!(built["params"]["width"], "65");
    assert_eq!(
        built["parts"]["bar"]["geometry"]["features"][0]["geometry"]["size"][0],
        65.
    );
}

#[test]
fn structured_assembly_example_keeps_the_supports_linked_at_both_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let text = include_str!("../../../examples/structured-assembly/design.jsonnet");
    write(dir.path(), "design.jsonnet", text);
    source().build(dir.path()).unwrap();
    let initial = faber_files::read(dir.path()).unwrap();
    for (name, span, height) in [("compact", 100., 35.), ("wide", 160., 50.)] {
        let data = faber_build::source_data_for(dir.path(), &initial, Some(name)).unwrap();
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(json["parts"]["other_leg"]["geometry"]["of"], "leg");
        assert_eq!(
            json["components"]["right"]["placement"]["translation"][0],
            span - 10.
        );
        assert_eq!(
            json["components"]["top"]["placement"]["translation"][2],
            height
        );
        let bodies = faber_core::build(&data, &OcctKernel).unwrap();
        let right = bodies.iter().find(|b| b.id == "other_leg").unwrap();
        let deck = bodies.iter().find(|b| b.id == "deck").unwrap();
        assert!(
            (right.bounds[1][0] - span).abs() < 1e-5 && (deck.bounds[0][2] - height).abs() < 1e-5
        );
    }
}
#[test]
fn face_helpers_keep_selectors_modes_and_split_tool_ids() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"
local f=import 'faber.libsonnet'; local face={normal:[0,0,1],tolerance:0.000001,expect:1};
{parts:{demo:f.part('Example',[
  f.box('base',[10,10,10]), f.remove_faces('remove','base',[face]), f.move_faces('move','base',[face],-2),
  f.split_face('split','base',face,{kind:'plane',plane:'mid'}),
  f.emboss('label','base',face,'letters',0.6,'engrave',at=[2,3])],'base')}}"#,
    );
    let baseline: faber_model::ProjectData = serde_json::from_value(
        serde_json::json!({"schema":1,"revision":1,"name":"Example","parts":{}}),
    )
    .unwrap();
    let built = faber_build::evaluate(dir.path(), &baseline).unwrap();
    let parts = serde_json::to_value(&built.data.parts["demo"].geometry).unwrap();
    let g = |i: usize| parts["features"][i]["geometry"].clone();
    assert_eq!(g(1)["faces"][0]["normal"], serde_json::json!([0., 0., 1.]));
    assert_eq!(g(2)["distance"], -2.);
    assert_eq!(
        g(3)["tool"],
        serde_json::json!({"kind":"plane","plane":"mid"})
    );
    assert_eq!(g(4)["mode"], "engrave");
    assert_eq!(g(4)["at"], serde_json::json!([2., 3.]));
}
#[test]
fn label_bottle_example_wraps_the_lettering_on_one_hollow_solid() {
    use faber_ports::Kernel;
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        include_str!("../../../examples/label-bottle/design.jsonnet"),
    );
    source().build(dir.path()).unwrap();
    let data = faber_files::read(dir.path()).unwrap();
    let labelled = faber_core::build(&data, &OcctKernel).unwrap().remove(0);
    assert_eq!(
        OcctKernel
            .properties(labelled.shape.as_ref().unwrap())
            .unwrap()
            .solids,
        1
    );
    let mut plain = data.clone();
    let Geometry::History { features, output } =
        &mut plain.parts.get_mut("bottle").unwrap().geometry
    else {
        panic!("Bottle needs a feature history");
    };
    assert!(matches!(
        features.last().map(|f| &f.geometry),
        Some(Geometry::Emboss { .. })
    ));
    features.pop();
    *output = "bottle_wall".into();
    let wall = faber_core::build(&plain, &OcctKernel).unwrap().remove(0);
    assert!(labelled.volume - wall.volume > 10.);
    let faces = OcctKernel.faces(labelled.shape.as_ref().unwrap()).unwrap();
    assert!(
        faces
            .iter()
            .filter_map(|f| f.cylinder.as_ref())
            .any(|c| c.internal && (c.radius - 25.).abs() < 1e-5)
    );
}

#[test]
fn solved_and_offset_sketch_helpers_keep_the_expression_and_references() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"local f=import 'faber.libsonnet';
{parts:{plate:f.part('Plate',[f.solved_sketch('outline','plane',{a:[0,0], b:[10,0]}, {l:{kind:'line',a:'a',b:'b'}}, [{kind:'length',line:'l',value:'width'}],['l']),f.offset_sketch('inset','outline',-2)],'inset')}}"#,
    );
    let baseline = serde_json::from_value(
        serde_json::json!({"schema":1,"revision":1,"name":"Example","parts":{}}),
    )
    .unwrap();
    let built = faber_build::evaluate(dir.path(), &baseline).unwrap();
    let g = serde_json::to_value(&built.data.parts["plate"].geometry).unwrap();
    assert_eq!(
        g["features"][0]["geometry"]["sketch"]["constraints"][0]["value"],
        "width"
    );
    assert_eq!(
        g["features"][0]["geometry"]["sketch"]["construction"][0],
        "l"
    );
    assert_eq!(g["features"][1]["geometry"]["distance"], -2.);
}

#[test]
fn the_constrained_plate_rebuilds_its_step_when_its_width_changes() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        include_str!("../../../examples/constrained-plate/design.jsonnet"),
    );
    source().build(dir.path()).unwrap();
    let mut data = faber_files::read(dir.path()).unwrap();
    let original = faber_core::build(&data, &OcctKernel).unwrap().remove(0);
    assert!((original.bounds[1][0] - original.bounds[0][0] - 60.).abs() < 1e-5);
    data.params.insert("plate_width".into(), "70".into());
    let wider = faber_core::build(&data, &OcctKernel).unwrap().remove(0);
    assert!((wider.bounds[1][0] - wider.bounds[0][0] - 70.).abs() < 1e-5);
    assert!((wider.volume - original.volume - (36. * 4. + 32. * 2.) * 10.).abs() < 1e-4);
}

#[test]
fn source_study_helpers_write_frequency_heat_and_shape_cases() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"local f=import 'faber.libsonnet';
local face={normal:[1,0,0],tolerance:0.001,expect:1};
{parts:{bar:f.part('Beam',[f.box('beam',[20,4,4])],'beam',material='steel-s235')},studies:{
 modes:f.modal_study('bar',[],modes=3),
 heat:f.thermal_study('bar',[f.temperature(face,-5),f.power(face,3),f.convection(face,25,20)]),
 shape:f.shape_study('bar',[f.fixed(face)],[f.force(face,[0,0,-10])],0.55,keep=[face])
}}"#,
    );
    source().ensure_built(dir.path()).unwrap();
    let data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("analysis.json")).unwrap()).unwrap();
    let rows = &data["studies"];
    assert_eq!(rows["modes"]["kind"], "modal");
    assert_eq!(rows["modes"]["modes"], 3);
    assert_eq!(rows["heat"]["conditions"][0]["celsius"], -5.0);
    assert_eq!(rows["heat"]["conditions"][2]["w_m2k"], 25.0);
    assert_eq!(rows["shape"]["target_volume"], 0.55);
    assert_eq!(rows["shape"]["keep"][0]["expect"], 1);
}

#[test]
fn sheet_helpers_keep_driven_rules_edges_and_holes() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"local f=import 'faber.libsonnet';
local base={outer:{kind:'rectangle',origin:[0,0],size:[50,40]}};
local hole={kind:'circle',centre:[10,12],radius:2};
{params:{stock:'2'},parts:{bracket:f.part('Bracket',[
    f.sheet('fold',{thickness:'stock',bend_radius:2},base,[
        f.flange('lip','base',2,30,angle_deg=-90,inset_start=1,holes=[hole])])
],'fold')}}"#,
    );
    let baseline = serde_json::from_value(
        serde_json::json!({"schema":1,"revision":1,"name":"Sheet","parts":{}}),
    )
    .unwrap();
    let built = faber_build::evaluate(dir.path(), &baseline).unwrap();
    let g = serde_json::to_value(&built.data.parts["bracket"].geometry).unwrap();
    let recipe = &g["features"][0]["geometry"];
    assert_eq!(recipe["rule"]["thickness"], "stock");
    assert_eq!(recipe["flanges"][0]["edge"], 2);
    assert_eq!(recipe["flanges"][0]["angle_deg"], -90.);
    assert_eq!(recipe["flanges"][0]["holes"][0]["radius"], 2.);
}

#[test]
fn surface_helpers_preserve_the_order_of_references_and_driven_values() {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "design.jsonnet",
        r#"local f=import 'faber.libsonnet';
{parts:{body:f.part('Surface recipes',[
 f.box('base',[10,10,10]),
 f.surface_extrude('side','rim','height'),
 f.surface_revolve('cone','rim','axis',180),
 f.surface_loft('loft',['rim','cap'],ruled=true),
 f.surface_sweep('sweep','rim','path'),
 f.patch('end',[{kind:'curve',feature:'rim'}],tangent_to=['loft']),
 f.trim('inside','loft','end',[1,2,3]),
 f.extend('more','inside',{near_centre:[0,0,0],tolerance:0.1,expect:1},3),
 f.stitch('joined',['side','end'],as_solid=false),
 f.split_body('piece','base','end',keep='positive'),
 f.replace_face('top','piece',{normal:[0,0,1],tolerance:0.001,expect:1},'end')
],'base')},params:{height:'8'}}"#,
    );
    let baseline = serde_json::from_value(
        serde_json::json!({"schema":1,"revision":0,"name":"Test","parts":{}}),
    )
    .unwrap();
    let d = faber_build::evaluate(dir.path(), &baseline).unwrap();
    let g = serde_json::to_value(&d.data.parts["body"].geometry).unwrap();
    let f = g["features"].as_array().unwrap();
    assert_eq!(f.len(), 11);
    assert_eq!(f[1]["geometry"]["distance"], "height");
    assert_eq!(
        f[3]["geometry"]["sections"],
        serde_json::json!(["rim", "cap"])
    );
    assert_eq!(
        f[6]["geometry"]["keep"]["near"],
        serde_json::json!([1., 2., 3.])
    );
}
