use faber_model::{Edit, Geometry};
use faber_ports::Mesh;
use faber_wiring::study::{run_analysis, save_shape};
use glam::DVec3;
use serde_json::json;

fn same_placed_mesh(actual: &Mesh, expected: &Mesh) {
    assert_eq!(actual.triangles, expected.triangles);
    assert_eq!(actual.vertices.len(), expected.vertices.len());
    for (&a, &b) in actual.vertices.iter().zip(&expected.vertices) {
        assert!(
            DVec3::from(a).distance(DVec3::from(b)) < 1e-7,
            "{a:?} vs {b:?}"
        );
    }
}

#[test]
fn shape_save_preserves_the_placed_mesh_and_stock_recipe() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(dir.path(), "Placed stock".into()).unwrap();
    let component =
        serde_json::from_value(json!({"tool":"set_component","id":"frame","component":{
            "name":"Frame","placement":{"translation":[30.,57.,12.],"rotation":[0.,0.,90.]}
        }}))
        .unwrap();
    let recipe = serde_json::from_value(
        json!({"name":"Stock","material":"steel-s235","component":"frame","colour":[1,2,3],
       "placement":{"translation":[7.,4.,5.],"rotation":[90.,0.,0.]},
       "geometry":{"op":"box","size":[24,8,8]}}),
    )
    .unwrap();
    let study=serde_json::from_value(json!({"part":"stock","kind":"shape","resolution_mm":2.,"target_volume":0.58,"min_member_mm":4.,
        "fixtures":[{"kind":"fixed","face":{"normal":[-1,0,0],"expect":1,"tolerance":0.01}}],
        "loads":[{"kind":"force","face":{"normal":[1,0,0],"expect":1,"tolerance":0.01},"newtons":[0,0,-20]}]})).unwrap();
    p.batch(vec![
        component,
        Edit::PutPart {
            id: "stock".into(),
            part: recipe,
        },
        Edit::SetStudy {
            id: "light".into(),
            study,
        },
    ])
    .unwrap();
    let source_path = dir.path().join("parts/stock.json");
    let source_bytes = std::fs::read(&source_path).unwrap();
    let before = p.data.revision;
    let mesh = run_analysis(&p, "light", None).unwrap().shape_mesh.unwrap();
    let stock = &p.bodies.iter().find(|b| b.id == "stock").unwrap();
    assert!(stock.bounds[0][1] > 50.);
    for &v in &mesh.vertices {
        for (axis, &value) in v.iter().enumerate() {
            assert!((stock.bounds[0][axis] - 0.2..=stock.bounds[1][axis] + 0.2).contains(&value));
        }
    }
    let asset_count = || std::fs::read_dir(dir.path().join("assets")).map_or(0, |s| s.count());
    let initial_count = asset_count();
    assert!(save_shape(&mut p, "stock", &mesh, before - 1).is_err());
    assert_eq!(asset_count(), initial_count);
    let id = save_shape(&mut p, "stock", &mesh, before).unwrap();
    assert_eq!(p.data.revision, before + 1);
    assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    let Geometry::Imported {
        asset,
        purchased: None,
        link: None,
    } = &p.data.parts[&id].geometry
    else {
        panic!("Expected an independent mesh asset")
    };
    let asset = asset.clone();
    let bytes = std::fs::read(dir.path().join("assets").join(&asset)).unwrap();
    assert_eq!(p.data.parts[&id].component, p.data.parts["stock"].component);
    assert_eq!(p.data.parts[&id].placement, p.data.parts["stock"].placement);
    same_placed_mesh(&p.bodies.iter().find(|b| b.id == id).unwrap().mesh, &mesh);
    drop(p);
    let reopened = faber_wiring::open(dir.path(), false, None).unwrap();
    same_placed_mesh(
        &reopened.bodies.iter().find(|b| b.id == id).unwrap().mesh,
        &mesh,
    );
    assert_eq!(std::fs::read(&source_path).unwrap(), source_bytes);
    assert_eq!(
        std::fs::read(dir.path().join("assets").join(&asset)).unwrap(),
        bytes
    );
}

#[test]
fn generated_stock_keeps_its_saved_shape_across_a_source_build() {
    let dir = tempfile::tempdir().unwrap();
    let source = br#"local f=import 'faber.libsonnet'; local end(x)={normal:[x,0,0],expect:1,tolerance:0.01};{
        parts:{bracket:f.part('Stock',[f.box('block',[24,8,8])],'block',material='steel-s235')},
        studies:{reduce_stock:f.shape_study('bracket',[f.fixed(end(-1))],[f.force(end(1),[0,0,-20])],target_volume=0.58,resolution_mm=2)}
    }"#;
    let path = dir.path().join("design.jsonnet");
    std::fs::write(&path, source).unwrap();
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    let snapshot = faber_wiring::read_snapshot(dir.path()).unwrap();
    assert_eq!(
        faber_wiring::study::check_shape_destination(&snapshot, "bracket").unwrap(),
        "bracket-shape"
    );
    let stock_path = dir.path().join("parts/bracket.json");
    let stock_bytes = std::fs::read(&stock_path).unwrap();
    let mesh = run_analysis(&p, "reduce_stock", Some(2.))
        .unwrap()
        .shape_mesh
        .unwrap();
    let before = p.data.revision;
    let stale = String::from_utf8(source.to_vec()).unwrap() + " // changed";
    std::fs::write(&path, stale).unwrap();
    assert!(
        save_shape(&mut p, "bracket", &mesh, before)
            .unwrap_err()
            .contains("Build")
    );
    assert_eq!(p.data.revision, before);
    assert!(!dir.path().join("study-results").exists());
    std::fs::write(&path, source).unwrap();
    let id = save_shape(&mut p, "bracket", &mesh, before).unwrap();
    assert_eq!(p.data.revision, before + 1);
    assert_eq!(std::fs::read(&path).unwrap(), source);
    faber_wiring::build_source(dir.path(), &mut p).unwrap();
    same_placed_mesh(&p.bodies.iter().find(|b| b.id == id).unwrap().mesh, &mesh);
    assert_eq!(std::fs::read(&stock_path).unwrap(), stock_bytes);
    assert_eq!(std::fs::read(&path).unwrap(), source);
    faber_wiring::history_move(dir.path(), &mut p, faber_ports::HistoryMove::Undo).unwrap();
    assert!(!p.data.parts.contains_key(&id));
    faber_wiring::history_move(dir.path(), &mut p, faber_ports::HistoryMove::Redo).unwrap();
    assert!(p.data.parts.contains_key(&id));
    let record = dir.path().join(".faber/build.json");
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("parts");
    std::fs::write(&record, serde_json::to_vec(&legacy).unwrap()).unwrap();
    faber_wiring::build_source(dir.path(), &mut p).unwrap();
    assert!(p.data.parts.contains_key(&id));
    std::fs::remove_file(&record).unwrap();
    faber_wiring::build_source(dir.path(), &mut p).unwrap();
    same_placed_mesh(&p.bodies.iter().find(|b| b.id == id).unwrap().mesh, &mesh);
    let original = p.data.clone();
    let conflicting = format!(
        "({}) + {{parts+: {{'BRACKET-shape': {{name:'New stock', colour:[4,5,6], geometry:{{op:'box',size:[10,10,10]}}}}}}}}",
        std::str::from_utf8(source).unwrap()
    );
    std::fs::write(&path, conflicting).unwrap();
    let error = faber_wiring::build_source(dir.path(), &mut p).unwrap_err();
    assert!(error.contains("saved shape"), "{error}");
    assert_eq!(p.data, original);
    std::fs::write(&path, source).unwrap();
    faber_wiring::build_source(dir.path(), &mut p).unwrap();
    assert!(p.data.parts.contains_key(&id));
}

#[test]
fn legacy_source_owned_mesh_is_removed_when_its_declaration_is_removed() {
    use faber_ports::AssetStore;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("block.off");
    std::fs::write(
        &path,
        "OFF\n4 4 0\n0 0 0\n1 0 0\n0 1 0\n0 0 1\n3 0 2 1\n3 0 1 3\n3 1 2 3\n3 2 0 3\n",
    )
    .unwrap();
    let asset = faber_files::FileAssets::new(dir.path()).put(&path).unwrap();
    let source = dir.path().join("design.jsonnet");
    std::fs::write(&source,format!("local f=import 'faber.libsonnet';{{parts:{{'bracket-shape':f.saved_mesh('Imported mesh','{asset}'),'-shape':f.saved_mesh('Legacy part','{asset}')}}}}")).unwrap();
    let mut p = faber_wiring::open(dir.path(), false, None).unwrap();
    assert!(p.data.parts.contains_key("bracket-shape"));
    let record = dir.path().join(".faber/build.json");
    let mut previous: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap();
    previous.as_object_mut().unwrap().remove("parts");
    std::fs::write(&record, serde_json::to_vec(&previous).unwrap()).unwrap();
    std::fs::write(&source, "{parts:{}}").unwrap();
    faber_wiring::build_source(dir.path(), &mut p).unwrap();
    assert!(!p.data.parts.contains_key("bracket-shape"));
}
