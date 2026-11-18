use super::controls_fixture::two_blocks;
use super::*;
use egui::{Key, Modifiers};
use egui_kittest::{Harness, kittest::Queryable};

fn settle(h: &mut Harness<'_, Shell>) {
    h.step();
    let until = Instant::now() + Duration::from_secs(10);
    while h.state().pending.is_some() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    assert!(h.state().pending.is_none());
    assert_eq!(h.state().error, None);
    h.step();
}

#[test]
fn ctrl_z_and_ctrl_y_undo_and_redo() {
    let (_dir, mut h) = two_blocks();
    let data = h.state().data.clone();
    h.state_mut().request_window(Draft::Parameters(
        data,
        BTreeMap::from([("width".into(), "6".into())]),
    ));
    h.step();
    h.step();
    h.get_by_label("Apply parameters").click();
    settle(&mut h);
    for (mods, key, present) in [
        (Modifiers::CTRL, Key::Z, false),
        (Modifiers::CTRL, Key::Y, true),
        (Modifiers::CTRL, Key::Z, false),
        (Modifiers::CTRL | Modifiers::SHIFT, Key::Z, true),
    ] {
        h.key_press_modifiers(mods, key);
        settle(&mut h);
        assert_eq!(
            h.state().data.params.get("width").map(String::as_str),
            present.then_some("6")
        );
    }
}

#[test]
fn ctrl_z_undoes_a_group_move_in_one_step() {
    let (_dir, mut h) = two_blocks();
    let original = h.state().data.parts.clone();
    h.state_mut().select_visible();
    h.key_press(Key::M);
    h.step();
    h.step();
    h.get_by_label("X distance").focus();
    h.step();
    h.get_by_label("X distance").type_text("10");
    h.step();
    h.step();
    assert!(h.get_by_label("X distance").value().unwrap().contains("10"));
    h.key_press(Key::Enter);
    settle(&mut h);
    for (id, part) in &original {
        assert_ne!(h.state().data.parts[id].placement, part.placement);
    }
    h.key_press_modifiers(Modifiers::CTRL, Key::Z);
    settle(&mut h);
    assert_eq!(h.state().data.parts, original);
}

#[test]
fn delete_removes_the_selection_in_one_revision() {
    let (_dir, mut h) = two_blocks();
    let revision = h.state().data.revision;
    h.state_mut().select_visible();
    h.key_press(Key::Delete);
    settle(&mut h);
    assert!(h.state().data.parts.is_empty());
    assert_eq!(h.state().data.revision, revision + 1);
    h.get_by_label("Deleted left, right. Ctrl+Z restores it.");
}

#[test]
fn delete_in_a_text_field_keeps_the_part() {
    let (_dir, mut h) = two_blocks();
    h.state_mut().select_visible();
    let data = h.state().data.clone();
    h.state_mut().request_window(Draft::Parameters(
        data,
        BTreeMap::from([("width".into(), "12".into())]),
    ));
    h.step();
    h.step();
    h.get_by_label("width").focus();
    h.step();
    h.key_press(Key::Delete);
    settle(&mut h);
    assert_eq!(h.state().data.parts.len(), 2);
}

#[test]
fn shortcuts_do_not_fire_under_a_tool_window() {
    let (_dir, mut h) = two_blocks();
    h.state_mut().select_visible();
    h.state_mut().new_feature("left", ToolKind::Shell).unwrap();
    h.step();
    h.state_mut().camera.distance = 400.;
    for key in [Key::F, Key::M, Key::S] {
        h.key_press(key);
        h.step();
    }
    assert_eq!(h.state().camera.distance, 400.);
    assert!(h.state().feature_draft.is_some());
    assert!(h.query_by_label("Command search").is_none());
}

#[test]
fn s_opens_the_command_search_and_enter_runs_it() {
    let (_dir, mut h) = two_blocks();
    h.get_by_label("Fit").click();
    h.step();
    let distance = h.state().camera.distance;
    h.state_mut().camera.distance *= 3.;
    h.key_press(Key::S);
    h.step();
    h.step();
    h.get_by_label("Command search").type_text("fit");
    h.step();
    h.key_press(Key::Enter);
    h.step();
    assert_eq!(h.state().camera.distance, distance);
    assert!(h.query_by_label("Command search").is_none());
}

#[test]
fn shift_s_toggles_the_section() {
    let (_dir, mut h) = two_blocks();
    for enabled in [true, false] {
        h.key_press_modifiers(Modifiers::SHIFT, Key::S);
        h.step();
        assert_eq!(h.state().section_on, enabled);
    }
}

#[test]
fn search_runs_home_and_strict() {
    let (_dir, mut h) = two_blocks();
    let ctx = h.ctx.clone();
    let distance = Camera::fit(&h.state().bodies).distance;
    h.state_mut().camera.distance = 517.;
    for name in ["Home", "Strict"] {
        let entry = command_search::search_commands(name)
            .into_iter()
            .next()
            .unwrap();
        h.state_mut().run_command(&ctx, entry.command);
    }
    assert_eq!(h.state().camera.distance, distance);
    assert!(h.state().strict_edits);
}

#[test]
fn paste_requires_a_live_source_and_target() {
    let (_dir, mut h) = two_blocks();
    let ctx = h.ctx.clone();
    let shell = h.state_mut();
    let entry = command_search::search_commands("Paste placement")[0];
    shell.select_only(Some("left".into()));
    shell.run_command(&ctx, keys::Command::CopyPlacement);
    shell.select_only(Some("right".into()));
    assert!(shell.command_enabled(entry.command));
    shell.data.parts.remove("left");
    assert!(!shell.command_enabled(entry.command));
    shell.select_only(None);
    assert!(!shell.command_enabled(entry.command));
}

#[test]
fn paste_places_the_part_in_the_copied_world_frame() {
    let (_dir, mut h) = two_blocks();
    let ctx = h.ctx.clone();
    let mut part = h.state().data.parts["right"].clone();
    part.component = Some("holder".into());
    let component = serde_json::from_value(serde_json::json!({
        "name": "Holder", "placement":{"rotation":[0,0,90],"translation":[40,30,10]}
    }))
    .unwrap();
    h.state_mut().save_sections(
        &ctx,
        vec![
            faber_model::Edit::SetComponent {
                id: "holder".into(),
                component,
            },
            faber_model::Edit::PutPart {
                id: "right".into(),
                part,
            },
        ],
    );
    settle(&mut h);
    h.state_mut().select_only(Some("left".into()));
    h.state_mut()
        .run_command(&ctx, keys::Command::CopyPlacement);
    h.state_mut().select_only(Some("right".into()));
    h.state_mut()
        .run_command(&ctx, keys::Command::PastePlacement);
    settle(&mut h);
    let data = &h.state().data;
    let source = faber_core::world_placement(data, "left").unwrap();
    let target = faber_core::world_placement(data, "right").unwrap();
    assert!(
        target.abs_diff_eq(source, 1e-8),
        "Pasted {target:?}, expected {source:?}"
    );
}
