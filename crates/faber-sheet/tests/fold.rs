use faber_model::{Flange, Relief, SheetRule, SketchLoop, SketchProfile, SketchSegment};
use faber_sheet::{build_for_flat, fold};
use glam::DVec3;

fn rule() -> SheetRule {
    SheetRule {
        thickness: 2.,
        bend_radius: 2.,
        k_factor: None,
        relief: Relief::None,
    }
}
fn base() -> SketchProfile {
    SketchProfile {
        outer: SketchLoop::Rectangle {
            origin: [0., 0.],
            size: [50., 40.],
        },
        holes: vec![],
    }
}
fn flange(id: &str, on: &str, edge: u32) -> Flange {
    Flange {
        id: id.into(),
        on: on.into(),
        edge,
        length: 30.,
        angle_deg: None,
        inset_start: None,
        inset_end: None,
        holes: vec![],
    }
}
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-8, "{a} compared to {b}");
}

#[test]
fn l_bracket_frames() {
    let f = fold(&rule(), &base(), &[flange("side", "base", 2)]).unwrap();
    let wall = &f.walls[1];
    let bend = &f.bends[0];
    let mut low = DVec3::ZERO;
    let mut high = DVec3::new(50., 40., 2.);
    for p in &wall.outline {
        for z in [0., 2.] {
            let v = wall.frame.transform_point3(DVec3::new(p[0], p[1], z));
            low = low.min(v);
            high = high.max(v);
        }
    }
    assert!((high - low).abs_diff_eq(DVec3::new(54., 40., 34.), 1e-9));
    near(bend.allowance, std::f64::consts::FRAC_PI_2 * 2.88);
    near(bend.width, 40.);
    assert!(
        wall.frame
            .transform_vector3(DVec3::Y)
            .abs_diff_eq(DVec3::Z, 1e-12)
    );
    assert_eq!(bend.line[0], [50., 40., 4.]);
    assert_eq!(f.walls[0].straight_edges[1], (2, [[50., 0.], [50., 40.]]));
}

#[test]
fn deformations_follow_angle_and_parent_even_when_list_is_unordered() {
    let mut a = flange("a", "base", 2);
    a.angle_deg = Some(-45.);
    let mut b = flange("b", "a", 3);
    b.angle_deg = Some(45.);
    let f = fold(&rule(), &base(), &[b, a]).unwrap();
    assert!(f.walls[1].frame.transform_vector3(DVec3::Y).z < -0.70);
    near(f.walls[2].frame.transform_vector3(DVec3::Z).z, 1.);
}

#[test]
fn inset_flange_gets_reliefs() {
    let mut r = rule();
    r.relief = Relief::Rectangular {
        width: 2.,
        depth: 4.,
    };
    let mut flange = flange("side", "base", 2);
    flange.inset_start = Some(5.);
    flange.inset_end = Some(5.);
    let f = fold(&r, &base(), &[flange]).unwrap();
    let wall = &f.walls[0];
    assert_eq!(wall.reliefs.len(), 2);
    assert!(wall.outline.len() >= 12);
    let area = |p: &Vec<[f64; 2]>| {
        p.iter()
            .zip(p.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
            .sum::<f64>()
            .abs()
            / 2.
    };
    near(area(&wall.outline), 1984.);
    near(f.bends[0].width, 30.);
}

#[test]
fn colliding_flanges_are_refused() {
    let fs = [flange("f1", "base", 1), flange("f2", "base", 2)];
    assert_eq!(
        fold(&rule(), &base(), &fs).unwrap_err(),
        "Flanges f1 and f2 overlap at the corner. Add an inset or a relief."
    );
    assert!(build_for_flat(&rule(), &base(), &fs).is_ok());
}

#[test]
fn tight_radius_warns() {
    let mut r = rule();
    r.bend_radius = 1.;
    let f = fold(&r, &base(), &[flange("side", "base", 2)]).unwrap();
    assert!(
        f.warnings
            .iter()
            .any(|s| s.contains("radius") && s.contains('1') && s.contains('2'))
    );
}

#[test]
fn flange_on_an_arc_edge_is_refused() {
    let mut b = base();
    b.outer = SketchLoop::Path {
        start: [0., 0.],
        segments: vec![
            SketchSegment::Line { end: [30., 0.] },
            SketchSegment::Arc {
                via: [40., 10.],
                end: [30., 20.],
            },
            SketchSegment::Line { end: [0., 20.] },
            SketchSegment::Line { end: [0., 0.] },
        ],
    };
    let e = fold(&rule(), &b, &[flange("f", "base", 2)]).unwrap_err();
    assert!(e.contains("straight"), "{e}");
    assert!(fold(&rule(), &b, &[flange("f", "base", 1)]).is_ok());
}

#[test]
fn hole_near_a_bend_warns() {
    let mut b = base();
    b.holes.push(SketchLoop::Circle {
        centre: [45., 22.],
        radius: 2.,
    });
    let f = fold(&rule(), &b, &[flange("side", "base", 2)]).unwrap();
    assert!(
        f.warnings
            .iter()
            .any(|s| s.contains("hole") && s.contains("3.000")),
        "{:?}",
        f.warnings
    );
}

#[test]
fn a_fold_back_through_the_base_is_refused() {
    let mut a = flange("side", "base", 2);
    a.length = 6.;
    let mut b = flange("return", "side", 3);
    b.angle_deg = Some(135.);
    b.length = 80.;
    let err = fold(&rule(), &base(), &[a, b]).unwrap_err();
    assert!(err.contains("return") && err.contains("base"), "{err}");
}

#[test]
fn collinear_flange_seams_can_meet_end_to_end() {
    let b = SketchProfile {
        outer: SketchLoop::Polygon {
            points: vec![[0., 0.], [20., 0.], [40., 0.], [40., 30.], [0., 30.]],
            corner_radius: None,
        },
        holes: vec![],
    };
    let f = fold(
        &rule(),
        &b,
        &[flange("left", "base", 1), flange("right", "base", 2)],
    )
    .unwrap();
    assert_eq!(f.walls.len(), 3);
}

#[test]
fn a_corner_relief_removes_material_at_the_shared_corner() {
    let mut r = rule();
    r.relief = Relief::Round { diameter: 3. };
    let f = fold(
        &r,
        &base(),
        &[flange("front", "base", 1), flange("right", "base", 2)],
    )
    .unwrap();
    assert!(
        f.walls[0]
            .reliefs
            .iter()
            .any(|cut| matches!(cut,SketchLoop::Circle{centre,..} if *centre==[50.,0.]))
    );
}

#[test]
fn each_hole_must_have_material_separating_it_from_the_next() {
    let mut b = base();
    b.holes = vec![
        SketchLoop::Rectangle {
            origin: [5., 5.],
            size: [3., 3.],
        },
        SketchLoop::Rectangle {
            origin: [8., 5.],
            size: [3., 3.],
        },
    ];
    assert!(fold(&rule(), &b, &[]).is_err());
}

#[test]
fn a_circle_has_no_straight_attachable_edge_and_keeps_its_exact_profile() {
    let mut b = base();
    b.outer = SketchLoop::Circle {
        centre: [5., 7.],
        radius: 20.,
    };
    let f = fold(&rule(), &b, &[]).unwrap();
    assert!(f.walls[0].straight_edges.is_empty());
    assert_eq!(f.walls[0].profile, b);
}

#[test]
fn invalid_rule_and_cyclic_parent_are_refused() {
    let mut r = rule();
    r.k_factor = Some(f64::NAN);
    assert!(fold(&r, &base(), &[]).is_err());
    let e = fold(
        &rule(),
        &base(),
        &[flange("a", "b", 1), flange("b", "a", 1)],
    )
    .unwrap_err();
    assert!(e.contains("cycle"));
}
