//! The toolpath image and the viewer draw the same verified stock.
use faber_wiring::{machining, machining_image};
use std::path::{Path, PathBuf};

fn copy_example() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/milled-plate");
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("parts")).unwrap();
    for f in [
        "design.jsonnet",
        "faber.json",
        "placement.json",
        "parts/plate.json",
        "manufacture.json",
    ] {
        std::fs::copy(root.join(f), dir.path().join(f)).unwrap();
    }
    dir
}
#[test]
fn milled_plate_paths_render_over_the_verified_stock() {
    let dir = copy_example();
    faber_wiring::design_source().build(dir.path()).unwrap();
    let (data, _) = faber_wiring::read(dir.path()).unwrap();
    let run = machining::calculate(dir.path(), &data, "top", None).unwrap();
    assert_eq!(run.paths.len(), 4);
    assert!(
        run.verification.gouge_mm <= 0.02,
        "Gouge {} mm.",
        run.verification.gouge_mm
    );
    assert!(run.verification.rapid_collisions.is_empty());
    assert!(run.paths.iter().all(|(p, _)| p.cut_length() > 0.));
    let png = dir.path().join("milled.png");
    machining_image::render(&run, &png, 800, 600).unwrap();
    let pic = image::open(&png).unwrap().to_rgb8();
    let blue = pic
        .pixels()
        .filter(|p| p[2] > 170 && p[0] < 90 && p[1] < 150)
        .count();
    let yellow = pic
        .pixels()
        .filter(|p| p[0] > 190 && p[1] > 145 && p[2] < 100)
        .count();
    assert!(
        blue > 200 && yellow > 20,
        "{blue} feed pixels, {yellow} rapid pixels."
    );
    if let Some(dest) = std::env::var_os("FABER_CAM_CAPTURE") {
        let dest = PathBuf::from(dest);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::copy(&png, dest.join("milled-plate.png")).unwrap();
    }
}

#[test]
fn first_rapid_uses_the_posted_clearance_and_laser_paths_stay_in_one_plane() {
    use faber_model::machining::ToolKind;
    use machining::Move::{Feed, Rapid};
    let dir = copy_example();
    let (data, _) = faber_wiring::read(dir.path()).unwrap();
    let mut run = machining::calculate(dir.path(), &data, "top", Some(&["face".into()])).unwrap();
    let mut next = run.paths[0].clone();
    run.paths[0].0.moves = vec![Rapid { to: [2., 3., 5.] }];
    next.0.moves = vec![
        Rapid {
            to: [19., 20., 12.],
        },
        Feed {
            to: [21., 20., 9.],
            feed: 100.,
        },
    ];
    run.paths.push(next);
    let frame = run.world_from_machine;
    let xyz = |p: [f64; 3]| frame.transform_point3(p.into()).to_array();
    let scene = machining_image::scene(&run).unwrap();
    let legs = &scene.paths[1].segments;
    assert_eq!(legs.len(), 3);
    assert!(legs[0].rapid && legs[1].rapid && !legs[2].rapid);
    assert_eq!(
        (legs[0].from, legs[0].to),
        (xyz([2., 3., 5.]), xyz([2., 3., 12.]))
    );
    assert_eq!(
        (legs[1].from, legs[1].to),
        (xyz([2., 3., 12.]), xyz([19., 20., 12.]))
    );
    for (_, t) in &mut run.paths {
        t.kind = ToolKind::Laser;
    }
    let scene = machining_image::scene(&run).unwrap();
    let legs = &scene.paths[1].segments;
    let z = run.stock.max[2];
    assert_eq!(legs.len(), 2);
    assert!(legs[0].rapid && !legs[1].rapid);
    assert_eq!(
        (legs[0].from, legs[0].to),
        (xyz([2., 3., z]), xyz([19., 20., z]))
    );
    assert_eq!(legs[1].to, xyz([21., 20., z]));
}

#[test]
fn opening_a_source_only_project_seeds_the_setups_once_and_preserves_undo() {
    use faber_core::{Operation, machining::Edit};
    use faber_ports::HistoryMove;
    let dir = tempfile::tempdir().unwrap();
    let source = include_str!("../../../examples/milled-plate/design.jsonnet");
    std::fs::write(dir.path().join("design.jsonnet"), source).unwrap();
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    assert_eq!(p.data.machining.operations.len(), 4);
    let remove = Operation::Machining(Box::new(Edit::RemoveOperation { id: "holes".into() }));
    p.batch_operations(vec![remove], false).unwrap();
    faber_wiring::history_move(dir.path(), &mut p, HistoryMove::Undo).unwrap();
    assert_eq!(p.data.machining.operations.len(), 4);
    faber_wiring::history_move(dir.path(), &mut p, HistoryMove::Redo).unwrap();
    assert_eq!(p.data.machining.operations.len(), 3);
    std::fs::write(
        dir.path().join("design.jsonnet"),
        format!("{source}\n// Check the saved setup."),
    )
    .unwrap();
    faber_wiring::refresh(dir.path(), &mut p).unwrap();
    assert_eq!(p.data.machining.operations.len(), 3);
}

#[test]
fn a_saved_manufacture_file_is_kept_when_the_project_manifest_is_built() {
    let source = include_str!("../../../examples/milled-plate/design.jsonnet");
    let original = include_str!("../../../examples/milled-plate/manufacture.json");
    for first_open in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("design.jsonnet"), source).unwrap();
        let mut definition: serde_json::Value = serde_json::from_str(original).unwrap();
        definition["machining"]["tools"]["end_mill"]["feed_mm_min"] = 123.into();
        definition["machining"]["operations"]
            .as_array_mut()
            .unwrap()
            .remove(2);
        let file = dir.path().join("manufacture.json");
        std::fs::write(&file, serde_json::to_vec(&definition).unwrap()).unwrap();
        if first_open {
            faber_wiring::open(dir.path(), false, None).unwrap();
        } else {
            faber_wiring::design_source().build(dir.path()).unwrap();
        }
        let cam = faber_files::read(dir.path()).unwrap().machining;
        assert_eq!(cam.tools["end_mill"].feed_mm_min, 123.);
        assert_eq!(cam.operations.len(), 3);
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("design.jsonnet"), source).unwrap();
    let file = dir.path().join("manufacture.json");
    let broken = br#"{"schema":999}"#;
    std::fs::write(&file, broken).unwrap();
    assert!(faber_wiring::open(dir.path(), false, None).is_err());
    assert_eq!(std::fs::read(file).unwrap(), broken);
    assert!(!dir.path().join("faber.json").exists());
}
