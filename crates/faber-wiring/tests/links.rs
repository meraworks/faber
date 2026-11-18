use faber_kernel_occt::OcctKernel;
use faber_model::{Edit, Geometry, Scalar};
use faber_ports::Kernel;
use faber_wiring::links;
use std::path::Path;

fn board(path: &Path, width: f64) {
    OcctKernel
        .write_shape(&OcctKernel.box_shape([width, 20., 1.6]).unwrap(), path)
        .unwrap();
}
fn fixture() -> (tempfile::TempDir, faber_core::Project) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    board(&root.join("board.step"), 40.);
    let mut p = faber_wiring::open(root, true, Some("Linked board".into())).unwrap();
    links::import(
        root,
        &mut p,
        "board".into(),
        "Board".into(),
        Path::new("board.step"),
        false,
    )
    .unwrap();
    (dir, p)
}
fn width(p: &faber_core::Project, id: &str) -> f64 {
    let body = p.bodies.iter().find(|b| b.id == id).unwrap();
    body.bounds[1][0] - body.bounds[0][0]
}
fn state(root: &Path, p: &faber_core::Project) -> String {
    let items = serde_json::to_value(links::list(root, p).unwrap()).unwrap();
    items[0]["state"].as_str().unwrap().into()
}
#[test]
fn changed_file_is_reported() {
    let (dir, p) = fixture();
    assert_eq!(state(dir.path(), &p), "current");
    board(&dir.path().join("board.step"), 50.);
    assert_eq!(state(dir.path(), &p), "changed");
    assert!((width(&p, "board") - 40.).abs() < 1e-5);
}
#[test]
fn refresh_updates_the_asset_and_keeps_the_placement() {
    let (dir, mut p) = fixture();
    let mut part = p.data.parts["board"].clone();
    part.placement.translation = [12., 4., 6.].map(Scalar::Number);
    p.put_part("board".into(), part.clone()).unwrap();
    board(&dir.path().join("board.step"), 75.);
    let revision = p.data.revision;
    assert_eq!(
        links::refresh(dir.path(), &mut p, None, revision).unwrap(),
        ["board"]
    );
    assert_eq!(p.data.parts["board"].placement, part.placement);
    assert!((width(&p, "board") - 75.).abs() < 1e-5);
    assert_eq!(state(dir.path(), &p), "current");
    let rev = p.data.revision;
    assert!(
        links::refresh(dir.path(), &mut p, None, rev)
            .unwrap()
            .is_empty()
    );
    assert_eq!(p.data.revision, rev);
}
#[test]
fn refresh_is_one_revision_and_undo_restores_the_old_asset() {
    let (dir, mut p) = fixture();
    links::import(
        dir.path(),
        &mut p,
        "copy".into(),
        "Second board".into(),
        Path::new("board.step"),
        false,
    )
    .unwrap();
    let revision = p.data.revision;
    board(&dir.path().join("board.step"), 65.);
    assert_eq!(
        links::refresh(dir.path(), &mut p, None, revision).unwrap(),
        ["board", "copy"]
    );
    assert_eq!(p.data.revision, revision + 1);
    p.undo().unwrap();
    assert!((width(&p, "board") - 40.).abs() < 1e-5 && (width(&p, "copy") - 40.).abs() < 1e-5);
    assert_eq!(state(dir.path(), &p), "changed");
}
#[test]
fn missing_link_keeps_the_last_asset() {
    let (dir, p) = fixture();
    drop(p);
    std::fs::remove_file(dir.path().join("board.step")).unwrap();
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    assert_eq!(state(dir.path(), &p), "missing");
    let rev = p.data.revision;
    assert!(
        links::refresh(dir.path(), &mut p, None, rev)
            .unwrap()
            .is_empty()
    );
    assert!((width(&p, "board") - 40.).abs() < 1e-5);
}
#[test]
fn geometry_tool_on_a_linked_part_is_refused() {
    let (dir, mut p) = fixture();
    let mut part = p.data.parts["board"].clone();
    let rev = p.data.revision;
    part.geometry = Geometry::Box {
        size: [1., 2., 3.].map(Scalar::Number),
    };
    let want = "Part board follows the file board.step. Change the file, then call refresh_links.";
    assert_eq!(p.put_part("board".into(), part.clone()).unwrap_err(), want);
    assert_eq!(
        p.batch(vec![Edit::PutPart {
            id: "board".into(),
            part: part.clone()
        }])
        .unwrap_err(),
        want
    );
    assert_eq!(
        p.batch(vec![
            Edit::RemovePart { id: "board".into() },
            Edit::PutPart {
                id: "board".into(),
                part
            }
        ])
        .unwrap_err(),
        want
    );
    assert_eq!(
        faber_core::pick_selector::editable_part("board", &p.data.parts["board"]).unwrap_err(),
        want
    );
    assert_eq!(p.data.revision, rev);
    assert_eq!(state(dir.path(), &p), "current");
}
#[test]
fn relative_path_follows_the_project_folder() {
    let (dir, p) = fixture();
    drop(p);
    let outer = tempfile::tempdir().unwrap();
    let moved = outer.path().join("moved");
    std::fs::rename(dir.path(), &moved).unwrap();
    let p = faber_wiring::open(&moved, false, None).unwrap();
    assert_eq!(state(&moved, &p), "current");
    let rows = serde_json::to_value(links::list(&moved, &p).unwrap()).unwrap();
    assert_eq!(rows[0]["path"], "board.step");
}
#[test]
fn bad_refresh_cannot_partly_replace_a_valid_link() {
    let (dir, mut p) = fixture();
    board(&dir.path().join("other.step"), 30.);
    links::import(
        dir.path(),
        &mut p,
        "other".into(),
        "Other".into(),
        Path::new("other.step"),
        false,
    )
    .unwrap();
    let rev = p.data.revision;
    let before = p.data.clone();
    board(&dir.path().join("board.step"), 55.);
    std::fs::write(dir.path().join("other.step"), "invalid solid").unwrap();
    assert!(links::refresh(dir.path(), &mut p, None, rev).is_err());
    assert_eq!(p.data, before);
    assert_eq!(
        links::refresh(dir.path(), &mut p, Some(&["board".into()]), rev).unwrap(),
        ["board"]
    );
    assert!((width(&p, "other") - 30.).abs() < 1e-5);
}

#[test]
fn independent_copy_keeps_the_linked_asset_when_the_file_changes() {
    let (dir, mut p) = fixture();
    p.move_linked(
        "board",
        &faber_core::Motion::Translate([80., 0., 0.]),
        "spare",
    )
    .unwrap();
    let place = p.data.parts["spare"].placement.clone();
    p.make_independent("spare").unwrap();
    board(&dir.path().join("board.step"), 55.);
    let revision = p.data.revision;
    assert_eq!(
        links::refresh(dir.path(), &mut p, None, revision).unwrap(),
        ["board"]
    );
    assert!((width(&p, "spare") - 40.).abs() < 1e-5);
    assert_eq!(p.data.parts["spare"].placement, place);
    faber_core::pick_selector::editable_part("spare", &p.data.parts["spare"]).unwrap();
}

#[test]
fn ordinary_copies_of_a_link_and_its_instance_have_their_own_geometry() {
    let (dir, mut p) = fixture();
    let move_by = faber_core::Motion::Translate([80., 0., 0.]);
    p.move_linked("board", &move_by, "spare").unwrap();
    for (from, to) in [("board", "direct"), ("spare", "from_spare")] {
        p.move_part(from, move_by.clone(), Some(to)).unwrap();
    }
    board(&dir.path().join("board.step"), 55.);
    let revision = p.data.revision;
    assert_eq!(
        links::refresh(dir.path(), &mut p, None, revision).unwrap(),
        ["board"]
    );
    for copy in ["direct", "from_spare"] {
        assert!(matches!(
            p.data.parts[copy].geometry,
            Geometry::Imported { link: None, .. }
        ));
        assert!((width(&p, copy) - 40.).abs() < 1e-5);
    }
}
