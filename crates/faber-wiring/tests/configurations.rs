use faber_core::Project;
use faber_model::Edit;
use serde_json::{Value, json};

fn edit(value: Value) -> Edit {
    serde_json::from_value(value).unwrap()
}
fn fixture() -> (tempfile::TempDir, Project) {
    let root = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(root.path(), true, Some("Box".into())).unwrap();
    p.batch(vec![edit(json!({"tool":"set_param","name":"w","expression":"10"})),edit(json!({"tool":"set_param","name":"h","expression":"3"})),edit(json!({"tool":"put_part","id":"box","part":{"name":"Box","colour":[80,90,100],"geometry":{"op":"box","size":["w",5,"h"]}}}))]).unwrap();
    for (name, params) in [
        ("large", json!({"w":"20","h":"6"})),
        ("small", json!({"w":"4"})),
        ("invalid", json!({"w":"-2"})),
    ] {
        p.batch(vec![edit(
            json!({"tool":"set_configuration","name":name,"configuration":{"params":params}}),
        )])
        .unwrap();
    }
    (root, p)
}
#[test]
fn use_sets_two_params_in_one_revision_and_undo_restores() {
    let (root, mut p) = fixture();
    let before = p.data.clone();
    faber_wiring::use_configuration(root.path(), &mut p, "large", false).unwrap();
    assert_eq!(p.data.revision, before.revision + 1);
    assert_eq!(p.data.active_configuration.as_deref(), Some("large"));
    assert!((p.bodies[0].volume - 600.).abs() < 1e-6);
    faber_wiring::use_configuration(root.path(), &mut p, "small", false).unwrap();
    assert_eq!(p.data.params["h"], "6");
    p.undo().unwrap();
    assert_eq!(p.data.active_configuration.as_deref(), Some("large"));
    p.undo().unwrap();
    assert_eq!(p.data.params, before.params);
    assert_eq!(p.data.active_configuration, None);
}
#[test]
fn unknown_param_and_invalid_geometry_leave_the_active_configuration_intact() {
    let (root, mut p) = fixture();
    faber_wiring::use_configuration(root.path(), &mut p, "large", false).unwrap();
    let before = p.data.clone();
    let unknown = edit(
        json!({"tool":"set_configuration","name":"oops","configuration":{"params":{"missing":"2"}}}),
    );
    assert!(p.batch(vec![unknown]).is_err());
    assert!(faber_wiring::use_configuration(root.path(), &mut p, "invalid", false).is_err());
    assert_eq!(p.data, before);
    assert_eq!(faber_files::read(root.path()).unwrap(), before);
}
#[test]
fn configuration_can_suppress_a_feature_and_switch_it_back() {
    let (root, mut p) = fixture();
    p.batch(vec![edit(json!({"tool":"put_part","id":"detail","part":{"name":"Detail","colour":[0,0,0],"geometry":{"op":"history","output":"base","features":[{"id":"base","geometry":{"op":"box","size":[3,3,3]}},{"id":"spare","geometry":{"op":"box","size":[1,1,1]}}]}}})),edit(json!({"tool":"set_configuration","name":"plain","configuration":{"suppressed":[{"part":"detail","feature":"spare"}]}}))]).unwrap();
    for (name, expected) in [("plain", true), ("large", false)] {
        faber_wiring::use_configuration(root.path(), &mut p, name, false).unwrap();
        let faber_model::Geometry::History { features, .. } = &p.data.parts["detail"].geometry
        else {
            panic!()
        };
        assert_eq!(features[1].suppressed, expected);
    }
}
#[test]
fn all_configurations_export_one_folder_each_without_changing_the_project() {
    let (root, mut p) = fixture();
    p.batch(vec![edit(
        json!({"tool":"remove_configuration","name":"invalid"}),
    )])
    .unwrap();
    let before = p.data.clone();
    let dest = tempfile::tempdir().unwrap();
    let options = faber_wiring::configurations::ExportOptions {
        format: "stl",
        force: false,
        tolerance: 0.1,
    };
    let files = faber_wiring::configurations::export(
        root.path(),
        &p.data,
        dest.path(),
        None,
        true,
        &options,
    )
    .unwrap();
    assert_eq!(files.len(), 2);
    assert!(dest.path().join("small/box.stl").is_file());
    assert!(dest.path().join("large/box.stl").is_file());
    assert_eq!(faber_files::read(root.path()).unwrap(), before);
}

#[test]
fn invalid_last_configuration_preserves_existing_export_files() {
    let (root, mut p) = fixture();
    for (name, w) in [("a_valid", "12"), ("z_invalid", "-1")] {
        p.batch(vec![edit(
            json!({"tool":"set_configuration","name":name,"configuration":{"params":{"w":w}}}),
        )])
        .unwrap();
    }
    let dest = tempfile::tempdir().unwrap();
    std::fs::create_dir(dest.path().join("a_valid")).unwrap();
    let earlier = dest.path().join("a_valid/box.stl");
    std::fs::write(&earlier, b"previous export").unwrap();
    let options = faber_wiring::configurations::ExportOptions {
        format: "stl",
        force: false,
        tolerance: 0.1,
    };
    assert!(
        faber_wiring::configurations::export(
            root.path(),
            &p.data,
            dest.path(),
            None,
            true,
            &options
        )
        .is_err()
    );
    assert_eq!(std::fs::read(earlier).unwrap(), b"previous export");
}

#[test]
fn blocked_last_destination_preserves_existing_export_files() {
    let (root, mut p) = fixture();
    p.batch(vec![edit(
        json!({"tool":"remove_configuration","name":"invalid"}),
    )])
    .unwrap();
    let dest = tempfile::tempdir().unwrap();
    std::fs::create_dir(dest.path().join("large")).unwrap();
    let previous = dest.path().join("large/box.stl");
    let blocked = dest.path().join("small");
    std::fs::write(&previous, b"last accepted export").unwrap();
    std::fs::write(&blocked, b"another file").unwrap();
    let options = faber_wiring::configurations::ExportOptions {
        format: "stl",
        force: false,
        tolerance: 0.1,
    };
    assert!(
        faber_wiring::configurations::export(
            root.path(),
            &p.data,
            dest.path(),
            None,
            true,
            &options
        )
        .is_err()
    );
    assert_eq!(std::fs::read(previous).unwrap(), b"last accepted export");
    assert_eq!(std::fs::read(blocked).unwrap(), b"another file");
}

#[cfg(windows)]
#[test]
fn locked_last_mesh_restores_the_earlier_export() {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, mut p) = fixture();
    p.batch(vec![edit(
        json!({"tool":"remove_configuration","name":"invalid"}),
    )])
    .unwrap();
    let dest = tempfile::tempdir().unwrap();
    for name in ["large", "small"] {
        let folder = dest.path().join(name);
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(folder.join("box.stl"), name.as_bytes()).unwrap();
    }
    let last = dest.path().join("small/box.stl");
    // The reader permits other readers but prevents replacement.
    let reader = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&last)
        .unwrap();
    let options = faber_wiring::configurations::ExportOptions {
        format: "stl",
        force: false,
        tolerance: 0.1,
    };
    assert!(
        faber_wiring::configurations::export(
            root.path(),
            &p.data,
            dest.path(),
            None,
            true,
            &options
        )
        .is_err()
    );
    drop(reader);
    assert_eq!(
        std::fs::read(dest.path().join("large/box.stl")).unwrap(),
        b"large"
    );
    assert_eq!(std::fs::read(last).unwrap(), b"small");
}

#[test]
fn generated_history_keeps_values_inherited_from_an_earlier_configuration() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("design.jsonnet"),r#"{
        params:{w:"8",h:"2"},
        configurations:{tall:{params:{h:"9"}},wide:{params:{w:"18"}},small:{params:{w:"2",h:"1"}}},
        parts:{box:{name:"Box",colour:[2,4,8],geometry:{op:"box",size:[std.parseInt($.params.w),5,std.parseInt($.params.h)]}}}
    }"#).unwrap();
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    for name in ["tall", "wide", "small"] {
        faber_wiring::use_configuration(dir.path(), &mut p, name, false).unwrap();
    }
    faber_wiring::history_move(dir.path(), &mut p, faber_ports::HistoryMove::Undo).unwrap();
    assert_eq!(p.data.params["h"], "9");
    assert_eq!(p.data.params["w"], "18");
    assert!((p.bodies[0].volume - 810.).abs() < 1e-6);
}

#[test]
fn malformed_configuration_expressions_are_refused_before_the_definition_is_saved() {
    let (_dir, mut p) = fixture();
    let before = p.data.clone();
    for expression in ["10 +", "w + 1", "unknown_size"] {
        let result=p.batch(vec![edit(json!({"tool":"set_configuration","name":"invalid_expression","configuration":{"params":{"w":expression}}}))]);
        assert!(result.is_err(), "Accepted expression {expression}");
        assert_eq!(p.data, before);
    }
}

#[test]
fn inactive_formulas_do_not_block_a_valid_configuration() {
    let (root, mut project) = fixture();
    let original = project.data.params.clone();
    for (name, parameter, expression) in [("from_h", "w", "h"), ("from_w", "h", "w")] {
        project
            .batch(vec![edit(json!({
                "tool":"set_configuration","name":name,
                "configuration":{"params":{(parameter):expression}}
            }))])
            .unwrap();
    }
    for (name, value) in [("from_h", 3.), ("from_w", 10.)] {
        faber_wiring::use_configuration(root.path(), &mut project, name, false).unwrap();
        assert!((project.bodies[0].volume - 5. * value * value).abs() < 1e-6);
        let saved = project.data.clone();
        let other = if name == "from_h" { "from_w" } else { "from_h" };
        let error =
            faber_wiring::use_configuration(root.path(), &mut project, other, false).unwrap_err();
        assert!(error.contains("cycle"), "{error}");
        assert_eq!(faber_files::read(root.path()).unwrap(), saved);
        project.undo().unwrap();
        assert_eq!(project.data.params, original);
    }
}
