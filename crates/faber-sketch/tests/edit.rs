use faber_model::{Scalar, SketchConstraint as C, SketchEntity as E, SolvedSketchData as Sketch};
use faber_sketch::edit::{SketchOp, apply, apply_parametric};
use faber_sketch::{Status, solve};

fn lines(points: &[(&str, [f64; 2])], segments: &[(&str, &str, &str)]) -> Sketch {
    Sketch {
        points: points.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        entities: segments
            .iter()
            .map(|(id, a, b)| {
                (
                    id.to_string(),
                    E::Line {
                        a: a.to_string(),
                        b: b.to_string(),
                    },
                )
            })
            .collect(),
        ..Default::default()
    }
}
fn rect() -> Sketch {
    let mut s = lines(
        &[
            ("p", [0., 0.]),
            ("q", [40., 0.]),
            ("r", [40., 20.]),
            ("s", [0., 20.]),
        ],
        &[
            ("south", "p", "q"),
            ("east", "q", "r"),
            ("north", "r", "s"),
            ("west", "s", "p"),
        ],
    );
    for id in ["p", "q", "r", "s"] {
        s.constraints.push(C::Fix { point: id.into() });
    }
    s
}
fn visible(s: &Sketch) -> Vec<&E> {
    s.entities
        .iter()
        .filter(|(id, _)| !s.construction.contains(id))
        .map(|(_, e)| e)
        .collect()
}
#[test]
fn fillet_of_a_rectangle_corner() {
    let s = apply(
        &rect(),
        &[SketchOp::Fillet {
            corner: "q".into(),
            radius: 5.,
        }],
    )
    .unwrap();
    assert_eq!(visible(&s).len(), 5);
    let sol = solve(&s).unwrap();
    assert_eq!(sol.status, Status::Solved);
    assert_eq!(sol.dof, 0);
    let loops = faber_sketch::profile::loops(&s, &sol).unwrap();
    assert!((loops[0].area.abs() - (800. - 25. + std::f64::consts::PI * 25. / 4.)).abs() < 1e-6);
    assert_eq!(
        s.constraints
            .iter()
            .filter(|c| matches!(c, C::Tangent { .. }))
            .count(),
        2
    );
}
#[test]
fn chamfer_cuts_equal_lengths_and_refuses_an_oversize_corner() {
    let s = rect();
    let result = apply(
        &s,
        &[SketchOp::Chamfer {
            corner: "p".into(),
            distance: 4.,
        }],
    )
    .unwrap();
    let sol = solve(&result).unwrap();
    assert_eq!(sol.dof, 0);
    assert!(
        (faber_sketch::profile::loops(&result, &sol).unwrap()[0]
            .area
            .abs()
            - 792.)
            .abs()
            < 1e-6
    );
    assert!(
        apply(
            &s,
            &[SketchOp::Chamfer {
                corner: "p".into(),
                distance: 21.
            }]
        )
        .is_err()
    );
}
#[test]
fn trim_between_two_crossings() {
    let mut s = lines(
        &[("left", [-10., 0.]), ("right", [10., 0.]), ("c", [0., 0.])],
        &[("line", "left", "right")],
    );
    s.entities.insert(
        "circle".into(),
        E::Circle {
            centre: "c".into(),
            radius: 3.,
        },
    );
    let edited = apply(
        &s,
        &[SketchOp::Trim {
            entity: "line".into(),
            near: [0.5, 0.],
        }],
    )
    .unwrap();
    assert_eq!(
        visible(&edited)
            .iter()
            .filter(|e| matches!(e, E::Line { .. }))
            .count(),
        2
    );
    let mut ends = edited
        .constraints
        .iter()
        .filter_map(|c| match c {
            C::PointOnCircle { point, circle } if circle == "circle" => {
                Some(edited.points[point][0])
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    ends.sort_by(f64::total_cmp);
    assert_eq!(ends, [-3., 3.]);
}
#[test]
fn extend_to_the_next_line() {
    let s = lines(
        &[
            ("a", [0., 0.]),
            ("b", [2., 0.]),
            ("p", [4., -1.]),
            ("q", [4., 1.]),
            ("r", [7., -1.]),
            ("s", [7., 1.]),
        ],
        &[("ray", "a", "b"), ("near", "p", "q"), ("far", "r", "s")],
    );
    let e = apply(
        &s,
        &[SketchOp::Extend {
            line: "ray".into(),
            end: "b".into(),
        }],
    )
    .unwrap();
    let E::Line { b, .. } = &e.entities["ray"] else {
        panic!()
    };
    assert_eq!(e.points[b], [4., 0.]);
    assert!(
        e.constraints
            .iter()
            .any(|c| matches!(c,C::PointOnLine{point,line} if point==b && line=="near"))
    );
}
#[test]
fn mirror_adds_symmetric_constraints() {
    let mut s = lines(
        &[
            ("a", [2., 1.]),
            ("b", [3., 2.]),
            ("i", [0., 0.]),
            ("j", [0., 10.]),
        ],
        &[("line", "a", "b"), ("axis", "i", "j")],
    );
    for p in ["a", "b", "i", "j"] {
        s.constraints.push(C::Fix { point: p.into() });
    }
    let mut e = apply(
        &s,
        &[SketchOp::Mirror {
            entities: vec!["line".into()],
            about: "axis".into(),
        }],
    )
    .unwrap();
    let mirror = e
        .constraints
        .iter()
        .find_map(|c| match c {
            C::Symmetric { a, b, .. } if a == "b" => Some(b.clone()),
            _ => None,
        })
        .unwrap();
    e.points.insert("b".into(), [6., 4.]);
    let solved = solve(&e).unwrap();
    let [x, y] = solved.points[&mirror];
    assert!((x + 6.).abs() < 1e-8 && (y - 4.).abs() < 1e-8);
}
#[test]
fn patterns_share_copied_endpoints_and_keep_construction_flags() {
    let mut s = lines(
        &[("a", [0., 0.]), ("b", [3., 0.]), ("c", [3., 1.])],
        &[("first", "a", "b"), ("second", "b", "c")],
    );
    s.construction.push("second".into());
    let e = apply(
        &s,
        &[SketchOp::Pattern {
            entities: vec!["first".into(), "second".into()],
            count: 3,
            step: [5., 0.],
        }],
    )
    .unwrap();
    assert_eq!(e.points.len(), 9);
    assert_eq!(e.entities.len(), 6);
    assert_eq!(e.construction.len(), 3);
}

#[test]
fn a_one_member_pattern_keeps_the_source() {
    let s = rect();
    let e = apply(
        &s,
        &[SketchOp::Pattern {
            entities: vec!["east".into()],
            count: 1,
            step: [0.; 2],
        }],
    )
    .unwrap();
    assert_eq!(e, s);
}
#[test]
fn adding_and_removing_geometry_preserves_unrelated_expressions() {
    let mut s = Sketch::<Scalar>::default();
    s.points.insert(
        "p".into(),
        [Scalar::Expression("width / 2".into()), Scalar::Number(0.)],
    );
    let ops = [
        SketchOp::AddPoint {
            id: "q".into(),
            at: [4., 5.].map(Scalar::Number),
        },
        SketchOp::AddEntity {
            id: "line".into(),
            entity: E::Line {
                a: "p".into(),
                b: "q".into(),
            },
            construction: true,
        },
        SketchOp::AddConstraint {
            constraint: C::Horizontal {
                line: "line".into(),
            },
        },
        SketchOp::Remove { id: "line".into() },
    ];
    let e = apply_parametric(&s, &ops, &mut |v| {
        Ok(match v {
            Scalar::Number(n) => *n,
            _ => 10.,
        })
    })
    .unwrap();
    assert_eq!(e.points["p"], s.points["p"]);
    assert!(e.entities.is_empty());
    assert!(e.constraints.is_empty());
    assert!(e.construction.is_empty());
    assert!(apply_parametric(&s, &[ops[0].clone(), ops[0].clone()], &mut |_| Ok(0.)).is_err());
    assert_eq!(s.points.len(), 1);
}

#[test]
fn pattern_spacing_remains_a_parameter_after_an_edit() {
    let s = lines(&[("a", [0., 0.]), ("b", [1., 1.])], &[("l", "a", "b")])
        .map_numbers(&mut |n| Ok(Scalar::Number(*n)))
        .unwrap();
    let op = SketchOp::Pattern {
        entities: vec!["l".into()],
        count: 3,
        step: [Scalar::Expression("pitch".into()), Scalar::Number(0.)],
    };
    let edited = apply_parametric(&s, &[op], &mut |n| {
        Ok(match n {
            Scalar::Number(n) => *n,
            Scalar::Expression(_) => 5.,
        })
    })
    .unwrap();
    let steps: Vec<_> = edited
        .constraints
        .iter()
        .filter_map(|c| match c {
            C::DistanceX { a, value, .. } if a == "a" => Some(value),
            _ => None,
        })
        .collect();
    assert!(
        steps
            .iter()
            .all(|v| matches!(v,Scalar::Expression(e) if e.contains("pitch")))
    );
    assert_eq!(steps.len(), 2);
}

#[test]
fn a_corner_can_join_two_distinct_coincident_endpoints() {
    let mut s = rect();
    s.points.insert("q2".into(), s.points["q"]);
    let E::Line { a, .. } = s.entities.get_mut("east").unwrap() else {
        unreachable!()
    };
    *a = "q2".into();
    s.constraints.push(C::Coincident {
        a: "q".into(),
        b: "q2".into(),
    });
    let e = apply(
        &s,
        &[SketchOp::Fillet {
            corner: "q".into(),
            radius: 5.,
        }],
    )
    .unwrap();
    let solved = solve(&e).unwrap();
    assert_eq!(solved.dof, 0);
    assert_eq!(faber_sketch::profile::loops(&e, &solved).unwrap().len(), 1);
}

#[test]
fn trimming_a_circle_keeps_the_arc_opposite_the_cursor() {
    let mut s = lines(
        &[("left", [-10., 0.]), ("right", [10., 0.]), ("c", [0., 0.])],
        &[("axis", "left", "right")],
    );
    s.entities.insert(
        "circle".into(),
        E::Circle {
            centre: "c".into(),
            radius: 3.,
        },
    );
    let e = apply(
        &s,
        &[SketchOp::Trim {
            entity: "circle".into(),
            near: [0., 3.],
        }],
    )
    .unwrap();
    let E::Arc { start, end, .. } = &e.entities["circle"] else {
        panic!()
    };
    assert_eq!(e.points[start][0], -3.);
    assert_eq!(e.points[end][0], 3.);
}

#[test]
fn extending_to_an_arc_uses_only_its_counter_clockwise_span() {
    let mut s = lines(
        &[
            ("a", [0., 0.]),
            ("b", [1., 0.]),
            ("c", [4., 0.]),
            ("start", [4., -1.]),
            ("end", [4., 1.]),
        ],
        &[("ray", "a", "b")],
    );
    s.entities.insert(
        "right_half".into(),
        E::Arc {
            centre: "c".into(),
            start: "start".into(),
            end: "end".into(),
        },
    );
    let e = apply(
        &s,
        &[SketchOp::Extend {
            line: "ray".into(),
            end: "b".into(),
        }],
    )
    .unwrap();
    assert_eq!(e.points["b"], [5., 0.]);
}

#[test]
fn adding_fix_uses_the_solved_position_and_keeps_matching_expressions() {
    let mut sketch = lines(&[("a", [0., 0.]), ("b", [40., 0.])], &[("line", "a", "b")]);
    sketch.constraints.extend([
        C::Fix { point: "a".into() },
        C::Horizontal {
            line: "line".into(),
        },
        C::Length {
            line: "line".into(),
            value: 50.,
        },
    ]);
    let mut sketch = sketch.map_numbers(&mut |v| Ok(Scalar::Number(*v))).unwrap();
    sketch.points.get_mut("b").unwrap()[1] = Scalar::Expression("level".into());
    let mut resolve = |s: &Scalar| match s {
        Scalar::Number(n) => Ok(*n),
        Scalar::Expression(e) if e == "level" => Ok(0.),
        _ => Err("Unknown expression.".into()),
    };
    let updated = apply_parametric(
        &sketch,
        &[SketchOp::AddConstraint {
            constraint: C::Fix { point: "b".into() },
        }],
        &mut resolve,
    )
    .unwrap();
    assert_eq!(updated.points["b"][1], Scalar::Expression("level".into()));
    let shape = updated.map_numbers(&mut resolve).unwrap();
    let sol = solve(&shape).unwrap();
    assert_eq!(sol.status, Status::Solved);
    assert!((shape.points["b"][0] - 50.).abs() < 1e-8);
}
