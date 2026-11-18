use faber_core::Project;
use faber_geometry::NativeKernel;
use faber_model::{Geometry, Part, Placement, Profile, ProjectData};
use faber_ports::ProjectStore;
use std::sync::{Arc, Mutex};

struct Store(Mutex<ProjectData>);

struct CountingKernel(std::sync::atomic::AtomicUsize);
impl faber_ports::Kernel for CountingKernel {
    fn build(&self, geometry: &Geometry) -> Result<faber_ports::Mesh, String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        faber_ports::Kernel::build(&NativeKernel, geometry)
    }
}
impl ProjectStore for Store {
    fn load(&self) -> Result<ProjectData, String> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn save(&self, _: Option<u64>, data: &ProjectData) -> Result<(), String> {
        *self.0.lock().unwrap() = data.clone();
        Ok(())
    }
}

fn part(height: f64) -> Part {
    Part {
        material: None,
        name: "Test".into(),
        geometry: Geometry::Extrude {
            profile: Profile {
                outer: vec![[0., 0.], [10., 0.], [10., 20.], [0., 20.]],
                holes: vec![],
            },
            height,
        },
        placement: Placement::default(),
        colour: [50, 100, 150],
        group: "Body".into(),
        note: String::new(),
    }
}

#[test]
fn build_before_save_and_keep_failed_changes_out() {
    let store = Arc::new(Store(Mutex::new(ProjectData {
        printer: None,
        drawings: Default::default(),
        studies: Default::default(),
        materials: Default::default(),
        schema: 1,
        revision: 0,
        name: "Test".into(),
        parts: Default::default(),
        params: Default::default(),
        contacts: Default::default(),
        motion: Default::default(),
        sections: Default::default(),
    })));
    let mut project = Project::open(store.clone(), Arc::new(NativeKernel)).unwrap();
    project.put_part("base".into(), part(30.)).unwrap();
    assert_eq!(project.data.revision, 1);
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-8);
    let before = store.load().unwrap();
    assert!(project.put_part("base".into(), part(-1.)).is_err());
    assert_eq!(store.load().unwrap(), before);
    assert_eq!(project.data, before);
}

#[test]
fn unsupported_schema_is_refused() {
    let store = Arc::new(Store(Mutex::new(ProjectData {
        printer: None,
        drawings: Default::default(),
        studies: Default::default(),
        materials: Default::default(),
        schema: 999,
        revision: 0,
        name: "Test".into(),
        parts: Default::default(),
        params: Default::default(),
        contacts: Default::default(),
        motion: Default::default(),
        sections: Default::default(),
    })));
    assert!(Project::open(store, Arc::new(NativeKernel)).is_err());
}

#[test]
fn part_ids_remain_safe_on_case_insensitive_file_systems() {
    let store = Arc::new(Store(Mutex::new(ProjectData {
        printer: None,
        drawings: Default::default(),
        studies: Default::default(),
        materials: Default::default(),
        schema: 1,
        revision: 0,
        name: "Test".into(),
        parts: Default::default(),
        params: Default::default(),
        contacts: Default::default(),
        motion: Default::default(),
        sections: Default::default(),
    })));
    let mut project = Project::open(store.clone(), Arc::new(NativeKernel)).unwrap();
    project.put_part("bin".into(), part(30.)).unwrap();
    let before = store.load().unwrap();
    assert!(project.put_part("BIN".into(), part(30.)).is_err());
    assert!(project.put_part("CON".into(), part(30.)).is_err());
    assert_eq!(before, store.load().unwrap());
}

#[test]
fn parameter_changes_rebuild_parts_and_failed_changes_leave_saved_data_intact() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Parameters".into())).unwrap();
    project
        .set_params(std::collections::BTreeMap::from([(
            "height".into(),
            "30".into(),
        )]))
        .unwrap();
    let recipe: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({
        "name":"Box", "colour":[100,100,100],
        "geometry":{"op":"extrude","profile":{"outer":[[0,0],[10,0],[10,20],[0,20]]},"height":"height"}
    })).unwrap();
    project.put_part("box".into(), recipe).unwrap();
    assert!((project.bodies[0].volume - 6000.).abs() < 1e-6);
    project
        .set_params(std::collections::BTreeMap::from([(
            "height".into(),
            "40".into(),
        )]))
        .unwrap();
    assert!((project.bodies[0].volume - 8000.).abs() < 1e-6);
    let before = faber_wiring::project_fingerprint(directory.path()).unwrap();
    assert!(
        project
            .set_params(std::collections::BTreeMap::from([(
                "height".into(),
                "-1".into()
            )]))
            .is_err()
    );
    assert_eq!(
        before,
        faber_wiring::project_fingerprint(directory.path()).unwrap()
    );
    assert!((project.bodies[0].volume - 8000.).abs() < 1e-6);
}

#[test]
fn parameter_edits_reuse_parts_with_unchanged_dimensions() {
    let store = Arc::new(Store(Mutex::new(ProjectData {
        printer: None,
        drawings: Default::default(),
        studies: Default::default(),
        materials: Default::default(),
        schema: 1,
        revision: 0,
        name: "Cache".into(),
        parts: Default::default(),
        params: Default::default(),
        contacts: Default::default(),
        motion: Default::default(),
        sections: Default::default(),
    })));
    let kernel = Arc::new(CountingKernel(std::sync::atomic::AtomicUsize::new(0)));
    let mut project = Project::open(store, kernel.clone()).unwrap();
    project
        .set_params(std::collections::BTreeMap::from([(
            "height".into(),
            "30".into(),
        )]))
        .unwrap();
    let mut variable = part(30.)
        .map_numbers(|value| Ok(faber_model::Scalar::Number(*value)))
        .unwrap();
    if let Geometry::Extrude { height, .. } = &mut variable.geometry {
        *height = faber_model::Scalar::Expression("height".into());
    }
    project.put_part("variable".into(), variable).unwrap();
    project.put_part("fixed".into(), part(20.)).unwrap();
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 2);
    project
        .set_params(std::collections::BTreeMap::from([(
            "height".into(),
            "40".into(),
        )]))
        .unwrap();
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 3);
    project
        .set_params(std::collections::BTreeMap::from([(
            "height".into(),
            "20 * 2".into(),
        )]))
        .unwrap();
    assert_eq!(kernel.0.load(std::sync::atomic::Ordering::Relaxed), 3);
}
