use super::controls_fixture::two_blocks;
use super::*;
use egui_kittest::kittest::{NodeT, Queryable};

#[test]
fn number_fields_show_units() {
    let (_dir, mut h) = two_blocks();
    h.state_mut().new_feature("left", ToolKind::Fillet).unwrap();
    for _ in 0..3 {
        h.step();
    }
    assert!(h.query_all_by_label("mm").count() >= 1);
}

#[test]
fn tools_need_a_selection() {
    let (_dir, h) = two_blocks();
    assert!(h.get_by_label("Fillet").accesskit_node().is_disabled());
}

#[test]
fn strict_mode_shows_in_the_header() {
    let (_dir, mut h) = two_blocks();
    h.state_mut().strict_edits = true;
    h.step();
    h.get_by_label("Strict").click();
    h.step();
    assert!(!h.state().strict_edits);
}

#[test]
fn parameters_can_be_added_in_an_empty_project() {
    let dir = tempfile::tempdir().unwrap();
    let p = faber_wiring::open(dir.path(), true, Some("Parameters".into())).unwrap();
    let root = dir.path().to_owned();
    let data = p.data.clone();
    let bodies = p.bodies.clone();
    drop(p);
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440., 1100.))
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label("Edit parameters").click();
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label("Parameter name").focus();
    h.step();
    h.get_by_label("Parameter name").type_text("w");
    h.step();
    h.get_by_label("Expression").focus();
    h.step();
    h.get_by_label("Expression").type_text("40");
    h.step();
    assert_eq!(h.state().new_parameter, ["w", "40"]);
    h.get_by_label("Add parameter").click();
    for _ in 0..3 {
        h.step();
    }
    assert_eq!(
        h.state()
            .parameter_draft
            .as_ref()
            .unwrap()
            .1
            .get("w")
            .map(String::as_str),
        Some("40"),
        "{:?}",
        h.state().window.error
    );
    h.get_by_label("Apply parameters").click();
    for _ in 0..80 {
        h.step();
        if h.state().parameter_draft.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        h.state().parameter_draft.is_none(),
        "{:?}",
        h.state().window.error
    );
    assert_eq!(faber_wiring::read(dir.path()).unwrap().0.params["w"], "40");
}

#[test]
fn used_parameter_cannot_be_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Expression".into())).unwrap();
    p.set_params([("w".into(), "20".into())].into()).unwrap();
    let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({
        "name":"Bracket","colour":[120,130,140],"geometry":{"op":"history","output":"base",
        "features":[{"id":"base","geometry":{"op":"box","size":["2*w",5,6]}}]}
    }))
    .unwrap();
    p.put_part("bracket".into(), part).unwrap();
    let expected = p.data.clone();
    drop(p);
    let result = crate::edit::parameters(dir.path(), &expected, BTreeMap::new(), false);
    assert_eq!(
        result.err().as_deref(),
        Some("Parameter w is used by bracket/base.")
    );
    assert_eq!(faber_wiring::read(dir.path()).unwrap().0, expected);
}

#[test]
fn delete_an_unused_parameter_is_one_undo_step() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Expression".into())).unwrap();
    p.set_params([("w".into(), "20".into())].into()).unwrap();
    let expected = p.data.clone();
    drop(p);
    let (data, _) = crate::edit::parameters(dir.path(), &expected, BTreeMap::new(), false).unwrap();
    assert!(data.params.is_empty());
    assert_eq!(data.revision, expected.revision + 1);
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    p.undo().unwrap();
    assert_eq!(p.data.params, expected.params);
}

#[test]
fn open_dialog_does_not_block_the_frame() {
    use std::task::Poll;
    let mut frames = 0;
    let mut dialog = super::dialogs::PendingDialog::new(std::future::poll_fn(move |cx| {
        frames += 1;
        if frames < 3 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(super::dialogs::Choice::Folder(Some(PathBuf::from(
                "chosen",
            ))))
        }
    }));
    let ctx = egui::Context::default();
    assert!(dialog.poll(&ctx).is_none());
    assert!(dialog.poll(&ctx).is_none());
    assert!(
        matches!(dialog.poll(&ctx), Some(super::dialogs::Choice::Folder(Some(p))) if p == std::path::Path::new("chosen"))
    );
}

#[test]
fn expression_in_a_size_field_shows_its_resolved_unit() {
    let (_dir, mut h) = two_blocks();
    let mut expected = h.state().data.clone();
    expected.params.insert("width".into(), "30".into());
    let geometry =
        serde_json::from_value(serde_json::json!({"op":"box","size":["width/2",4,8]})).unwrap();
    let form =
        crate::feature_form::FeatureForm::new(expected, "left".into(), "block".into(), &geometry)
            .unwrap();
    h.state_mut().request_window(Draft::Feature(form));
    for _ in 0..4 {
        h.step();
    }
    assert!(h.query_by_label("= 15 mm").is_some());
}

#[test]
fn strict_mode_removes_an_unused_parameter_and_accepts_unchanged_values() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Strict edit".into())).unwrap();
    p.set_params([("count".into(), "6".into())].into()).unwrap();
    let expected = p.data.clone();
    drop(p);
    let (unchanged, _) =
        crate::edit::parameters(dir.path(), &expected, expected.params.clone(), true).unwrap();
    assert_eq!(unchanged, expected);
    let (removed, _) =
        crate::edit::parameters(dir.path(), &expected, BTreeMap::new(), true).unwrap();
    assert!(removed.params.is_empty());
    assert_eq!(removed.revision, expected.revision + 1);
}

#[test]
fn an_untyped_parameter_does_not_claim_a_length_unit() {
    let (_dir, mut h) = two_blocks();
    let data = h.state().data.clone();
    h.state_mut().request_window(Draft::Parameters(
        data,
        [("turn".into(), "360 / 4".into())].into(),
    ));
    for _ in 0..3 {
        h.step();
    }
    assert!(h.query_by_label("= 90").is_some());
}

#[test]
fn inspector_help_clears_the_section_controls_at_each_scroll_position() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/fastener-check");
    let (data, bodies) = faber_wiring::read(&root).unwrap();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(1440., 920.))
        .with_step_dt(1. / 60.)
        .wgpu_setup(crate::gpu_setup())
        .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
    const HELP: &str = "Drag empty space to select.";
    for size in [egui::vec2(1440., 920.), egui::vec2(860., 600.)] {
        h.set_size(size);
        for _ in 0..6 {
            h.step();
        }
        let canvas = super::controls_fixture::canvas(&h);
        for target in ["initial", "Reverse direction", HELP] {
            if target != "initial" {
                h.get_by_label_contains(target).scroll_to_me();
                for _ in 0..20 {
                    h.step();
                }
            }
            let help = h.get_by_label_contains(HELP).rect();
            let keys = h.get_by_label_contains("Shift+S  Section").rect();
            for label in [
                "Show hatching",
                "Custom cut colour",
                "Reverse direction",
                "The cut changes the view only.",
            ] {
                let control = h.get_by_label(label).rect();
                assert!(
                    help.top() >= control.bottom() && keys.top() >= control.bottom(),
                    "{size:?} after {target}: {label} {control:?}, mouse help {help:?}, keys {keys:?}"
                );
            }
            assert!(
                keys.top() >= help.bottom(),
                "Keyboard and mouse help overlap"
            );
            if target == HELP {
                assert!(
                    help.center().y > 0. && help.center().y < size.y,
                    "Cannot scroll to {help:?}"
                );
            }
            assert_eq!(
                canvas,
                super::controls_fixture::canvas(&h),
                "Scrolling the inspector resized the viewport"
            );
        }
    }
}
