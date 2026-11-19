use faber_sketch::{Constraint::*, Entity, Sketch, Status, drag, solve};
fn rectangle(width: bool, height: bool) -> Sketch {
    let mut s = Sketch::default();
    for (p, xy) in [
        ("a", [0., 0.]),
        ("b", [35., 2.]),
        ("c", [43., 18.]),
        ("d", [-2., 23.]),
    ] {
        s.points.insert(p.into(), xy);
    }
    for (id, a, b) in [
        ("ab", "a", "b"),
        ("bc", "b", "c"),
        ("cd", "c", "d"),
        ("da", "d", "a"),
    ] {
        s.entities.insert(
            id.into(),
            Entity::Line {
                a: a.into(),
                b: b.into(),
            },
        );
    }
    s.constraints = vec![
        Fix { point: "a".into() },
        Horizontal { line: "ab".into() },
        Vertical { line: "bc".into() },
        Horizontal { line: "cd".into() },
        Vertical { line: "da".into() },
    ];
    if width {
        s.constraints.push(DistanceX {
            a: "a".into(),
            b: "b".into(),
            value: 40.,
        });
    }
    if height {
        s.constraints.push(DistanceY {
            a: "a".into(),
            b: "d".into(),
            value: 20.,
        });
    }
    s
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}
#[test]
fn rectangle_is_fully_constrained() {
    let r = solve(&rectangle(true, true)).unwrap();
    assert_eq!(r.status, Status::Solved);
    assert_eq!(r.dof, 0);
    for (name, expected) in [
        ("a", [0., 0.]),
        ("b", [40., 0.]),
        ("c", [40., 20.]),
        ("d", [0., 20.]),
    ] {
        for (actual, expected) in r.points[name].into_iter().zip(expected) {
            near(actual, expected);
        }
    }
}
#[test]
fn rectangle_without_height_has_one_freedom() {
    let s = rectangle(true, false);
    let r = solve(&s).unwrap();
    assert_eq!(r.status, Status::Under);
    assert_eq!(r.dof, 1);
    assert_eq!(r.free_points, ["c", "d"]);
}
#[test]
fn contradiction_is_named() {
    let mut s = rectangle(true, true);
    s.constraints.push(DistanceX {
        a: "a".into(),
        b: "b".into(),
        value: 50.,
    });
    let r = solve(&s).unwrap();
    assert_eq!(r.status, Status::Over);
    assert_eq!(r.conflicting, vec![5, 7]);
    assert!(r.residual >= 4.99);
}
#[test]
fn redundant_constraint_is_named() {
    let mut s = rectangle(true, true);
    s.constraints.push(DistanceY {
        a: "a".into(),
        b: "b".into(),
        value: 0.,
    });
    let r = solve(&s).unwrap();
    assert_eq!(r.status, Status::Solved);
    assert!(r.redundant.contains(&7));
    assert!(r.conflicting.is_empty());
}
#[test]
fn unconstrained_sketch_keeps_its_points() {
    let mut s = rectangle(false, false);
    s.constraints.clear();
    let r = solve(&s).unwrap();
    assert_eq!(r.dof, 8);
    assert_eq!(r.points, s.points);
}
#[test]
fn non_positive_radius_is_refused() {
    let mut s = Sketch::default();
    s.points.insert("p".into(), [0., 0.]);
    s.entities.insert(
        "c1".into(),
        Entity::Circle {
            centre: "p".into(),
            radius: 0.,
        },
    );
    assert_eq!(
        solve(&s).unwrap_err(),
        "Radius of circle c1 is 0 mm. It must be above 0."
    );
}
#[test]
fn drag_moves_a_free_point_and_keeps_the_constraints() {
    let s = rectangle(true, false);
    let r = drag(&s, "c", [51., 42.]).unwrap();
    near(r.points["c"][0], 40.);
    near(r.points["c"][1], 42.);
    near(r.points["d"][1], 42.);
    assert_eq!(r.dof, 1);
    near(r.points["a"][0], 0.);
    near(r.points["a"][1], 0.);
}
#[test]
fn start_far_away_still_solves_and_is_deterministic() {
    for seed in 1..=20 {
        let mut s = rectangle(true, true);
        let mut t = seed as u64;
        for (p, xy) in &mut s.points {
            if p == "a" {
                continue;
            }
            for v in xy {
                t = t.wrapping_mul(6364136223846793005).wrapping_add(1);
                *v += ((t >> 32) as f64 / u32::MAX as f64 - 0.5) * 12.;
            }
        }
        let a = solve(&s).unwrap();
        let b = solve(&s).unwrap();
        assert_eq!(a.status, Status::Solved);
        assert_eq!(a.points, b.points);
        near(a.points["c"][0], 40.);
        near(a.points["c"][1], 20.);
    }
}
fn chain() -> Sketch {
    let mut s = Sketch::default();
    for i in 0..=50 {
        let x = i as f64 * 2.;
        s.points.insert(format!("b{i}"), [x, 0.]);
        s.points.insert(format!("t{i}"), [x, 9.]);
        s.entities.insert(
            format!("v{i}"),
            Entity::Line {
                a: format!("b{i}"),
                b: format!("t{i}"),
            },
        );
        s.constraints.push(Vertical {
            line: format!("v{i}"),
        });
    }
    for i in 0..50 {
        for side in ["b", "t"] {
            let l = format!("{side}e{i}");
            s.entities.insert(
                l.clone(),
                Entity::Line {
                    a: format!("{side}{i}"),
                    b: format!("{side}{}", i + 1),
                },
            );
            s.constraints.push(Horizontal { line: l });
        }
        s.constraints.push(DistanceX {
            a: format!("b{i}"),
            b: format!("b{}", i + 1),
            value: 2.,
        });
    }
    s.constraints.push(Fix { point: "b0".into() });
    s
}
#[test]
fn two_hundred_constraints() {
    let mut s = chain();
    s.constraints.push(DistanceY {
        a: "b0".into(),
        b: "t0".into(),
        value: 10.,
    });
    let r = solve(&s).unwrap();
    assert_eq!(r.status, Status::Solved);
    assert!(r.iterations <= 50);
    near(r.points["t50"][1], 10.);
}
#[test]
fn dragging_a_fifty_rectangle_chain_changes_the_common_height() {
    let s = chain();
    for height in [9., 10.5, 12., 11., 9.] {
        let result = drag(&s, "t50", [100., height]).unwrap();
        assert_eq!(result.status, Status::Under);
        assert_eq!(result.dof, 1);
        assert_eq!(result.free_points.len(), 51);
        for i in 0..=50 {
            near(result.points[&format!("t{i}")][1], height);
            near(result.points[&format!("b{i}")][1], 0.);
        }
    }
}
#[test]
fn malformed_references_and_dimensions_are_refused() {
    let mut s = rectangle(true, true);
    s.constraints.push(Length {
        line: "ab".into(),
        value: 0.,
    });
    assert!(solve(&s).is_err());
    s.constraints.pop();
    s.constraints.push(Perpendicular {
        a: "no".into(),
        b: "ab".into(),
    });
    assert!(solve(&s).is_err());
}

#[test]
fn parallel_lines_can_start_exactly_perpendicular_and_reverse() {
    for perpendicular in [false, true] {
        let mut s = Sketch::default();
        for (id, v) in [
            ("a", [0., 0.]),
            ("b", [4., 0.]),
            ("c", [8., 0.]),
            ("d", if perpendicular { [12., 0.] } else { [8., 4.] }),
        ] {
            s.points.insert(id.into(), v);
        }
        for (id, a, b) in [("base", "a", "b"), ("arm", "c", "d")] {
            s.entities.insert(
                id.into(),
                Entity::Line {
                    a: a.into(),
                    b: b.into(),
                },
            );
        }
        for id in ["a", "b", "c"] {
            s.constraints.push(Fix { point: id.into() });
        }
        s.constraints.push(Length {
            line: "arm".into(),
            value: 4.,
        });
        s.constraints.push(if perpendicular {
            Perpendicular {
                a: "base".into(),
                b: "arm".into(),
            }
        } else {
            Parallel {
                a: "base".into(),
                b: "arm".into(),
            }
        });
        let r = solve(&s).unwrap();
        assert_eq!(r.status, Status::Solved);
        let d = r.points["d"];
        if perpendicular {
            near(d[0], 8.);
            near(d[1].abs(), 4.)
        } else {
            near(d[1], 0.);
            near((d[0] - 8.).abs(), 4.)
        }
    }
}

#[test]
fn slot_with_tangent_arcs() {
    let mut s = Sketch::default();
    for (id, v) in [
        ("o", [0., 0.]),
        ("p", [30., 0.]),
        ("a", [0., -5.]),
        ("b", [30., -5.]),
        ("c", [30., 5.]),
        ("d", [0., 5.]),
    ] {
        s.points.insert(id.into(), v);
    }
    for (id, a, b) in [("bottom", "a", "b"), ("top", "c", "d")] {
        s.entities.insert(
            id.into(),
            Entity::Line {
                a: a.into(),
                b: b.into(),
            },
        );
    }
    for (id, c, a, b) in [("right", "p", "b", "c"), ("left", "o", "d", "a")] {
        s.entities.insert(
            id.into(),
            Entity::Arc {
                centre: c.into(),
                start: a.into(),
                end: b.into(),
            },
        );
        s.constraints.push(Radius {
            entity: id.into(),
            value: 5.,
        });
        for l in ["bottom", "top"] {
            s.constraints.push(Tangent {
                a: l.into(),
                b: id.into(),
            });
        }
    }
    s.constraints.push(Length {
        line: "bottom".into(),
        value: 30.,
    });
    let r = solve(&s).unwrap();
    assert!(r.status != Status::Over);
    let loops = faber_sketch::profile::loops(&s, &r).unwrap();
    near(loops[0].area, 300. + std::f64::consts::PI * 25.);
}

#[test]
fn equal_circles_share_a_free_radius_at_fixed_centres() {
    let mut s = Sketch::default();
    for (c, p) in [("a", [0., 0.]), ("b", [8., 0.])] {
        s.points.insert(c.into(), p);
        s.constraints.push(Fix { point: c.into() });
    }
    for (c, p) in [("left", "a"), ("right", "b")] {
        s.entities.insert(
            c.into(),
            Entity::Circle {
                centre: p.into(),
                radius: 3.,
            },
        );
    }
    s.constraints.push(Equal {
        a: "left".into(),
        b: "right".into(),
    });
    let r = solve(&s).unwrap();
    assert_eq!(r.dof, 1);
    assert!(r.free_points.is_empty());
    assert_eq!(r.free_radii, ["left", "right"]);
}

#[test]
fn constraints_cannot_collapse_an_entity() {
    let mut s = Sketch::default();
    s.points.insert("a".into(), [0., 0.]);
    s.points.insert("b".into(), [5., 0.]);
    s.entities.insert(
        "wall".into(),
        Entity::Line {
            a: "a".into(),
            b: "b".into(),
        },
    );
    s.constraints.push(Coincident {
        a: "a".into(),
        b: "b".into(),
    });
    let e = solve(&s).unwrap_err();
    assert!(e.contains("wall") && e.contains("zero length"), "{e}");
}

#[test]
fn point_on_a_quarter_arc_stays_in_its_trimmed_span() {
    let mut s = Sketch::default();
    for (id, xy) in [
        ("c", [0., 0.]),
        ("start", [4., 0.]),
        ("end", [0., 4.]),
        ("point", [-4., 0.]),
    ] {
        s.points.insert(id.into(), xy);
        if id != "point" {
            s.constraints.push(Fix { point: id.into() });
        }
    }
    s.entities.insert(
        "arc".into(),
        Entity::Arc {
            centre: "c".into(),
            start: "start".into(),
            end: "end".into(),
        },
    );
    s.constraints.push(PointOnCircle {
        point: "point".into(),
        circle: "arc".into(),
    });
    let r = solve(&s).unwrap();
    assert_ne!(r.status, Status::Over);
    let [x, y] = r.points["point"];
    near(x.hypot(y), 4.);
    assert!(x >= -1e-9 && y >= -1e-9, "Point outside arc at {x},{y}");
}

#[test]
fn holding_the_pointer_still_preserves_an_underconstrained_pose() {
    let s = rectangle(true, false);
    let base = solve(&s).unwrap();
    let held = drag(&s, "c", base.points["c"]).unwrap();
    for (id, p) in base.points {
        assert_eq!(held.points[&id].map(f64::to_bits), p.map(f64::to_bits));
    }
}

#[test]
fn tangent_at_coincident_but_distinct_arc_and_line_ends_keeps_its_rank() {
    let mut s = Sketch::default();
    for (id, p) in [
        ("centre", [0., 0.]),
        ("arc_end", [0., 3.]),
        ("line_end", [3., 4.]),
        ("arc_start", [3., 0.]),
        ("joint", [3., 0.]),
        ("line_start", [3., 0.]),
    ] {
        s.points.insert(id.into(), p);
    }
    s.entities.insert(
        "arc".into(),
        Entity::Arc {
            centre: "centre".into(),
            start: "arc_start".into(),
            end: "arc_end".into(),
        },
    );
    s.entities.insert(
        "line".into(),
        Entity::Line {
            a: "line_start".into(),
            b: "line_end".into(),
        },
    );
    for id in ["centre", "line_end", "arc_end"] {
        s.constraints.push(Fix { point: id.into() });
    }
    s.constraints.push(Coincident {
        a: "arc_start".into(),
        b: "joint".into(),
    });
    s.constraints.push(Coincident {
        a: "joint".into(),
        b: "line_start".into(),
    });
    s.constraints.push(Tangent {
        a: "arc".into(),
        b: "line".into(),
    });
    let r = solve(&s).unwrap();
    assert_eq!(r.status, Status::Solved);
    assert_eq!(r.dof, 0);
}

#[test]
fn two_arcs_tangent_at_a_common_point_have_no_false_freedom() {
    let mut s = Sketch::default();
    for (id, xy) in [
        ("left", [0., 0.]),
        ("right", [8., 0.]),
        ("left_top", [0., 4.]),
        ("right_top", [8., 4.]),
        ("contact", [4., 0.]),
    ] {
        s.points.insert(id.into(), xy);
        if id != "contact" {
            s.constraints.push(Fix { point: id.into() });
        }
    }
    for (id, c, other) in [("a", "left", "left_top"), ("b", "right", "right_top")] {
        s.entities.insert(
            id.into(),
            Entity::Arc {
                centre: c.into(),
                start: "contact".into(),
                end: other.into(),
            },
        );
    }
    s.constraints.push(Tangent {
        a: "a".into(),
        b: "b".into(),
    });
    let r = solve(&s).unwrap();
    assert_eq!(r.status, Status::Solved);
    assert!(r.free_points.is_empty());
}

#[test]
fn large_conflict_diagnosis_checks_unique_constraints_with_a_budget() {
    let mut s = chain();
    let width = DistanceX {
        a: "b0".into(),
        b: "b1".into(),
        value: 2.,
    };
    for _ in 0..700 {
        s.constraints.push(width.clone());
    }
    let last = s.constraints.len();
    s.constraints.push(DistanceX {
        a: "b0".into(),
        b: "b1".into(),
        value: 4.,
    });
    let r = solve(&s).unwrap();
    assert_eq!(r.status, Status::Over);
    assert_eq!(r.conflicting, [last]);
    assert!(r.diagnosis_limit.is_none(), "{:?}", r.diagnosis_limit);
}
