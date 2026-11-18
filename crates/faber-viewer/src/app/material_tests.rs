use super::controls_fixture::two_blocks;
use super::*;
use egui_kittest::kittest::Queryable;

fn choose(h: &mut egui_kittest::Harness<'_, Shell>, label: &str) {
    h.get_by_role_and_label(egui::Role::ComboBox, "Material")
        .click();
    for _ in 0..4 {
        h.step();
    }
    h.get_by_label(label).scroll_to_me();
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label(label).click();
}

#[test]
fn select_material_changes_one_part_and_shows_its_mass() {
    let (dir, mut h) = two_blocks();
    let initial = h.state().data.revision;
    assert!(
        h.query_by_role_and_label(egui::Role::ComboBox, "Material")
            .is_none()
    );
    h.state_mut().select_only(Some("right".into()));
    h.step();
    assert!(h.query_by_label("Mass").is_none());
    choose(&mut h, "Steel S235JR");
    for _ in 0..120 {
        h.step();
        if h.state().data.revision != initial {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let data = faber_wiring::read(dir.path()).unwrap().0;
    let parts = serde_json::to_value(data.parts).unwrap();
    assert_eq!(data.revision, initial + 1);
    assert_eq!(parts["right"]["material"], "steel-s235");
    assert!(parts["left"].get("material").is_none());
    h.step();
    h.get_by_label("26.49 g");
    choose(&mut h, "Unassigned");
    for _ in 0..120 {
        h.step();
        if h.state().data.revision == initial + 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(h.state().data.revision, initial + 2);
    h.step();
    assert!(h.query_by_label("Mass").is_none());
    let saved = faber_wiring::read(dir.path()).unwrap().0;
    assert!(
        serde_json::to_value(&saved.parts["right"])
            .unwrap()
            .get("material")
            .is_none()
    );
}
