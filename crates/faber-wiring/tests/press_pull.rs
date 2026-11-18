use faber_model::{Geometry, Part, Scalar};
use faber_wiring::press_pull::{PressFace, apply, preview};
use serde_json::json;

fn solid(geometry: serde_json::Value) -> (tempfile::TempDir, faber_core::Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::create(dir.path(), "Press pull".into()).unwrap();
    let part: Part<Scalar> =
        serde_json::from_value(json!({"name":"Block","colour":[10,20,30],"geometry":geometry}))
            .unwrap();
    project.put_part("block".into(), part).unwrap();
    (dir, project)
}
fn top(project: &faber_core::Project) -> PressFace {
    let faces = project.topology("block").unwrap().1;
    let face = faces
        .iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.99))
        .unwrap();
    PressFace::new(project, "block", face.index).unwrap()
}
#[test]
fn press_pull_adds_ten_mm_and_saves_one_revision() {
    let (dir, mut p) = solid(json!({"op":"box","size":[20,20,20]}));
    let initial = p.data.clone();
    let target = top(&p);
    let previewed = preview(&p, &target, 10.).unwrap();
    assert!((previewed.volume - 12000.).abs() < 1e-6);
    assert_eq!(faber_files::read(dir.path()).unwrap(), initial);
    apply(&mut p, &target, 10., false).unwrap();
    assert_eq!(p.data.revision, initial.revision + 1);
    assert!((p.bodies[0].volume - 12000.).abs() < 1e-6);
    let Geometry::History { features, .. } = &p.data.parts["block"].geometry else {
        panic!()
    };
    assert_eq!(features.len(), 3);
    assert!(matches!(features[1].geometry, Geometry::Thicken { .. }));
    assert!(matches!(features[2].geometry, Geometry::Union { .. }));
    p.undo().unwrap();
    assert_eq!(p.data.parts, initial.parts);
}
#[test]
fn press_pull_cuts_five_mm_and_refuses_a_cut_through_the_part() {
    let (_dir, mut p) = solid(json!({"op":"box","size":[20,20,20]}));
    let face = top(&p);
    assert!((preview(&p, &face, -5.).unwrap().volume - 6000.).abs() < 1e-6);
    assert_eq!(
        apply(&mut p, &face, -20., false).unwrap_err(),
        "The cut removes the whole part."
    );
    assert_eq!(p.data.revision, 1);
}
#[test]
fn press_pull_refuses_a_curved_face() {
    let (_dir, p) = solid(json!({"op":"cylinder","radius":4,"height":5}));
    let id = p
        .topology("block")
        .unwrap()
        .1
        .into_iter()
        .find(|f| f.cylinder.is_some())
        .unwrap()
        .index;
    assert_eq!(
        PressFace::new(&p, "block", id).err().unwrap(),
        "Press pull needs a planar face."
    );
}
