use faber_model::{Geometry, Part, Placement, Profile};

#[test]
fn file_store_locks_writes_and_reopens_without_losing_data() {
    let directory = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(directory.path(), true, Some("Test".into())).unwrap();
    assert!(faber_wiring::open(directory.path(), false, None).is_err());
    let part = Part {
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
    };
    project.put_part("base".into(), part.clone()).unwrap();
    let before = std::fs::read(directory.path().join("faber.json")).unwrap();
    assert!(project.put_part("../escape".into(), part).is_err());
    assert_eq!(
        before,
        std::fs::read(directory.path().join("faber.json")).unwrap()
    );
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - 800.).abs() < 1e-8);
    drop(project);
    let project = faber_wiring::open(directory.path(), false, None).unwrap();
    assert_eq!(project.data.revision, 1);
    assert_eq!(project.bodies.len(), 1);
}
