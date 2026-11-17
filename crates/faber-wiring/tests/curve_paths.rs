use faber_model::{Part, Scalar};
use serde_json::json;

#[test]
fn sweep_a_circle_along_an_exact_spatial_arc() {
    let directory = tempfile::tempdir().unwrap();
    let mut project =
        faber_wiring::open(directory.path(), true, Some("Curve path".into())).unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({"name":"Bend","colour":[100,140,170],"geometry":{"op":"history","output":"bend","features":[
        {"id":"section","geometry":{"op":"sketch","placement":{"translation":[10,0,0],"rotation":[90,0,0]},"profile":{"outer":{"kind":"circle","centre":[0,0],"radius":1}}}},
        {"id":"path","geometry":{"op":"curve","path":{"start":[10,0,0],"segments":[{"kind":"arc","via":[7.0710678118654755,7.0710678118654755,0],"end":[0,10,0]}]}}},
        {"id":"bend","geometry":{"op":"sweep_sketch","source":"section","path":"path"}}
    ]}})).unwrap();
    project.put_part("bend".into(), part).unwrap();
    assert!((project.bodies[0].volume - 5. * std::f64::consts::PI.powi(2)).abs() < 1e-5);
    let (_, bodies) = faber_wiring::read(directory.path()).unwrap();
    assert!((bodies[0].volume - project.bodies[0].volume).abs() < 1e-6);
    let mut recipe = serde_json::to_value(&project.data.parts["bend"]).unwrap();
    recipe["geometry"]["features"][0]["geometry"]["placement"]["rotation"] = json!([0, 0, 0]);
    recipe["geometry"]["features"][0]["geometry"]["profile"]["holes"] =
        json!([{"kind":"circle","centre":[0,0],"radius":0.5}]);
    recipe["geometry"]["features"][1]["geometry"]["path"]["segments"][0] =
        json!({"kind":"arc","via":[7.0710678118654755,0,7.0710678118654755],"end":[0,0,10]});
    let part: Part<Scalar> = serde_json::from_value(recipe.clone()).unwrap();
    project.put_part("bend".into(), part).unwrap();
    assert!((project.bodies[0].volume - 3.75 * std::f64::consts::PI.powi(2)).abs() < 1e-5);
    let before = project.data.clone();
    recipe["geometry"]["features"][1]["geometry"]["path"]["segments"][0]["end"] = json!([10, 0, 0]);
    let part: Part<Scalar> = serde_json::from_value(recipe).unwrap();
    assert!(project.put_part("bend".into(), part).is_err());
    assert_eq!(before, project.data);
}
