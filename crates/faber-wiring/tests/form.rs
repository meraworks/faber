use faber_core::Project;
use faber_model::{Geometry, Part, Scalar};
use faber_wiring::form::{self, FormOp, Select};
use serde_json::{Value, json};

fn project() -> (tempfile::TempDir, Project) {
    let dir = tempfile::tempdir().unwrap();
    let p = faber_wiring::open(dir.path(), true, Some("Forms".into())).unwrap();
    (dir, p)
}
fn recipe(level: u32, solid: bool) -> Value {
    json!({"op":"form","cage":faber_form::form_box([20.;3],[1;3]),"levels":level,"as_solid":solid})
}
fn part(g: Value) -> Part<Scalar> {
    serde_json::from_value(json!({"name":"Grip","colour":[10,160,160],"geometry":g})).unwrap()
}
#[test]
fn form_part_builds_and_has_a_volume() {
    let (_dir, mut p) = project();
    p.put_part("grip".into(), part(recipe(0, false))).unwrap();
    assert!(p.bodies[0].shape.is_none());
    assert!((p.bodies[0].volume - 8000.).abs() < 1e-7);
    let info = form::info(&p, "grip", None, false).unwrap();
    assert_eq!(info["mesh"]["watertight"], true);
    assert_eq!(info["edges"], 12);
    assert!(info.get("cage").is_none());
}
#[test]
fn solid_form_takes_a_hole_and_a_boolean_difference() {
    let (_dir, mut p) = project();
    let base = recipe(2, true);
    p.put_part("grip".into(), part(base.clone())).unwrap();
    let original = p.bodies[0].volume;
    let first = json!({"id":"base","geometry":base});
    let hole = json!({"id":"hole","geometry":{"op":"hole","source":"base","depth":28,"diameter":4,
      "placement":{"translation":[0,0,24],"rotation":[180,0,0]}}});
    p.put_part(
        "grip".into(),
        part(json!({"op":"history","features":[first,hole],"output":"hole"})),
    )
    .unwrap();
    assert!(p.bodies[0].shape.is_some() && p.bodies[0].volume < original - 75.);
    p.put_part(
        "grip".into(),
        part(json!({"op":"history","features":[first,
      {"id":"tool","geometry":{"op":"box","size":[18,18,28]}},
      {"id":"cut","geometry":{"op":"difference","left":"base","right":"tool"}}],"output":"cut"})),
    )
    .unwrap();
    use faber_ports::Kernel;
    let props = faber_kernel_occt::OcctKernel
        .properties(p.bodies[0].shape.as_ref().unwrap())
        .unwrap();
    assert_eq!(props.solids, 1);
    assert!(props.volume > 0. && props.volume < original);
}
#[test]
fn edit_form_is_one_revision_and_undo_restores_the_cage() {
    let (dir, mut p) = project();
    p.set_params([("span".into(), "10".into())].into()).unwrap();
    let mut g = recipe(0, false);
    g["cage"]["vertices"][1][0] = json!("span");
    p.put_part("grip".into(), part(g)).unwrap();
    let before = p.data.parts["grip"].clone();
    let rev = p.data.revision;
    let ops = [FormOp::Move {
        select: Select::Vertices(vec![0]),
        by: [0., 1., 0.],
    }];
    form::edit_form(&mut p, "grip", None, &ops, rev).unwrap();
    assert_eq!(p.data.revision, rev + 1);
    assert!(form::edit_form(&mut p, "grip", None, &ops, rev).is_err());
    let Geometry::Form { cage, .. } = &p.data.parts["grip"].geometry else {
        panic!()
    };
    assert_eq!(cage.vertices[1][0], Scalar::Expression("span".into()));
    assert_eq!(
        faber_wiring::read_snapshot(dir.path()).unwrap().data.parts,
        p.data.parts
    );
    p.undo().unwrap();
    assert_eq!(p.data.parts["grip"], before);
}
#[test]
fn self_intersecting_form_is_not_a_solid() {
    let (_dir, mut p) = project();
    let mut g = recipe(0, false);
    let verts = g["cage"]["vertices"].as_array_mut().unwrap();
    let top = verts
        .iter_mut()
        .find(|v| v[2].as_f64() == Some(20.))
        .unwrap();
    *top = json!([0., 0., -12.]);
    p.put_part("grip".into(), part(g.clone())).unwrap();
    assert_eq!(
        form::info(&p, "grip", None, false).unwrap()["intersects_itself"],
        true
    );
    g["as_solid"] = json!(true);
    let rev = p.data.revision;
    assert!(
        p.put_part("grip".into(), part(g))
            .unwrap_err()
            .contains("The form intersects itself.")
    );
    assert_eq!(p.data.revision, rev);
}

#[test]
fn creased_form_normals_keep_one_closed_solid() {
    let (_dir, mut p) = project();
    let mut cage = faber_form::form_box([20.; 3], [1; 3]);
    cage.creases = faber_form::Cage::from_data(&cage)
        .unwrap()
        .edges()
        .into_iter()
        .map(|edge| faber_model::Crease {
            edge,
            sharpness: 10.,
        })
        .collect();
    for level in [0, 2] {
        let mut g = recipe(level, true);
        g["cage"] = json!(cage);
        p.put_part("grip".into(), part(g)).unwrap();
        let info = form::info(&p, "grip", None, false).unwrap();
        assert_eq!(info["intersects_itself"], false);
        assert_eq!(info["mesh"]["watertight"], true);
        assert!(p.bodies[0].shape.is_some() && (p.bodies[0].volume - 8000.).abs() < 1e-6);
    }
}
