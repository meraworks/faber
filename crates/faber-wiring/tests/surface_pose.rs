use faber_wiring::surface::{Request, analyse};
use faber_wiring::surface_pose::analyse_posed;
use glam::Mat4;
use std::collections::BTreeMap;

#[test]
fn draft_uses_the_current_world_pull_and_returns_paint_on_the_rest_mesh() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Motion".into())).unwrap();
    p.put_part::<f64>(
        "part".into(),
        serde_json::from_value(serde_json::json!({
            "name":"Part","colour":[100,150,200],"geometry":{"op":"box","size":[1,2,3]}
        }))
        .unwrap(),
    )
    .unwrap();
    let request = Request::Draft {
        pull: [0., 0., 1.],
        min_deg: 1.,
    };
    let rest = analyse(&p.bodies, "part", &request).unwrap();
    assert_eq!(rest.report["vertical_mm2"], 18.);
    let poses = BTreeMap::from([(
        "part".into(),
        Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2),
    )]);
    let rotated = analyse_posed(&p.bodies, "part", &request, &poses).unwrap();
    assert!((rotated.report["vertical_mm2"].as_f64().unwrap() - 10.).abs() < 1e-10);
    assert_eq!(rotated.mesh.mesh.vertices, rest.mesh.mesh.vertices);
    let scale = BTreeMap::from([("part".into(), Mat4::from_scale(glam::vec3(1., 2., 1.)))]);
    assert!(
        analyse_posed(&p.bodies, "part", &request, &scale)
            .unwrap_err()
            .contains("rigid")
    );
}
