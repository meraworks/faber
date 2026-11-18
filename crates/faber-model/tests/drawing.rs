use faber_model::{Edit, drawing::Drawing};
use serde_json::json;

#[test]
fn drawing_refs_round_trip_for_all_view_and_dimension_kinds() {
    let vertex = json!({"kind":"vertex","part":"block","near":[12,2,0]});
    let edge =
        json!({"kind":"edge","part":"block","edge":{"radius":5,"tolerance":0.01,"expect":1}});
    let axis = json!({"kind":"cylinder_axis","part":"block","face":{"cylinder_radius":5,"tolerance":0.01,"expect":1}});
    let data = json!({"title":"Mount","sheet":{"size":"a4","orientation":"portrait"},"scale":0.5,"projection":"first",
        "views":{"front":{"at":[52,54],"kind":"base","parts":["block"],"direction":"front"},
                 "top":{"at":[52,110],"kind":"projected","from":"front","side":"top"},
                 "cut":{"at":[151,40],"kind":"section","from":"top","line":[[0,1],[4,1]]},
                 "detail":{"at":[150,110],"kind":"detail","from":"top","radius":8,"centre":[0,0]}},
        "dimensions":{"width":{"view":"front","offset":4,"kind":"linear","a":vertex,"b":axis,"along":"horizontal"},
                      "d":{"view":"top","offset":4,"kind":"diameter","edge":edge},
                      "r":{"view":"top","offset":6,"kind":"radius","edge":edge},
                      "angle":{"view":"front","offset":9,"kind":"angle","a":edge,"b":edge}},
        "notes":[{"at":[8,9],"text":"Dimensions in mm"}],"title_block":{"author":"A"},"parts_list":{"at":[12,14]}});
    let d: Drawing = serde_json::from_value(data.clone()).unwrap();
    d.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<Drawing>(serde_json::to_value(&d).unwrap()).unwrap(),
        d
    );
    for op in [
        json!({"tool":"set_drawing","id":"overview","drawing":data}),
        json!({"tool":"remove_drawing","id":"overview"}),
    ] {
        let edit = serde_json::from_value::<Edit>(op.clone()).unwrap();
        assert_eq!(serde_json::to_value(edit).unwrap()["id"], "overview");
    }
    let mut invalid = data.clone();
    invalid["scale"] = json!(0);
    assert!(
        serde_json::from_value::<Drawing>(invalid)
            .unwrap()
            .validate()
            .is_err()
    );
    let mut invalid = data;
    invalid["views"]["front"]["unexpected"] = json!(5);
    assert!(serde_json::from_value::<Drawing>(invalid).is_err());
}

#[test]
fn overall_dimensions_and_diameter_groups_keep_live_references() {
    let axis = serde_json::json!({"kind":"cylinder_axis","part":"base","face":{"cylinder_radius":5,"tolerance":0.02,"expect":1}});
    let mut input = serde_json::json!({"title":"Plate","views":{"front":{"kind":"base","direction":"front","at":[20,30]}},
        "dimensions":{"width":{"view":"front","offset":-6,"kind":"overall","along":"horizontal"},
                      "holes":{"view":"front","offset":6,"kind":"diameter_group","edges":[axis,axis]}}});
    let drawing: Drawing = serde_json::from_value(input.clone()).unwrap();
    drawing.validate().unwrap();
    assert_eq!(
        serde_json::from_value::<Drawing>(serde_json::to_value(&drawing).unwrap()).unwrap(),
        drawing
    );
    input["dimensions"]["width"]["along"] = serde_json::json!("aligned");
    assert!(
        serde_json::from_value::<Drawing>(input.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    input["dimensions"].as_object_mut().unwrap().remove("width");
    input["dimensions"]["holes"]["edges"] = serde_json::json!([axis]);
    assert!(
        serde_json::from_value::<Drawing>(input)
            .unwrap()
            .validate()
            .is_err()
    );
}
