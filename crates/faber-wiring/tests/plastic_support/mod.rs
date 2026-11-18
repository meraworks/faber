use faber_model::{Part, Scalar};
use serde_json::{Value, json};

pub fn plate(op: Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Plate","colour":[100,140,170],"geometry":{
        "op":"history","output":"attachment","features":[
            {"id":"plate","geometry":{"op":"box","size":[40,40,2]}},
            {"id":"top","geometry":{"op":"plane","placement":{"translation":[0,0,2]}}},
            {"id":"attachment","geometry":op}
        ]
    }}))
    .unwrap()
}

pub fn build(op: Value) -> Result<faber_core::Project, String> {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Attachments".into()))?;
    p.put_part("plate".into(), plate(op))?;
    Ok(p)
}
