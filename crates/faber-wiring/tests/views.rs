use faber_wiring::{
    read_snapshot,
    views::{self, Picture},
};
use std::path::Path;
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assembly-motion")
}
#[test]
fn render_with_a_view_hides_its_parts_and_reports_a_removed_part() {
    let mut project = read_snapshot(&root()).unwrap();
    let saved=serde_json::from_value(serde_json::json!({"camera":{"eye":[80,-90,100],"target":[20,0,0],"up":[0,0,1],"ortho_height":90},"hidden":["rod","lid"],"style":"studio","explode":0.4,"joints":{"input":10}})).unwrap();
    project.data.views.insert("bench".into(), saved);
    let opts = Picture {
        view: Some("bench".into()),
        ..Default::default()
    };
    let frame = views::prepare(&project, &opts).unwrap();
    assert!(frame.bodies.iter().all(|b| b.id != "rod"));
    assert_eq!(frame.warnings, ["View bench: part lid was not found."]);
    assert_eq!(frame.camera.ortho_height, Some(90.));
    assert_eq!(frame.bodies.len() + 1, project.bodies.len());
    assert_ne!(
        frame
            .bodies
            .iter()
            .find(|b| b.id == "wheel")
            .unwrap()
            .bounds,
        project
            .bodies
            .iter()
            .find(|b| b.id == "wheel")
            .unwrap()
            .bounds
    );
    assert!(
        views::prepare(
            &project,
            &Picture {
                view: Some("missing".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn a_cycle_in_saved_poses_is_reported() {
    let mut project = read_snapshot(&root()).unwrap();
    let saved=serde_json::from_value(serde_json::json!({"camera":{"eye":[0,0,10],"target":[0,0,0],"up":[0,1,0],"fov_deg":38},"pose":"loop"})).unwrap();
    project.data.views.insert("loop".into(), saved);
    assert!(
        views::pose(&project.data, "loop")
            .unwrap_err()
            .to_lowercase()
            .contains("cycle")
    );
}

#[test]
fn a_view_edit_checks_the_final_joint_pose_before_saving() {
    use faber_model::{Edit, motion::PoseValue};
    let tmp = tempfile::tempdir().unwrap();
    faber_files::layout::write(tmp.path(), &faber_files::read(&root()).unwrap()).unwrap();
    let mut project = faber_wiring::open(tmp.path(), false, None).unwrap();
    let template = project.data.views["front_detail"].clone();
    let set = |name: &str, parent: Option<&str>, joint: Option<(&str, f64)>| {
        let mut view = template.clone();
        view.pose = parent.map(str::to_owned);
        view.joints = joint
            .into_iter()
            .map(|(k, v)| (k.into(), PoseValue::Number(v)))
            .collect();
        Edit::SetView {
            name: name.into(),
            view,
        }
    };
    let remove = |name: &str| Edit::RemoveView { name: name.into() };
    // A batch can add the target of a reference after the view that uses it.
    project
        .batch(vec![
            set("detail", Some("base_pose"), None),
            set("base_pose", None, Some(("input", 12.))),
        ])
        .unwrap();
    assert_eq!(
        views::pose(&project.data, "detail").unwrap()["input"],
        PoseValue::Number(12.)
    );
    let before = project.data.clone();
    let files = || {
        ["analysis.json", "faber.json"].map(|name| std::fs::read(tmp.path().join(name)).unwrap())
    };
    let bytes = files();
    for (edits, message) in [
        (vec![set("cycle", Some("cycle"), None)], "Cycle"),
        (vec![set("base_pose", Some("detail"), None)], "Cycle"),
        (
            vec![set("detail", None, Some(("unknown_joint", 12.)))],
            "unknown_joint",
        ),
        (vec![set("detail", None, Some(("input", 1000.)))], "limits"),
        (vec![remove("base_pose")], "base_pose"),
    ] {
        let err = project.batch(edits).unwrap_err();
        assert!(err.contains(message), "{err}");
        assert_eq!(before, project.data);
        assert_eq!(bytes, files());
    }
    project
        .batch(vec![remove("base_pose"), set("detail", None, None)])
        .unwrap();
    assert!(views::pose(&project.data, "detail").unwrap().is_empty());
}

#[test]
fn a_portrait_auto_fit_widens_the_distance_and_keeps_named_framing() {
    let p = read_snapshot(&root()).unwrap();
    let auto = views::prepare(&p, &Default::default()).unwrap();
    let saved = views::prepare(
        &p,
        &Picture {
            view: Some("front_detail".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_ne!(auto.camera, auto.camera_at_size(64, 400));
    assert_eq!(auto.camera, auto.camera_at_size(400, 64));
    assert_eq!(saved.camera, saved.camera_at_size(64, 400));
}

#[test]
fn a_removed_section_in_a_bookmark_leaves_the_model_visible() {
    let mut p = read_snapshot(&root()).unwrap();
    p.data.views.get_mut("front_detail").unwrap().section = Some("old_cut".into());
    let view = views::prepare(
        &p,
        &Picture {
            view: Some("front_detail".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        view.warnings,
        ["View front_detail: section old_cut was not found."]
    );
    assert_eq!(view.bodies.len(), p.bodies.len());
    assert!(
        views::prepare(
            &p,
            &Picture {
                cut: Some(views::Cut::Saved("old_cut".into())),
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn a_scoped_camera_cut_keeps_the_other_parts_whole() {
    let mut project = read_snapshot(&root()).unwrap();
    let section = serde_json::from_value(serde_json::json!({
        "plane":{"kind":"plane","origin":[0,0,1],"normal":[0,0,1]},"parts":["slide"]
    }))
    .unwrap();
    project.data.sections.insert("slide_cut".into(), section);
    let frame = views::prepare(
        &project,
        &Picture {
            cut: Some(views::Cut::Saved("slide_cut".into())),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(frame.bodies.len(), project.bodies.len());
    for body in frame.bodies {
        if body.id == "slide" {
            assert!((body.bounds[1][2] - 1.).abs() < 1e-9);
        } else {
            assert_eq!(
                body.bounds,
                project
                    .bodies
                    .iter()
                    .find(|b| b.id == body.id)
                    .unwrap()
                    .bounds
            );
        }
    }
}
