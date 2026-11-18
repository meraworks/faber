use faber_core::pick_selector::{edge_selector, face_selector};
use faber_model::{Part, Scalar};
use faber_ports::EdgeInfo;
use serde_json::json;

fn project_at(root: &std::path::Path) -> faber_core::Project {
    let mut project = faber_wiring::open(root, true, Some("Pick test".into())).unwrap();
    project
        .set_params([("h".into(), "30".into())].into())
        .unwrap();
    let part: Part<Scalar> = serde_json::from_value(json!({
        "name":"Block", "colour":[100,120,140],
        "placement":{"translation":[40,70,50],"rotation":[90,0,0]},
        "geometry":{"op":"history","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,"h"]}}
        ],"output":"base"}
    }))
    .unwrap();
    project.put_part("p".into(), part).unwrap();
    project
}

#[test]
fn selector_from_four_top_edges_of_a_box() {
    let tmp = tempfile::tempdir().unwrap();
    let p = project_at(tmp.path());
    let (edges, faces) = p.unplaced_topology("p").unwrap();
    let top: Vec<_> = edges
        .iter()
        .filter(|e| (e.centre[2] - 30.).abs() < 1e-6)
        .map(|e| e.index)
        .collect();
    let selector = edge_selector(&edges, &top).unwrap();
    assert_eq!(selector.expect, 4);
    let selector = selector
        .map_numbers(&mut |n| match n {
            Scalar::Number(v) => Ok(*v),
            _ => Err("Expected number".into()),
        })
        .unwrap();
    let found = p
        .selected_topology("p", Some("base"), Some(&selector), None)
        .unwrap()
        .0;
    assert_eq!(found.iter().map(|e| e.index).collect::<Vec<_>>(), top);
    let top_face = faces
        .iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.999))
        .unwrap();
    assert_eq!(
        face_selector(&faces, &[top_face.index])
            .unwrap()
            .normal
            .unwrap(),
        [0., 0., 1.].map(Scalar::Number)
    );
}

#[test]
fn fillet_from_picked_edges_survives_a_resize() {
    let tmp = tempfile::tempdir().unwrap();
    let mut p = project_at(tmp.path());
    let topo = faber_wiring::pick_topology(&p.bodies[0], &p.data).unwrap();
    let top: Vec<_> = topo
        .edges
        .iter()
        .filter(|e| (e.centre[2] - 30.).abs() < 1e-6)
        .map(|e| e.index)
        .collect();
    assert_eq!(top.len(), 4);
    let selector = topo.edge_selector(&top).unwrap();
    let mut part: Part<Scalar> = p.data.parts["p"].clone();
    part.geometry = serde_json::from_value(json!({
        "op":"history","features":[
            {"id":"base","geometry":{"op":"box","size":[10,20,"h"]}},
            {"id":"round","geometry":{"op":"fillet","source":"base","edges":selector,"radius":1}}
        ],"output":"round"
    }))
    .unwrap();
    p.put_part("p".into(), part).unwrap();
    let count = p.topology("p").unwrap().0.len();
    p.set_params([("h".into(), "80".into())].into()).unwrap();
    assert_eq!(p.topology("p").unwrap().0.len(), count);
    assert!(p.bodies[0].volume > 15000. && p.bodies[0].volume < 16000.);
}

#[test]
fn selector_refusal_text() {
    let a = EdgeInfo {
        index: 1,
        length: 5.,
        centre: [0.; 3],
        direction: None,
        radius: Some(1.),
    };
    let mut b = a.clone();
    b.index = 2;
    assert_eq!(
        edge_selector(&[a, b], &[1]).unwrap_err(),
        "The picked edges cannot be described by one selector. Pick fewer edges."
    );
}

#[test]
fn double_click_selects_the_tangent_chain() {
    use faber_ports::EdgeLines;
    let mut lines = EdgeLines::default();
    let mut add = |points: Vec<[f64; 3]>| {
        let start = lines.points.len() as u32;
        lines.points.extend(points);
        lines.ranges.push(start..lines.points.len() as u32);
    };
    let arc = |cx: f64, angle: f64| {
        (0..=64)
            .map(|i| {
                let a = (angle + 180. * i as f64 / 64.).to_radians();
                [cx + a.cos(), a.sin(), 0.]
            })
            .collect()
    };
    add(vec![[0., -1., 0.], [5., -1., 0.]]);
    add(arc(5., -90.));
    add(vec![[5., 1., 0.], [0., 1., 0.]]);
    add(arc(0., 90.));
    add(vec![[0., -1., 0.], [0., -1., 5.]]);
    assert_eq!(
        faber_core::pick_selector::tangent_chain(&lines, 1),
        [1, 2, 3, 4]
    );
}

#[test]
fn face_loop_excludes_a_disconnected_inner_wire() {
    use faber_ports::{EdgeFace, EdgeLines};
    let lines = EdgeLines {
        points: vec![
            [0., 0., 0.],
            [1., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [0., 1., 0.],
            [0., 0., 0.],
            [0.3, 0.3, 0.],
            [0.4, 0.3, 0.],
            [0.3, 0.3, 0.],
        ],
        ranges: vec![0..2, 2..4, 4..6, 6..9],
    };
    let links = (1..=4)
        .map(|edge| EdgeFace { edge, face: 1 })
        .collect::<Vec<_>>();
    assert_eq!(
        faber_core::pick_selector::edge_loop(&lines, &links, 1, 1),
        [1, 2, 3]
    );
}
