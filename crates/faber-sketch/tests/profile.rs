use faber_sketch::{
    Sketch,
    profile::{loops, regions},
    solve,
};
use serde_json::json;
use std::f64::consts::PI;

fn rectangle() -> Sketch {
    serde_json::from_value(json!({
        "points":{"p1":[0,0],"p2":[40,0],"p3":[40,20],"p4":[0,20]},
        "entities":{"l1":{"kind":"line","a":"p1","b":"p2"},
        "l2":{"kind":"line","a":"p3","b":"p2"},"l3":{"kind":"line","a":"p3","b":"p4"},
        "l4":{"kind":"line","a":"p4","b":"p1"}}
    }))
    .unwrap()
}

#[test]
fn rectangle_with_a_circle_is_one_region_with_one_hole() {
    let mut s = rectangle();
    s.points.insert("c".into(), [20., 10.]);
    s.entities.insert(
        "hole".into(),
        faber_sketch::Entity::Circle {
            centre: "c".into(),
            radius: 5.,
        },
    );
    let contours = loops(&s, &solve(&s).unwrap()).unwrap();
    let profiles = regions(&contours);
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].holes.len(), 1);
    assert!(contours.iter().any(|c| (c.area - 25. * PI).abs() < 1e-9));
}

#[test]
fn open_chain_is_not_a_profile() {
    let mut s = rectangle();
    s.entities.remove("l4");
    assert_eq!(
        loops(&s, &solve(&s).unwrap()).unwrap_err(),
        "The entities l1, l2, l3 do not close a loop. The open ends are at p1 and p4."
    );
}

#[test]
fn construction_lines_are_ignored() {
    let mut s = rectangle();
    s.entities.insert(
        "guide".into(),
        faber_sketch::Entity::Line {
            a: "p1".into(),
            b: "p3".into(),
        },
    );
    s.construction.push("guide".into());
    let contours = loops(&s, &solve(&s).unwrap()).unwrap();
    assert_eq!(contours.len(), 1);
    assert!((contours[0].area - 800.).abs() < 1e-9);
}

#[test]
fn reversed_arc_in_a_slot_keeps_its_exact_area() {
    let s: Sketch = serde_json::from_value(json!({"points":{"a":[0,5],"b":[30,5],"c":[30,-5],"d":[0,-5],"left":[0,0],"right":[30,0]},
        "entities":{"top":{"kind":"line","a":"a","b":"b"},"bottom":{"kind":"line","a":"c","b":"d"},
        "l":{"kind":"arc","centre":"left","start":"a","end":"d"},"r":{"kind":"arc","centre":"right","start":"c","end":"b"}}})).unwrap();
    let contours = loops(&s, &solve(&s).unwrap()).unwrap();
    assert!((contours[0].area - (300. + 25. * PI)).abs() < 1e-9);
}

#[test]
fn crossed_touching_and_duplicate_circles_are_refused() {
    for (x, r) in [(-9., 4.), (6., 4.), (0., 10.)] {
        let s:Sketch=serde_json::from_value(json!({"points":{"a":[0,0],"b":[x,0]},
            "entities":{"outside":{"kind":"circle","centre":"a","radius":10},"inside":{"kind":"circle","centre":"b","radius":r}}})).unwrap();
        let err = loops(&s, &solve(&s).unwrap()).unwrap_err();
        assert!(err.contains("boundaries"), "{err}");
    }
}
