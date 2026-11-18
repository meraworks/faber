use faber_drawing::{ModelSource, Resolved};
use faber_model::{Edit, Part, Scalar};
use faber_ports::drawing::{Curve2, ViewFrame};
use faber_wiring::drawing;
use serde_json::json;
use std::{fs, path::Path};

fn save_auto(project: &mut faber_core::Project) {
    let drawing = drawing::auto_drawing(project, &[], Default::default(), "Workshop").unwrap();
    assert_eq!(drawing.views.len(), 4);
    assert!(drawing.dimensions.len() >= 3);
    project
        .batch_at_revision(
            project.data.revision,
            vec![Edit::SetDrawing {
                id: "overview".into(),
                drawing,
            }],
        )
        .unwrap();
}

#[test]
fn auto_drawing_of_the_trash_bin_example() {
    let dir = tempfile::tempdir().unwrap();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/trash-bin");
    for f in ["faber.json", "placement.json", "analysis.json"] {
        fs::copy(example.join(f), dir.path().join(f)).unwrap();
    }
    fs::create_dir_all(dir.path().join("parts")).unwrap();
    fs::copy(
        example.join("parts/bin.json"),
        dir.path().join("parts/bin.json"),
    )
    .unwrap();
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    save_auto(&mut p);
    let path = dir.path().join("bin.svg");
    let rendered = drawing::export(&p, "overview", &path, None).unwrap();
    assert!(rendered.problems.is_empty(), "{:?}", rendered.problems);
    assert!(fs::read_to_string(path).unwrap().contains("VISIBLE"));
}

#[test]
fn drawing_follows_a_parameter_change() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Drawings".into())).unwrap();
    p.set_params([("w".into(), "40".into())].into()).unwrap();
    let recipe: Part<Scalar> = serde_json::from_value(
        json!({"name":"Plate","geometry":{"op":"box","size":["w",20,10]},"colour":[120,150,140]}),
    )
    .unwrap();
    p.put_part("plate".into(), recipe).unwrap();
    save_auto(&mut p);
    let path = dir.path().join("sheet.svg");
    drawing::export(&p, "overview", &path, None).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains(">40</text>"));
    p.set_params([("w".into(), "50".into())].into()).unwrap();
    let rendered = drawing::export(&p, "overview", &path, None).unwrap();
    assert!(rendered.problems.is_empty(), "{:?}", rendered.problems);
    assert!(fs::read_to_string(&path).unwrap().contains(">50</text>"));
    p.set_params([("w".into(), "100".into())].into()).unwrap();
    drawing::export(&p, "overview", &path, None).unwrap();
    assert!(fs::read_to_string(path).unwrap().contains(">100</text>"));
}

fn drilled_plate(with_second: bool) -> Part<Scalar> {
    let mut features = vec![
        json!({"id":"stock","geometry":{"op":"box","size":[60,30,5]}}),
        json!({"id":"drill_a","geometry":{"op":"cylinder","radius":5,"height":5}}),
        json!({"id":"at_a","geometry":{"op":"transform","source":"drill_a","placement":{"translation":[12,15,0]}}}),
        json!({"id":"first_bore","geometry":{"op":"difference","left":"stock","right":"at_a"}}),
    ];
    let mut output = "first_bore";
    if with_second {
        features.extend([
            json!({"id":"drill_b","geometry":{"op":"cylinder","radius":"r","height":5}}),
            json!({"id":"at_b","geometry":{"op":"transform","source":"drill_b","placement":{"translation":[45,15,0]}}}),
            json!({"id":"both_bores","geometry":{"op":"difference","left":"first_bore","right":"at_b"}}),
        ]);
        output = "both_bores";
    }
    serde_json::from_value(json!({"name":"Mount","colour":[100,150,180],
        "geometry":{"op":"history","features":features,"output":output}
    }))
    .unwrap()
}

#[test]
fn saved_bore_diameters_follow_a_resize_and_a_removed_bore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bores.svg");
    let mut p = faber_wiring::open(dir.path(), true, Some("Fixture".into())).unwrap();
    p.set_params([("r".into(), "5".into())].into()).unwrap();
    p.put_part("plate".into(), drilled_plate(true)).unwrap();
    save_auto(&mut p);
    drawing::export(&p, "overview", &path, None).unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains(">2x Ø10</text>")
    );

    p.set_params([("r".into(), "6".into())].into()).unwrap();
    let updated = drawing::export(&p, "overview", &path, None).unwrap();
    let svg = fs::read_to_string(&path).unwrap();
    assert!(
        !svg.contains("2x Ø10"),
        "Both holes still have the old diameter"
    );
    assert!(svg.contains(">?</text>"));
    assert!(
        updated
            .problems
            .iter()
            .any(|p| p.starts_with("Dimension diameter")),
        "{:?}",
        updated.problems
    );

    p.put_part("plate".into(), drilled_plate(false)).unwrap();
    let removed = drawing::export(&p, "overview", &path, None).unwrap();
    let svg = fs::read_to_string(&path).unwrap();
    assert!(!svg.contains("2x Ø10") && !svg.contains("Ø12"));
    assert!(
        removed
            .problems
            .iter()
            .any(|p| p.starts_with("Dimension diameter")),
        "The removed circular reference must be reported"
    );
    assert!(svg.contains(">?</text>"));
}

#[test]
fn mesh_outline_keeps_a_concave_corner_and_omits_internal_diagonals() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("angle.off");
    fs::write(&input,"OFF\n6 4 0\n0 0 0\n3 0 0\n3 1 0\n1 1 0\n1 3 0\n0 3 0\n3 0 1 3\n3 1 2 3\n3 0 3 5\n3 3 4 5\n").unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Angle".into())).unwrap();
    p.import_part("angle".into(), "Angle".into(), &input, false)
        .unwrap();
    let m = drawing::DrawingModel::new(&p);
    let top = ViewFrame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
    };
    let q = m.project(&[], &top, true).unwrap();
    assert!(m.limits(&[]).iter().any(|s| s.contains("outline")));
    assert!(q.hidden.is_empty());
    assert_eq!(q.visible.len(), 1);
    let Curve2::Polyline(points) = &q.visible[0] else {
        panic!("Expected an outline");
    };
    assert_eq!(points.len(), 7);
    assert!(
        points
            .iter()
            .any(|p| (p[0] - 1.).abs() < 1e-7 && (p[1] - 1.).abs() < 1e-7)
    );
    let edge_view = ViewFrame {
        origin: [0.; 3],
        x: [1., 0., 0.],
        y: [0., 0., 1.],
    };
    let edge = m.project(&[], &edge_view, false).unwrap();
    assert_eq!(
        edge.visible.len(),
        1,
        "An edge-on sheet must show its span."
    );
    assert_eq!(edge.bounds, [[0., 0.], [3., 0.]]);
}

#[test]
fn parts_list_groups_resolved_geometry_and_accounts_for_unit_mass() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Parts".into())).unwrap();
    p.set_params([("width".into(), "10".into())].into())
        .unwrap();
    for (id, size, offset) in [("first", json!("width"), 0), ("second", json!(10), 20)] {
        let part:Part<Scalar>=serde_json::from_value(json!({"name":"Block","material":"pla","colour":[80,100,120],"geometry":{"op":"box","size":[size,10,10]},"placement":{"translation":[offset,0,0]}})).unwrap();
        p.put_part(id.into(), part).unwrap();
    }
    let rows = drawing::DrawingModel::new(&p).parts_list(&[]).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].quantity, 2);
    assert!((rows[0].mass_g.unwrap() - 1.24).abs() < 0.05);
}

#[test]
fn oblique_section_returns_loops_in_the_given_frame_and_discards_foreground() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Section".into())).unwrap();
    for (id, y) in [("cross", 0), ("front", -10)] {
        let part:Part<Scalar>=serde_json::from_value(json!({"name":id,"colour":[80,100,120],"geometry":{"op":"box","size":[4,4,4]},"placement":{"translation":[if y<0{10}else{0},y,0]}})).unwrap();
        p.put_part(id.into(), part).unwrap();
    }
    let model = drawing::DrawingModel::new(&p);
    let k = 0.5_f64.sqrt();
    let frame = ViewFrame {
        origin: [2., 2., 0.],
        x: [k, k, 0.],
        y: [0., 0., 1.],
    };
    let loops = model.section_in_frame(&[], &frame).unwrap();
    assert_eq!(loops.len(), 1);
    assert_eq!(loops[0].len(), 4);
    let back = model.cut_view(&[], &frame, false).unwrap();
    assert!(
        (back.bounds[1][0] - 2. * std::f64::consts::SQRT_2).abs() < 1e-5,
        "{:?}",
        back.bounds
    );
    let anchors = model.anchors(&["cross".into()]).unwrap();
    assert!(anchors.iter().any(
        |(a, value)| model.resolve(a).unwrap() == *value && matches!(value, Resolved::Point(_))
    ));
}

#[test]
fn drawing_snapshot_can_read_while_an_editor_holds_the_lock() {
    let dir = tempfile::tempdir().unwrap();
    let p = faber_wiring::open(dir.path(), true, Some("Live".into())).unwrap();
    let mut read = drawing::open_snapshot(dir.path()).unwrap();
    assert_eq!(read.data.revision, p.data.revision);
    assert!(
        read.set_params([("width".into(), "10".into())].into())
            .is_err()
    );
}

#[test]
fn overlapped_section_hatches_the_shared_material_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Cut".into())).unwrap();
    for (id, x) in [("base", 0), ("overlap", 2)] {
        let part:Part<Scalar>=serde_json::from_value(json!({"name":id,"colour":[80,100,120],"geometry":{"op":"box","size":[4,4,4]},"placement":{"translation":[x,0,0]}})).unwrap();
        p.put_part(id.into(), part).unwrap();
    }
    let frame = ViewFrame {
        origin: [0., 0., 2.],
        x: [1., 0., 0.],
        y: [0., 1., 0.],
    };
    let loops = drawing::DrawingModel::new(&p)
        .section_in_frame(&[], &frame)
        .unwrap();
    assert_eq!(loops.len(), 1);
    let ring = &loops[0];
    let twice_area: f64 = ring
        .iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
        .sum();
    assert!((0.5 * twice_area.abs() - 24.).abs() < 1e-7);
}

#[test]
fn automatic_diameter_anchors_keep_outer_cylinders_distinct_from_holes() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Shaft".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({
        "name": "Axle", "colour": [120,150,140],
        "geometry": {"op":"history","features":[
            {"id":"shaft","geometry":{"op":"cylinder","radius":4,"height":12}},
            {"id":"collar","geometry":{"op":"cylinder","radius":7,"height":4}},
            {"id":"stepped","geometry":{"op":"union","left":"shaft","right":"collar"}}
        ], "output":"stepped"}
    }))
    .unwrap();
    p.put_part("axle".into(), part).unwrap();
    let anchors = drawing::DrawingModel::new(&p).anchors(&[]).unwrap();
    let cylinders: Vec<_> = anchors
        .iter()
        .filter_map(|(a, _)| match a {
            faber_model::drawing::Anchor::CylinderAxis { face, .. } => Some(face),
            _ => None,
        })
        .collect();
    assert_eq!(cylinders.len(), 2);
    assert!(cylinders.iter().all(|f| f.internal == Some(false)));
}

#[test]
fn drawing_snapshot_does_not_mix_a_pending_part_with_the_old_revision() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Live".into())).unwrap();
    let shape = |width: i32| -> Part<Scalar> {
        serde_json::from_value(json!({
            "name":"Plate","colour":[0,128,190],"geometry":{"op":"box","size":[width,5,4]}
        }))
        .unwrap()
    };
    p.put_part("plate".into(), shape(10)).unwrap();
    let manifest = dir.path().join("faber.json");
    let old_manifest = fs::read(&manifest).unwrap();
    let old_rev = p.data.revision;
    p.put_part("plate".into(), shape(30)).unwrap();
    fs::write(&manifest, &old_manifest).unwrap();
    let snapshot = drawing::open_snapshot(dir.path()).unwrap();
    assert_eq!(snapshot.data.revision, old_rev);
    assert!((snapshot.bodies[0].bounds[1][0] - 10.).abs() < 1e-6);
    assert_eq!(fs::read(manifest).unwrap(), old_manifest);
}

#[test]
fn drawing_snapshot_respects_authored_part_edits_without_a_pending_save() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Live".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({
        "name":"Plate","colour":[0,128,190],"geometry":{"op":"box","size":[10,5,4]}
    }))
    .unwrap();
    p.put_part("plate".into(), part).unwrap();
    let file = dir.path().join("parts/plate.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    value["geometry"]["size"][0] = json!(17);
    fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
    let snapshot = drawing::open_snapshot(dir.path()).unwrap();
    assert_eq!(snapshot.data.revision, p.data.revision);
    assert!((snapshot.bodies[0].bounds[1][0] - 17.).abs() < 1e-6);
}
