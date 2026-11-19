use super::*;
use crate::{Constraint::*, Entity, Sketch};
fn fixture() -> Sketch {
    let mut s = Sketch::default();
    for (id, p) in [
        ("o", [0., 0.]),
        ("o2", [0., 0.]),
        ("x", [4., 0.]),
        ("y", [0., 4.]),
        ("m", [2., 0.]),
        ("xm", [-4., 0.]),
        ("top", [4., 4.]),
        ("p", [4., 1.]),
        ("q", [4., -1.]),
        ("c2", [8., 0.]),
    ] {
        s.points.insert(id.into(), p);
    }
    for (id, a, b) in [
        ("a", "o", "x"),
        ("b", "o", "y"),
        ("a2", "y", "top"),
        ("a3", "xm", "o"),
        ("t", "x", "top"),
    ] {
        s.entities.insert(
            id.into(),
            Entity::Line {
                a: a.into(),
                b: b.into(),
            },
        );
    }
    for (id, c, r) in [("c", "o", 4.), ("cc", "o", 2.), ("d", "c2", 4.)] {
        s.entities.insert(
            id.into(),
            Entity::Circle {
                centre: c.into(),
                radius: r,
            },
        );
    }
    s.entities.insert(
        "arc".into(),
        Entity::Arc {
            centre: "o".into(),
            start: "x".into(),
            end: "y".into(),
        },
    );
    s.constraints = vec![
        Coincident {
            a: "o".into(),
            b: "o2".into(),
        },
        PointOnLine {
            point: "m".into(),
            line: "a".into(),
        },
        PointOnCircle {
            point: "x".into(),
            circle: "c".into(),
        },
        Midpoint {
            point: "m".into(),
            line: "a".into(),
        },
        Horizontal { line: "a".into() },
        Vertical { line: "b".into() },
        Parallel {
            a: "a".into(),
            b: "a2".into(),
        },
        Perpendicular {
            a: "a".into(),
            b: "b".into(),
        },
        Collinear {
            a: "a".into(),
            b: "a3".into(),
        },
        Tangent {
            a: "t".into(),
            b: "c".into(),
        },
        Tangent {
            a: "c".into(),
            b: "d".into(),
        },
        Equal {
            a: "a".into(),
            b: "b".into(),
        },
        Equal {
            a: "arc".into(),
            b: "d".into(),
        },
        Concentric {
            a: "c".into(),
            b: "cc".into(),
        },
        Symmetric {
            a: "p".into(),
            b: "q".into(),
            about: "a".into(),
        },
        Fix { point: "o".into() },
        Distance {
            a: "x".into(),
            b: "o".into(),
            value: 4.,
        },
        DistanceX {
            a: "o".into(),
            b: "x".into(),
            value: 4.,
        },
        DistanceY {
            a: "o".into(),
            b: "y".into(),
            value: 4.,
        },
        PointLineDistance {
            point: "y".into(),
            line: "a".into(),
            value: 4.,
        },
        Angle {
            a: "a".into(),
            b: "b".into(),
            degrees: 90.,
        },
        Radius {
            entity: "arc".into(),
            value: 4.,
        },
        Diameter {
            entity: "c".into(),
            value: 8.,
        },
        Length {
            line: "t".into(),
            value: 4.,
        },
    ];
    s
}
#[test]
fn residual_is_zero_when_satisfied() {
    let s = fixture();
    let p = Problem::new(&s).unwrap();
    let rows = p.rows(&p.start, None).unwrap();
    assert!(
        rows.iter().all(|r| r.value.f.abs() < 1e-10),
        "{:?}",
        rows.iter().map(|r| r.value.f).collect::<Vec<_>>()
    );
}
#[test]
fn jacobian_matches_finite_differences() {
    let s = fixture();
    let p = Problem::new(&s).unwrap();
    let mut seed = 23004u64;
    for _ in 0..20 {
        let mut x = p.start.clone();
        for v in &mut x {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            *v += ((seed >> 32) as f64 / u32::MAX as f64 - 0.5) * 0.3;
        }
        let rows = p.rows(&x, None).unwrap();
        for j in 0..x.len() {
            let original = x[j];
            x[j] = original + 1e-6;
            let hi = p.rows(&x, None).unwrap();
            x[j] = original - 1e-6;
            let lo = p.rows(&x, None).unwrap();
            x[j] = original;
            for (i, r) in rows.iter().enumerate() {
                let grad = r
                    .value
                    .d
                    .iter()
                    .find(|(col, _)| *col == j)
                    .map_or(0., |v| v.1);
                let fd = (hi[i].value.f - lo[i].value.f) / 2e-6;
                assert!(
                    (fd - grad).abs() < 1e-5,
                    "row {i}, column {j}, {fd} vs {grad}"
                );
            }
        }
    }
}
