use faber_core::Project;
use faber_geometry::NativeKernel;
use faber_model::{Part, ProjectData, Scalar};
use faber_ports::{HistoryMove, ProjectStore};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn recipe() -> Part<Scalar> {
    serde_json::from_value(json!({
        "name":"Cut block", "colour":[120,150,170],
        "geometry":{"op":"history","output":"cut","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,"height"]}},
            {"id":"tool","geometry":{"op":"box","size":[10,20,"height"]}},
            {"id":"moved","geometry":{"op":"transform","source":"tool","placement":{"translation":[5,0,0]}}},
            {"id":"cut","geometry":{"op":"difference","left":"base","right":"moved"}}
        ]}
    })).unwrap()
}

#[test]
fn feature_topology_uses_source_coordinates_and_updates_after_edits() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Queries".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    let mut part = recipe();
    part.placement.translation = [100., 200., 300.].map(Scalar::Number);
    project.put_part("block".into(), part).unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    let (edges, faces) = project.feature_topology("block", "base").unwrap();
    assert_eq!(edges.len(), 12);
    assert_eq!(faces.len(), 6);
    assert!(
        faces
            .iter()
            .all(|face| face.centre[0] <= 10. && face.centre[2] <= 30.)
    );
    assert!(edges.iter().any(|edge| (edge.length - 30.).abs() < 1e-8));
    let (_, placed) = project.topology("block").unwrap();
    assert!(placed.iter().all(|face| face.centre[0] >= 100.));
    assert!(project.feature_topology("block", "missing").is_err());
    assert!(project.feature_topology("missing", "base").is_err());
    assert_eq!(
        faber_wiring::project_fingerprint(directory.path()).unwrap(),
        before
    );
    project
        .set_params([("height".into(), "40".into())].into())
        .unwrap();
    let (edges, _) = project.feature_topology("block", "base").unwrap();
    assert!(edges.iter().any(|edge| (edge.length - 40.).abs() < 1e-8));
    assert!(!edges.iter().any(|edge| (edge.length - 30.).abs() < 1e-8));
    drop(project);
    let mut reopened = faber_wiring::open(directory.path(), false, None).unwrap();
    assert_eq!(
        reopened.feature_topology("block", "base").unwrap().0.len(),
        12
    );
    let mut plain = recipe();
    plain.geometry = faber_model::Geometry::Box {
        size: [10., 20., 30.].map(Scalar::Number),
    };
    reopened.put_part("block".into(), plain).unwrap();
    assert!(reopened.feature_topology("block", "base").is_err());
}

#[test]
fn ordered_features_rebuild_after_parameter_edits_and_survive_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("History".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project.put_part("block".into(), recipe()).unwrap();
    assert!((project.bodies[0].volume - 3000.).abs() < 1e-6);
    project
        .set_params([("height".into(), "40".into())].into())
        .unwrap();
    assert!((project.bodies[0].volume - 4000.).abs() < 1e-6);
    drop(project);
    let reopened = faber_wiring::open(directory.path(), false, None).unwrap();
    assert!((reopened.bodies[0].volume - 4000.).abs() < 1e-6);
}

#[test]
fn invalid_feature_references_and_duplicate_ids_do_not_change_saved_project() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("History".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project.put_part("block".into(), recipe()).unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    for mode in 0..3 {
        let mut bad = serde_json::to_value(recipe()).unwrap();
        match mode {
            0 => bad["geometry"]["features"][0]["id"] = json!("tool"),
            1 => bad["geometry"]["features"][2]["geometry"]["source"] = json!("cut"),
            _ => bad["geometry"]["output"] = json!("absent"),
        }
        let bad: Part<Scalar> = serde_json::from_value(bad).unwrap();
        assert!(project.put_part("block".into(), bad).is_err());
        assert_eq!(
            before,
            faber_wiring::project_fingerprint(directory.path()).unwrap()
        );
    }
}

#[test]
fn feature_edits_preserve_order_and_refuse_broken_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Edit".into())).unwrap();
    project
        .set_params([("height".into(), "30".into())].into())
        .unwrap();
    project.put_part("block".into(), recipe()).unwrap();
    let changed = serde_json::from_value(
        json!({"op":"transform","source":"tool","placement":{"translation":[2,0,0]}}),
    )
    .unwrap();
    project.edit_feature("block", "moved", changed).unwrap();
    assert!((project.bodies[0].volume - 1200.).abs() < 1e-6);
    let unused = serde_json::from_value(
        json!({"id":"spare","geometry":{"op":"cylinder","radius":2,"height":10}}),
    )
    .unwrap();
    project
        .add_feature("block", unused, Some("base"), false)
        .unwrap();
    project.move_feature("block", "spare", None).unwrap();
    project.delete_feature("block", "spare").unwrap();
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    assert!(project.delete_feature("block", "tool").is_err());
    assert!(project.move_feature("block", "cut", Some("base")).is_err());
    assert!(project.delete_feature("block", "missing").is_err());
    assert_eq!(
        before,
        faber_wiring::project_fingerprint(directory.path()).unwrap()
    );
    let reopened = faber_wiring::read(directory.path()).unwrap();
    assert!((reopened.1[0].volume - 1200.).abs() < 1e-6);
}

/// A store with explicit undo and redo stacks.
struct StepStore {
    current: Mutex<ProjectData>,
    undo: Mutex<Vec<ProjectData>>,
    redo: Mutex<Vec<ProjectData>>,
}

impl ProjectStore for StepStore {
    fn load(&self) -> Result<ProjectData, String> {
        Ok(self.current.lock().unwrap().clone())
    }
    fn save(&self, _: Option<u64>, data: &ProjectData) -> Result<(), String> {
        *self.current.lock().unwrap() = data.clone();
        Ok(())
    }
    fn history_target(&self, action: HistoryMove) -> Result<ProjectData, String> {
        let stack = match action {
            HistoryMove::Undo => &self.undo,
            HistoryMove::Redo => &self.redo,
        };
        stack
            .lock()
            .unwrap()
            .last()
            .cloned()
            .ok_or_else(|| "No history step is available".to_string())
    }
    fn save_history(
        &self,
        expected: &ProjectData,
        data: &ProjectData,
        action: HistoryMove,
    ) -> Result<(), String> {
        let (from, to) = match action {
            HistoryMove::Undo => (&self.undo, &self.redo),
            HistoryMove::Redo => (&self.redo, &self.undo),
        };
        from.lock()
            .unwrap()
            .pop()
            .ok_or_else(|| "No history step is available".to_string())?;
        to.lock().unwrap().push(expected.clone());
        *self.current.lock().unwrap() = data.clone();
        Ok(())
    }
}

#[test]
fn undo_across_a_revision_only_step_moves_the_history() {
    // Histories written before the no-op guard can hold a step that differs only by revision.
    let current = ProjectData {
        studies: Default::default(),
        materials: Default::default(),
        schema: 1,
        revision: 2,
        name: "History".into(),
        parts: Default::default(),
        params: Default::default(),
        contacts: Default::default(),
        motion: Default::default(),
        sections: Default::default(),
    };
    let older = ProjectData {
        revision: 1,
        ..current.clone()
    };
    let store = Arc::new(StepStore {
        current: Mutex::new(current),
        undo: Mutex::new(vec![older]),
        redo: Mutex::new(vec![]),
    });
    let mut project = Project::open(store.clone(), Arc::new(NativeKernel)).unwrap();
    project.undo().unwrap();
    assert!(store.undo.lock().unwrap().is_empty());
    assert_eq!(store.redo.lock().unwrap().len(), 1);
    assert!(project.undo().is_err());
}
