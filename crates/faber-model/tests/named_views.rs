use faber_model::{Edit, ProjectData};
use serde_json::json;
#[test]
fn perspective_and_orthographic_views_preserve_the_camera_and_display_state() {
    let original = json!({"schema":1,"name":"Views","revision":0,"parts":{}});
    let mut saved = original.clone();
    let camera = json!({"eye":[1.,2.,3.],"target":[0.,0.,0.],"up":[0.,0.,1.],"ortho_height":25.});
    let view = json!({"camera":camera,"hidden":["cover"],"section":"cut","explode":0.5,"pose":"open","style":"studio","note":"Assembly"});
    saved["views"] = json!({"ortho":view,"iso":{"camera":{"eye":[1.,2.,3.],"target":[0.,0.,0.],"up":[0.,0.,1.],"fov_deg":50.}}});
    let model: ProjectData = serde_json::from_value(saved.clone()).unwrap();
    let out = serde_json::to_value(model).unwrap();
    assert_eq!(out["views"]["ortho"], view);
    assert_eq!(
        out["views"]["iso"]["camera"],
        saved["views"]["iso"]["camera"]
    );
    let empty: ProjectData = serde_json::from_value(original).unwrap();
    assert!(serde_json::to_value(empty).unwrap().get("views").is_none());
    for edit in [
        json!({"tool":"set_view","name":"saved","view":view}),
        json!({"tool":"remove_view","name":"saved"}),
    ] {
        assert_eq!(
            serde_json::to_value(serde_json::from_value::<Edit>(edit.clone()).unwrap()).unwrap(),
            edit
        );
    }
}
#[test]
fn cameras_require_one_projection_and_a_valid_basis() {
    use faber_model::view::{Camera, NamedView};
    let mut c = Camera {
        eye: [1., 2., 3.],
        target: [0.; 3],
        up: [0., 0., 1.],
        fov_deg: Some(55.),
        ortho_height: None,
    };
    assert!(c.validate().is_ok());
    c.ortho_height = Some(20.);
    assert!(c.validate().is_err());
    c.fov_deg = None;
    assert!(c.validate().is_ok());
    c.ortho_height = None;
    assert!(c.validate().is_err());
    c.fov_deg = Some(180.);
    assert!(c.validate().is_err());
    c.fov_deg = Some(60.);
    c.up = c.eye;
    assert!(c.validate().is_err());
    c.up = [0., 0., 1.];
    c.target = c.eye;
    assert!(c.validate().is_err());
    c.eye = [0., 1., 0.];
    c.target = [0.; 3];
    let mut v: NamedView =
        serde_json::from_value(json!({"camera":c,"joints":{"hinge":30.}})).unwrap();
    assert!(v.validate().is_ok());
    v.explode = Some(-0.1);
    assert!(v.validate().is_err());
    v.explode = None;
    v.style = Some("unknown".into());
    assert!(v.validate().is_err());
}
