use faber_model::{Geometry, Purchased, Scalar};
use faber_ports::HistoryMove;
use faber_wiring::mesh_edit::{self, Keep, MeshEdit, Target};
use serde_json::json;

const VERTICES: &str = "0 0 0\n10 0 0\n10 10 0\n0 10 0\n0 0 10\n10 0 10\n10 10 10\n0 10 10\n";
const SIDES: &str =
    "3 0 2 1\n3 0 3 2\n3 0 1 5\n3 0 5 4\n3 1 2 6\n3 1 6 5\n3 2 3 7\n3 2 7 6\n3 3 0 4\n3 3 4 7\n";
fn project(closed: bool, purchased: bool) -> (tempfile::TempDir, faber_core::Project) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("input.off");
    let (count, top) = if closed {
        (12, "3 4 5 6\n3 4 6 7\n")
    } else {
        (10, "")
    };
    std::fs::write(&source, format!("OFF\n8 {count} 0\n{VERTICES}{SIDES}{top}")).unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Mesh tools".into())).unwrap();
    let record = purchased.then(|| Purchased {
        source: "Fixture".into(),
        licence: "Test".into(),
        designation: "Part".into(),
        mass_g: None,
    });
    p.import_with_record("cube".into(), "Cube".into(), &source, false, record)
        .unwrap();
    (dir, p)
}
fn asset(p: &faber_core::Project) -> String {
    let Geometry::Imported { asset, .. } = &p.data.parts["cube"].geometry else {
        panic!("Expected an imported mesh");
    };
    asset.clone()
}
fn edit(p: &mut faber_core::Project, op: MeshEdit) -> Result<mesh_edit::EditResult, String> {
    let revision = p.data.revision;
    mesh_edit::apply(p, "cube", revision, op)
}
fn cut(keep: Keep, cap: bool, copy: Option<&str>) -> MeshEdit {
    MeshEdit::Cut {
        origin: [0., 0., 4.],
        normal: [0., 0., 1.],
        keep,
        cap,
        copy_as: copy.map(String::from),
    }
}

#[test]
fn editing_an_instance_mesh_never_creates_an_independent_asset() {
    let (_root, mut project) = project(true, false);
    project
        .move_linked(
            "cube",
            &faber_core::Motion::Translate([5., 0., 0.]),
            "spare",
        )
        .unwrap();
    let before = project.data.clone();
    assert!(mesh_edit::report(&project, "spare").is_ok());
    let operation = MeshEdit::ToSolid {
        merge_planar: true,
        copy_as: Some("replacement".into()),
    };
    let error = mesh_edit::apply(&mut project, "spare", before.revision, operation).unwrap_err();
    assert_eq!(
        error,
        "Part spare is an instance of cube. Change part cube."
    );
    assert_eq!(project.data, before);
}

#[test]
fn repair_is_one_revision_and_undo_restores() {
    let (dir, mut p) = project(false, false);
    let before = p.data.revision;
    assert_eq!(mesh_edit::report(&p, "cube").unwrap().open_edges, 4);
    let changed = edit(&mut p, MeshEdit::Repair(Default::default())).unwrap();
    assert_eq!(changed.revision, before + 1);
    assert_eq!(changed.log["filled_holes"], 1);
    assert!(mesh_edit::report(&p, "cube").unwrap().watertight);
    faber_wiring::history_move(dir.path(), &mut p, HistoryMove::Undo).unwrap();
    assert_eq!(mesh_edit::report(&p, "cube").unwrap().open_edges, 4);
}

#[test]
fn old_asset_file_is_unchanged() {
    let (dir, mut p) = project(false, false);
    let before = asset(&p);
    let path = dir.path().join("assets").join(&before);
    let bytes = std::fs::read(&path).unwrap();
    edit(&mut p, MeshEdit::Repair(Default::default())).unwrap();
    assert_ne!(before, asset(&p));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn purchased_mesh_is_refused() {
    let (_dir, mut p) = project(true, true);
    let before = p.data.clone();
    for op in [
        MeshEdit::Repair(Default::default()),
        MeshEdit::Reduce(Target::Triangles(6)),
        cut(Keep::Below, true, Some("copy")),
        MeshEdit::ToSolid {
            merge_planar: true,
            copy_as: Some("copy".into()),
        },
    ] {
        assert_eq!(
            edit(&mut p, op).unwrap_err(),
            "Part cube is a purchased part. A change is not permitted."
        );
        assert_eq!(p.data, before);
    }
}

#[test]
fn solid_from_a_mesh_takes_a_hole() {
    let (dir, mut p) = project(true, false);
    edit(
        &mut p,
        MeshEdit::ToSolid {
            merge_planar: true,
            copy_as: Some("converted".into()),
        },
    )
    .unwrap();
    assert!(
        p.bodies
            .iter()
            .find(|b| b.id == "cube")
            .unwrap()
            .shape
            .is_none()
    );
    p.add_feature("converted",serde_json::from_value(json!({"id":"bore","geometry":{"op":"hole","source":"base","diameter":4,"depth":10,"placement":{"translation":[5,5,0]}}})).unwrap(),None,true).unwrap();
    drop(p);
    let p = faber_wiring::open(dir.path(), false, None).unwrap();
    let b = p.bodies.iter().find(|b| b.id == "converted").unwrap();
    assert!(b.shape.is_some() && (b.volume - (1000. - 40. * std::f64::consts::PI)).abs() < 1e-6);
}

#[test]
fn both_sides_keep_the_parametric_placement_and_source_precision() {
    let (_dir, mut p) = project(true, false);
    p.set_params([("offset".into(), "100.123456789".into())].into())
        .unwrap();
    let mut part = p.data.parts["cube"].clone();
    part.placement.translation = [
        Scalar::from(0.),
        Scalar::from(0.),
        serde_json::from_value(json!("offset")).unwrap(),
    ];
    p.put_part("cube".into(), part.clone()).unwrap();
    let op = MeshEdit::Cut {
        origin: [0., 0., 104.246913578],
        normal: [0., 0., 1.],
        keep: Keep::Both,
        cap: true,
        copy_as: Some("lid".into()),
    };
    let before = p.data.revision;
    let result = edit(&mut p, op).unwrap();
    assert_eq!(result.revision, before + 1);
    assert_eq!(p.data.parts["lid"].placement, part.placement);
    for (id, expected) in [("cube", 412.3456789), ("lid", 587.6543211)] {
        let r = mesh_edit::report(&p, id).unwrap();
        assert!((r.volume_mm3.unwrap() - expected).abs() < 1e-8);
    }
    assert!(
        (p.bodies.iter().find(|b| b.id == "lid").unwrap().bounds[0][2] - 104.246913578).abs()
            < 1e-9
    );
}

#[test]
fn stale_request_and_copy_collision_leave_the_assets_and_revision_unchanged() {
    let (dir, mut p) = project(true, false);
    let revision = p.data.revision;
    let count = std::fs::read_dir(dir.path().join("assets"))
        .unwrap()
        .count();
    p.set_params([("offset".into(), "2".into())].into())
        .unwrap();
    let latest = p.data.clone();
    let error =
        mesh_edit::apply(&mut p, "cube", revision, cut(Keep::Both, true, Some("lid"))).unwrap_err();
    assert!(error.to_lowercase().contains("revision"));
    assert!(edit(&mut p, cut(Keep::Both, true, Some("cube"))).is_err());
    assert_eq!(p.data, latest);
    assert_eq!(
        std::fs::read_dir(dir.path().join("assets"))
            .unwrap()
            .count(),
        count
    );
}

#[test]
fn no_op_and_open_cut_are_distinct_changes() {
    let (_dir, mut p) = project(true, false);
    let before = p.data.revision;
    let same = edit(&mut p, MeshEdit::Reduce(Target::Triangles(200))).unwrap();
    assert!(!same.changed && p.data.revision == before);
    let same = edit(
        &mut p,
        MeshEdit::Cut {
            origin: [0., 0., 15.],
            normal: [0., 0., 1.],
            keep: Keep::Below,
            cap: true,
            copy_as: None,
        },
    )
    .unwrap();
    assert!(!same.changed && p.data.revision == before);
    edit(&mut p, cut(Keep::Above, false, Some("open"))).unwrap();
    assert_eq!(p.data.revision, before + 1);
    assert!(mesh_edit::report(&p, "cube").unwrap().watertight);
    assert!(!mesh_edit::report(&p, "open").unwrap().watertight);
}

#[test]
fn unbuildable_staged_asset_is_not_published() {
    let (dir, mut project) = project(true, false);
    let temporary = dir.path().join("invalid.off");
    std::fs::write(&temporary, "OFF\n0 0 0\n").unwrap();
    let key = project.asset_key(&temporary).unwrap();
    let revision = project.data.revision;
    let mut part = project.data.parts["cube"].clone();
    part.geometry = Geometry::Imported {
        asset: key.clone(),
        purchased: None,
        link: None,
    };
    let edit = faber_model::Edit::PutPart {
        id: "cube".into(),
        part,
    };
    assert!(
        project
            .batch_with_assets(revision, vec![edit], [(key.clone(), temporary)].into())
            .is_err()
    );
    assert_eq!(project.data.revision, revision);
    assert!(!dir.path().join("assets").join(key).exists());
}
