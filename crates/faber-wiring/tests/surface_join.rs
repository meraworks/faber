use faber_core::Project;
use faber_kernel_occt::OcctKernel;
use faber_ports::{Kernel, Shape};
use serde_json::{Value, json};
use std::f64::consts::PI;

fn curve(id: &str, points: &[[f64; 3]]) -> Value {
    json!({"id":id,"geometry":{"op":"curve","path":{"start":points[0],"segments":points[1..].iter().map(|p|json!({"kind":"line","end":p})).collect::<Vec<_>>()}}})
}
fn square(z: f64) -> [[f64; 3]; 5] {
    [
        [0., 0., z],
        [20., 0., z],
        [20., 20., z],
        [0., 20., z],
        [0., 0., z],
    ]
}
fn patch(id: &str, boundary: &str) -> Value {
    json!({"id":id,"geometry":{"op":"patch","boundary":[{"kind":"curve","feature":boundary}]}})
}
fn part(features: Vec<Value>, output: &str) -> faber_model::Part<faber_model::Scalar> {
    serde_json::from_value(json!({"name":"Shell","colour":[120,180,150],"geometry":{"op":"history","features":features,"output":output}})).unwrap()
}
fn build(
    mut features: Vec<Value>,
    output: Option<&str>,
) -> Result<(Project, tempfile::TempDir), String> {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Surface join".into()))?;
    if output.is_none() {
        features.push(json!({"id":"reference","geometry":{"op":"box","size":[1,1,1]}}));
    }
    p.put_part(
        "shell".into(),
        part(features, output.unwrap_or("reference")),
    )?;
    Ok((p, dir))
}
fn area(p: &Project, id: &str) -> f64 {
    p.feature_topology("shell", id)
        .unwrap()
        .1
        .iter()
        .map(|f| f.area)
        .sum()
}
#[test]
fn patch_of_a_planar_square() {
    let (p, _) = build(vec![curve("rim", &square(0.)), patch("cap", "rim")], None).unwrap();
    assert!((area(&p, "cap") - 400.).abs() < 1e-6);
}
fn tube() -> Vec<Value> {
    vec![
        json!({"id":"circle","geometry":{"op":"sketch","profile":{"outer":{"kind":"circle","centre":[0,0],"radius":10}}}}),
        json!({"id":"tube","geometry":{"op":"surface_extrude","profile":"circle","distance":20}}),
    ]
}
fn tube_cap(tangent: bool) -> Value {
    json!({"id":"cap","geometry":{"op":"patch","boundary":[{"kind":"edge","feature":"tube","edge":{"near_centre":[0,0,20],"radius":10,"tolerance":0.0001,"expect":1}}],"tangent_to":if tangent{vec!["tube"]}else{vec![]}}})
}
#[test]
fn patch_of_a_circle_is_a_disc() {
    let mut f = tube();
    f.push(tube_cap(false));
    let (p, _) = build(f, None).unwrap();
    assert!((area(&p, "cap") / (PI * 100.) - 1.).abs() < 0.001);
}
#[test]
fn tangent_patch_closes_a_tube_end() {
    let mut f = tube();
    f.push(tube_cap(true));
    let (p, _) = build(f, None).unwrap();
    assert!(area(&p, "cap") > PI * 100. + 1.);
}
fn bent() -> Vec<Value> {
    let mut pts = square(0.);
    pts[1][2] = 5.;
    pts[3][2] = 5.;
    vec![curve("rim", &pts), patch("cap", "rim")]
}
#[test]
fn patch_of_a_bent_boundary() {
    let (p, _) = build(bent(), None).unwrap();
    assert!(area(&p, "cap") > 400.);
    let edges = p.feature_topology("shell", "cap").unwrap().0;
    assert_eq!(edges.len(), 4);
    for mid in [
        [10., 0., 2.5],
        [20., 10., 2.5],
        [10., 20., 2.5],
        [0., 10., 2.5],
    ] {
        assert!(
            edges
                .iter()
                .any(|e| e.centre.iter().zip(mid).all(|(a, b)| (a - b).abs() < 0.001))
        );
    }
}
#[test]
fn open_boundary_is_refused() {
    let f = vec![
        curve(
            "rim",
            &[
                [10., 2.5, 0.],
                [10., 10., 0.],
                [0., 10., 0.],
                [0., 0., 0.],
                [10., 0., 0.],
            ],
        ),
        patch("cap", "rim"),
    ];
    let err = build(f, None).err().unwrap();
    assert!(
        err.contains("The boundary has a gap of 2.5 mm at (10, 0, 0)."),
        "{err}"
    );
}
fn box_shell(lid: bool, solid: bool) -> Vec<Value> {
    let mut v = vec![
        curve("lower", &square(0.)),
        json!({"id":"skin","geometry":{"op":"surface_extrude","profile":"lower","direction":[0,0,1],"distance":10}}),
        patch("bottom", "lower"),
    ];
    if lid {
        v.extend([curve("upper", &square(10.)), patch("top", "upper")]);
    }
    v.push(json!({"id":"joined","geometry":{"op":"stitch","surfaces":if lid {vec!["skin","bottom","top"]}else{vec!["skin","bottom"]},"as_solid":solid}}));
    v
}
#[test]
fn box_from_six_faces() {
    let (p, _) = build(box_shell(true, true), Some("joined")).unwrap();
    assert!((p.bodies[0].volume - 4000.).abs() < 1e-6);
}
#[test]
fn stitched_solid_takes_a_fillet() {
    let mut v = box_shell(true, true);
    v.push(json!({"id":"round","geometry":{"op":"fillet","source":"joined","edges":{"near_centre":[0,0,5],"expect":1,"tolerance":0.001},"radius":2}}));
    let (p, _) = build(v, Some("round")).unwrap();
    assert!(p.bodies[0].volume < 4000.);
}
#[test]
fn open_shell_is_not_a_solid() {
    let err = build(box_shell(false, true), Some("joined")).err().unwrap();
    assert!(
        err.contains("The stitched shell has 4 open edges. It is not a solid."),
        "{err}"
    );
}
fn faces(n: usize) -> Vec<Shape> {
    let k = OcctKernel;
    let body = k.box_shape([20., 20., 10.]).unwrap();
    (1..=n)
        .map(|id| k.face_shape(&body, id as u32).unwrap())
        .collect()
}
#[test]
fn open_shell_is_a_surface_without_as_solid() {
    let joined = OcctKernel.stitch(&faces(5), 0.001, false).unwrap();
    assert_eq!(joined.open_edges, 4);
    assert!(!joined.solid);
}
#[test]
fn inside_out_shell_is_turned() {
    let mut f = vec![];
    let [a, b, c, d, e, g, h, i] = [
        [0., 0., 0.],
        [20., 0., 0.],
        [20., 20., 0.],
        [0., 20., 0.],
        [0., 0., 10.],
        [20., 0., 10.],
        [20., 20., 10.],
        [0., 20., 10.],
    ];
    let mut ids = vec![];
    for (n, rim) in [
        [a, b, c, d, a],
        [e, i, h, g, e],
        [a, e, g, b, a],
        [b, g, h, c, b],
        [c, h, i, d, c],
        [d, i, e, a, d],
    ]
    .iter()
    .enumerate()
    {
        let r = format!("r{n}");
        let p = format!("p{n}");
        f.extend([curve(&r, rim), patch(&p, &r)]);
        ids.push(p);
    }
    f.push(json!({"id":"joined","geometry":{"op":"stitch","surfaces":ids,"as_solid":true}}));
    let (p, _) = build(f, Some("joined")).unwrap();
    assert!((p.bodies[0].volume - 4000.).abs() < 1e-6);
    for face in p.feature_topology("shell", "joined").unwrap().1 {
        let normal = face.normal.unwrap();
        let outward = face
            .centre
            .into_iter()
            .zip([10., 10., 5.])
            .zip(normal)
            .map(|((pos, centre), n)| (pos - centre) * n)
            .sum::<f64>();
        assert!(
            outward > 0.,
            "The normal points into the box at {:?}",
            face.centre
        );
    }
}
#[test]
fn gap_above_the_tolerance_stays_open() {
    let k = OcctKernel;
    let face = |x: f64| {
        k.sketch(
            &serde_json::from_value(
                json!({"outer":{"kind":"rectangle","origin":[x,0],"size":[20,20]}}),
            )
            .unwrap(),
            &Default::default(),
        )
        .unwrap()
    };
    let pair = [face(0.), face(20.01)];
    let apart = k.stitch(&pair, 0.001, false).unwrap();
    assert_eq!(apart.open_edges, 8);
    assert_eq!(apart.largest_gap, 0.);
    let joined = k.stitch(&pair, 0.02, false).unwrap();
    assert_eq!(joined.open_edges, 6);
    assert!(
        (joined.largest_gap - 0.01).abs() < 1e-6,
        "{}",
        joined.largest_gap
    );
}
#[test]
fn thicken_of_a_patch_is_a_solid() {
    let mut v = bent();
    v.push(json!({"id":"wall","geometry":{"op":"thicken","source":"cap","thickness":2}}));
    let (p, _) = build(v, Some("wall")).unwrap();
    assert!((p.bodies[0].volume / (2. * area(&p, "cap")) - 1.).abs() < 0.02);
}
#[test]
fn a_partial_seam_measures_only_the_length_that_was_joined() {
    let k = OcctKernel;
    let panel = |x: f64, y: f64, w: f64, h: f64| {
        k.sketch(
            &serde_json::from_value(json!({
                "outer":{"kind":"rectangle","origin":[x,y],"size":[w,h]}
            }))
            .unwrap(),
            &Default::default(),
        )
        .unwrap()
    };
    let joined = k
        .stitch(
            &[panel(0., 0., 20., 20.), panel(20.01, 5., 10., 10.)],
            0.02,
            false,
        )
        .unwrap();
    assert_eq!(joined.open_edges, 8);
    assert!(
        (joined.largest_gap - 0.01).abs() < 1e-6,
        "{}",
        joined.largest_gap
    );
}
#[test]
fn a_compound_of_solids_is_not_a_stitch_input() {
    let k = OcctKernel;
    let a = k.box_shape([10.; 3]).unwrap();
    let b = k
        .transform_shape(
            &a,
            &faber_model::Placement {
                translation: [20., 0., 0.],
                ..Default::default()
            },
        )
        .unwrap();
    let compound = k.combine(&a, &b, faber_ports::Combine::Union).unwrap();
    assert!(
        k.stitch(&[compound], 0.001, false)
            .unwrap_err()
            .contains("must be a surface")
    );
}
fn open_line(id: &str, plane: &str, length: Value) -> Value {
    json!({"id":id,"geometry":{"op":"solved_sketch","plane":plane,"sketch":{
       "points":{"a":[0,0],"b":[20,0],"c":[1,5],"d":[2,8]},
       "entities":{"rail":{"kind":"line","a":"a","b":"b"},"guide":{"kind":"line","a":"c","b":"d"}},
       "construction":["guide"],"constraints":[{"kind":"fix","point":"a"},{"kind":"horizontal","line":"rail"},{"kind":"distance_x","a":"a","b":"b","value":length}]
    }}})
}
#[test]
fn open_solved_profile_extrudes_normal_to_its_plane_and_rebuilds() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Open sketch".into())).unwrap();
    p.set_params([("width".into(), "20".into())].into())
        .unwrap();
    let fs = vec![
        json!({"id":"plane","geometry":{"op":"plane","placement":{"translation":[5,2,4],"rotation":[90,0,0]}}}),
        open_line("line", "plane", json!("width")),
        json!({"id":"skin","geometry":{"op":"surface_extrude","profile":"line","distance":3}}),
        json!({"id":"body","geometry":{"op":"thicken","source":"skin","thickness":1}}),
    ];
    p.put_part("shell".into(), part(fs.clone(), "body"))
        .unwrap();
    let b = &p.bodies[0];
    assert!((b.volume - 60.).abs() < 1e-6);
    assert!((b.bounds[0][1] + 1.).abs() < 1e-5);
    assert!((b.bounds[1][1] - 2.).abs() < 1e-5);
    p.set_params([("width".into(), "25".into())].into())
        .unwrap();
    assert!((p.bodies[0].volume - 75.).abs() < 1e-6);
    let mut invalid = fs;
    invalid[2]["geometry"] = json!({"op":"extrude_sketch","source":"line","vector":[0,0,3]});
    let e = p
        .put_part("shell".into(), part(invalid, "skin"))
        .unwrap_err();
    assert!(e.contains("do not close a loop"), "{e}");
}
#[test]
fn revolve_loft_and_sweep_take_open_solved_profiles() {
    let mut features = vec![
        json!({"id":"xy","geometry":{"op":"plane","placement":{}}}),
        open_line("line", "xy", json!(20)),
        json!({"id":"raised","geometry":{"op":"plane","placement":{"translation":[0,0,5]}}}),
        open_line("top", "raised", json!(20)),
        json!({"id":"side","geometry":{"op":"plane","placement":{"translation":[10,0,0],"rotation":[0,-90,0]}}}),
        open_line("meridian", "side", json!(20)),
        json!({"id":"axis","geometry":{"op":"axis","origin":[0,0,0],"direction":[0,0,1]}}),
        json!({"id":"loft","geometry":{"op":"surface_loft","sections":["line","top"],"ruled":true}}),
        json!({"id":"round","geometry":{"op":"surface_revolve","profile":"meridian","axis":"axis","angle_deg":360}}),
    ];
    features.push(json!({"id":"sweep","geometry":{"op":"surface_sweep","profile":"line","path":{"start":[0,0,0],"segments":[{"kind":"line","end":[0,0,5]}]}}}));
    let (p, _) = build(features, None).unwrap();
    assert!((area(&p, "loft") - 100.).abs() < 1e-6);
    assert!((area(&p, "sweep") - 100.).abs() < 1e-6);
    assert!((area(&p, "round") - 400. * PI).abs() < 1e-5);
}
#[test]
fn an_open_solved_arc_stays_circular() {
    let spec = json!({"id":"arc","geometry":{"op":"solved_sketch","plane":"work","sketch":{
    "points":{"c":[0,0],"a":[10,0],"b":[0,10]},
    "entities":{"side":{"kind":"arc","centre":"c","start":"a","end":"b"}}}}});
    let (p, _) = build(
        vec![
            json!({"id":"work","geometry":{"op":"plane","placement":{}}}),
            spec,
            json!({"id":"wall","geometry":{"op":"surface_extrude","profile":"arc","distance":4}}),
        ],
        None,
    )
    .unwrap();
    assert!((area(&p, "wall") - 20. * PI).abs() < 1e-6);
    assert!(
        (p.feature_topology("shell", "wall").unwrap().1[0]
            .cylinder
            .as_ref()
            .unwrap()
            .radius
            - 10.)
            .abs()
            < 1e-7
    );
}
