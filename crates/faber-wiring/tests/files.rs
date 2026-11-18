use faber_model::{Geometry, Part, Placement, Profile};

mod common;

fn base_part() -> Part {
    Part {
        component: None,
        material: None,
        name: "Base".into(),
        geometry: Geometry::Extrude {
            profile: Profile {
                outer: vec![[0., 0.], [10., 0.], [10., 20.], [0., 20.]],
                holes: vec![],
            },
            height: 4.,
        },
        placement: Placement::default(),
        colour: [70, 80, 90],
        group: String::new(),
        note: String::new(),
    }
}

#[test]
fn failed_change_leaves_every_file_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Test".into())).unwrap();
    project.put_part("base".into(), base_part()).unwrap();
    let before = common::project_files(directory.path());
    assert!(project.put_part("../escape".into(), base_part()).is_err());
    assert_eq!(before, common::project_files(directory.path()));
}

#[test]
fn undo_restores_the_split_files() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Test".into())).unwrap();
    project.put_part("base".into(), base_part()).unwrap();
    let one_part = common::project_files(directory.path());
    project.put_part("lid".into(), base_part()).unwrap();
    assert!(directory.path().join("parts/lid.json").is_file());
    project.undo().unwrap();
    assert!(!directory.path().join("parts/lid.json").exists());
    // Undo gives a new revision, so the manifest bytes differ. Other files must match.
    let without_manifest = |files: Vec<(std::path::PathBuf, Vec<u8>)>| {
        files
            .into_iter()
            .filter(|(path, _)| path.file_name() != Some("faber.json".as_ref()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        without_manifest(one_part),
        without_manifest(common::project_files(directory.path()))
    );
}

#[test]
fn file_store_locks_writes_and_reopens_without_losing_data() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Test".into())).unwrap();
    assert!(faber_wiring::open(directory.path(), false, None).is_err());
    project.put_part("base".into(), base_part()).unwrap();
    let before = common::project_files(directory.path());
    assert!(project.put_part("../escape".into(), base_part()).is_err());
    assert_eq!(before, common::project_files(directory.path()));
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 800.).abs() < 1e-8);
    drop(project);
    let project = faber_wiring::open(directory.path(), false, None).unwrap();
    assert_eq!(project.data.revision, 1);
    assert_eq!(project.bodies.len(), 1);
}
