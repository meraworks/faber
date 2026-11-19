use faber_model::Scalar;
use faber_model::study::Study;
use faber_wiring::study::{run_analysis, save_shape};
use serde_json::{Value, json};

fn face(n: [i32; 3]) -> Value {
    json!({"normal":n,"tolerance":0.01,"expect":1})
}
fn block(kind: Value) -> (tempfile::TempDir, faber_core::Project) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::create(dir.path(), "Studies".into()).unwrap();
    p.put_part::<Scalar>(
        "block".into(),
        serde_json::from_value(json!({"name":"Block","colour":[90,120,130],
        "material":"steel-s235","geometry":{"op":"box","size":[24,8,8]}}))
        .unwrap(),
    )
    .unwrap();
    let mut s = json!({"part":"block","resolution_mm":2,"fixtures":[{"kind":"fixed","face":face([-1,0,0])}],
        "loads":[{"kind":"force","face":face([1,0,0]),"newtons":[0,0,-20]}]});
    s.as_object_mut()
        .unwrap()
        .extend(kind.as_object().unwrap().clone());
    p.set_study(
        "study".into(),
        serde_json::from_value::<Study<Scalar>>(s).unwrap(),
    )
    .unwrap();
    (dir, p)
}
#[test]
fn modal_report_has_three_frequencies_in_rising_order() {
    let (_d, p) = block(json!({"kind":"modal","modes":3,"loads":[]}));
    let output = run_analysis(&p, "study", None).unwrap();
    let modes = output.report["modes"].as_array().unwrap();
    let f: Vec<_> = modes
        .iter()
        .map(|m| m["frequency_hz"].as_f64().unwrap())
        .collect();
    assert!(f[0] > 0. && f.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!((f.len(), output.fields.len()), (3, 3));
}
#[test]
fn invalid_study_settings_leave_the_project_unchanged() {
    let (_d, mut p) = block(json!({"kind":"modal","modes":1,"loads":[]}));
    let before = p.data.clone();
    for (kind, word) in [
        (json!({"kind":"modal","modes":0}), "mode"),
        (json!({"kind":"shape","target_volume":1.2}), "volume"),
        (
            json!({"kind":"thermal","stress":false,"reference_c":20,"conditions":[
            {"kind":"convection","w_m2k":-2,"ambient_c":18,"face":face([1,0,0])}]}),
            "convection",
        ),
    ] {
        let mut s = json!({"part":"block","loads":[],"fixtures":[]});
        s.as_object_mut()
            .unwrap()
            .extend(kind.as_object().unwrap().clone());
        assert!(
            p.set_study("bad".into(), serde_json::from_value(s).unwrap())
                .unwrap_err()
                .to_lowercase()
                .contains(word)
        );
        assert_eq!(p.data, before);
    }
}

#[test]
fn natural_frequencies_ignore_static_loads_and_report_the_limit() {
    let (_d, p) = block(
        json!({"kind":"modal","modes":2,"loads":[{"kind":"force","face":face([-1,0,0]),"newtons":[-50,0,0]}]}),
    );
    let output = run_analysis(&p, "study", None).unwrap();
    assert!(
        output.report["limits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str().unwrap().contains("preload"))
    );
}
#[test]
fn thermal_study_with_stress_reports_both() {
    let (_d, p) = block(json!({"kind":"thermal","conditions":[
        {"kind":"temperature","celsius":90,"face":face([-1,0,0])},{"kind":"temperature","celsius":-10,"face":face([1,0,0])}],
        "stress":true,"reference_c":20,"loads":[]}));
    let a = run_analysis(&p, "study", None).unwrap();
    assert_eq!(a.report["min_c"], -10.);
    assert_eq!(a.report["max_c"], 90.);
    assert!(a.report["max_von_mises_mpa"].as_f64().unwrap() > 0.);
    assert_eq!(a.fields.len(), 2);
}
#[test]
fn shape_study_saves_and_reopens_a_mesh_part_in_one_revision() {
    check_saved_shape(0.70, 1);
}

#[test]
fn a_disconnected_result_refuses_a_static_study_with_one_fixture() {
    check_saved_shape(0.58, 2);
}

fn check_saved_shape(target: f64, pieces: usize) {
    let (d, mut p) = block(json!({"kind":"shape","target_volume":target,"min_member_mm":4}));
    let rev = p.data.revision;
    let a = run_analysis(&p, "study", None).unwrap();
    assert_eq!(a.report["pieces"], pieces);
    assert_eq!(p.data.revision, rev);
    assert_eq!(p.data.parts.len(), 1);
    let mesh = a.shape_mesh.unwrap();
    let inspected = faber_meshtools::try_report(&mesh).unwrap();
    let volume = inspected
        .volume_mm3
        .expect("The result mesh must be closed");
    let reported = a.report["mesh_volume_mm3"]
        .as_f64()
        .expect("Report the volume of the result mesh");
    assert!((reported - volume).abs() < 1e-8 * volume);
    let mass = a.report["mesh_mass_g"].as_f64().unwrap();
    assert!((mass - volume * 7.85 / 1000.).abs() < 1e-8 * mass);
    assert!(save_shape(&mut p, "block", &mesh, rev - 1).is_err());
    assert_eq!(
        save_shape(&mut p, "block", &mesh, rev).unwrap(),
        "block-shape"
    );
    assert_eq!(p.data.revision, rev + 1);
    assert!(save_shape(&mut p, "block", &mesh, rev + 1).is_err());
    drop(p);
    let mut p = faber_wiring::open(d.path(), false, None).unwrap();
    assert!(p.imported_mesh("block-shape").unwrap().triangles.len() > 8);
    let mut study = json!({"part":"block-shape","resolution_mm":2,"kind":"static","loads":[{"kind":"gravity","direction":[0,0,-1]}],"fixtures":[]});
    // Select one flat mesh facet by its centre in part coordinates.
    let mesh = p.imported_mesh("block-shape").unwrap();
    let t = mesh.triangles[0].map(|i| mesh.vertices[i as usize]);
    let centre = std::array::from_fn::<_, 3, _>(|i| (t[0][i] + t[1][i] + t[2][i]) / 3.);
    study["fixtures"] =
        json!([{"kind":"fixed","face":{"near_centre":centre,"tolerance":0.000001,"expect":1}}]);
    p.set_study("verify".into(), serde_json::from_value(study).unwrap())
        .unwrap();
    let check = run_analysis(&p, "verify", None);
    if pieces == 1 {
        assert!(check.unwrap().report["max_von_mises_mpa"].as_f64().unwrap() > 0.);
    } else {
        assert!(check.unwrap_err().contains("no connection to a fixture"));
    }
}

#[test]
fn mode_fields_share_one_refined_surface_and_keep_their_own_displacements() {
    let (_d, p) = block(json!({"kind":"modal","modes":3,"loads":[]}));
    let output = run_analysis(&p, "study", None).unwrap();
    let first = output.fields[0].surface.as_ref().unwrap();
    let second = output.fields[1].surface.as_ref().unwrap();
    assert!(first.mesh.vertices.len() > output.fields[0].vertex_displacement.len());
    assert_eq!(
        first.mesh.vertices.as_ptr(),
        second.mesh.vertices.as_ptr(),
        "The modes must share the display geometry"
    );
    assert_eq!(
        first.face_of_triangle.as_ptr(),
        second.face_of_triangle.as_ptr()
    );
    assert_ne!(first.vertex_displacement, second.vertex_displacement);
}

#[test]
fn a_full_mode_report_contains_only_scalar_results() {
    let (_d, p) = block(json!({"kind":"modal","modes":32,"loads":[]}));
    let a = run_analysis(&p, "study", None).unwrap();
    assert_eq!(a.fields.len(), 32);
    assert_eq!(a.report["modes"].as_array().unwrap().len(), 32);
    let bytes = a.report.to_string().len();
    assert!(bytes < 2500, "The 32-mode report has {bytes} bytes");
}
