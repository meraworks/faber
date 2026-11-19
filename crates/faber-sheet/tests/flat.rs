use faber_model::{Flange, Relief, SheetRule, SketchLoop, SketchProfile};
use faber_ports::vector::Path2;
use faber_sheet::{BendLine, Direction, FlatPattern, flatten, to_vector};
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
            origin: [0.; 2],
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
fn fold(fl: &[Flange]) -> FlatPattern {
    flatten((&rule(), &base(), fl)).unwrap()
}
fn allowance() -> f64 {
    std::f64::consts::FRAC_PI_2 * (2. + 0.44 * 2.)
}
fn only_line(p: &FlatPattern) -> &BendLine {
    assert_eq!(p.bends.len(), 1);
    &p.bends[0]
}
#[test]
fn l_bracket_flat_size() {
    let p = fold(&[flange("f1", "base", 2)]);
    assert!((p.size_mm[0] - 84.5239).abs() < 1e-4);
    assert!((p.size_mm[1] - 40.).abs() < 1e-7);
    let bend = only_line(&p);
    for a in bend.line {
        assert!((a[0] - 50. - allowance() * 0.5).abs() < 1e-7);
    }
    assert!((bend.allowance - allowance()).abs() < 1e-9);
    assert!(p.problems.is_empty());
}
#[test]
fn flat_area_is_the_wall_area_plus_the_bend_zones() {
    let p = fold(&[flange("f1", "base", 2)]);
    assert!((p.area_mm2 - 40. * (80. + allowance())).abs() < 1e-5);
    assert!((p.cut_length_mm - 2. * (120. + allowance())).abs() < 1e-5);
}
#[test]
fn u_channel_has_two_bend_lines() {
    let p = fold(&[flange("f1", "base", 2), flange("f2", "base", 4)]);
    assert_eq!(p.bends.len(), 2);
    assert!((p.size_mm[0] - 110. - 2. * allowance()).abs() < 1e-6);
    assert_eq!(p.outline.len(), 1);
}
#[test]
fn holes_move_with_their_wall() {
    let mut f = flange("f1", "base", 2);
    f.holes.push(SketchLoop::Circle {
        centre: [8., 12.],
        radius: 3.,
    });
    let p = fold(&[f]);
    assert_eq!(p.holes.len(), 1);
    let centre = [50. + allowance() + 12., 32.];
    assert!(
        p.holes[0]
            .iter()
            .all(|v| ((v[0] - centre[0]).hypot(v[1] - centre[1]) - 3.).abs() < 1e-5)
    );
    let cut = &to_vector(&p, "Bracket").layers[0].paths;
    assert!(cut.iter().any(|s|matches!(s,Path2::Circle{centre:c,radius} if (c[0]-centre[0]).abs()<1e-7 && (c[1]-centre[1]).abs()<1e-7 && *radius==3.)));
    assert!((p.area_mm2 - 40. * (80. + allowance()) + 9. * std::f64::consts::PI).abs() < 1e-5);
}
#[test]
fn flat_overlap_is_reported() {
    let mut fl: Vec<_> = (1..=4)
        .map(|edge| {
            let mut f = flange(&format!("f{edge}"), "base", edge);
            f.inset_start = Some(1.);
            f.inset_end = Some(1.);
            f
        })
        .collect();
    fl.extend([
        flange("f5", "f2", 4),
        flange("f6", "f3", 2),
        flange("f7", "f4", 4),
        flange("f8", "f1", 2),
    ]);
    let p = fold(&fl);
    assert!(
        p.problems
            .iter()
            .any(|s| s == "Walls f5 and f6 overlap in the flat pattern."),
        "{:?}",
        p.problems
    );
}
#[test]
fn dxf_has_cut_and_bend_layers() {
    let p = fold(&[flange("f1", "base", 2)]);
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("cut.dxf");
    faber_files::VectorWriter
        .write_flat_pattern(&to_vector(&p, "Sheet"), &path)
        .unwrap();
    let dxf = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    assert_eq!(dxf.matches("0\nPOLYLINE\n8\nCUT\n").count(), 1);
    assert_eq!(dxf.matches("0\nLINE\n8\nBEND\n").count(), 1);
    assert!(dxf.contains("66\n1\n") && dxf.contains("70\n1\n"));
    assert!(dxf.contains("6\nDASHED\n"));
}
#[test]
fn bend_direction_follows_the_sign() {
    let mut f = flange("f", "base", 2);
    f.angle_deg = Some(-45.);
    let p = fold(&[f]);
    assert_eq!(only_line(&p).direction, Direction::Down);
    assert!(only_line(&p).allowance > 0.);
}
#[test]
fn rounded_cut_edges_keep_their_exact_arc_length_and_area() {
    let base = SketchProfile {
        outer: SketchLoop::Rectangle {
            origin: [0.; 2],
            size: [60., 45.],
        },
        holes: vec![SketchLoop::Slot {
            centre: [25., 20.],
            length: 16.,
            width: 4.,
            rotation: 33.,
        }],
    };
    let flat = flatten((&rule(), &base, &[flange("f1", "base", 2)])).unwrap();
    let doc = to_vector(&flat, "");
    let arcs = doc.layers[0]
        .paths
        .iter()
        .filter(|p| matches!(p,Path2::Arc {radius,..} if (*radius-2.).abs()<1e-7))
        .count();
    assert_eq!(arcs, 2);
    let width = 90. + allowance();
    assert!((flat.area_mm2 - (width * 45. - 48. - 4. * std::f64::consts::PI)).abs() < 1e-5);
    assert!(
        (flat.cut_length_mm - (2. * (width + 45.) + 24. + 4. * std::f64::consts::PI)).abs() < 1e-5
    );
}
#[test]
fn flat_pattern_carries_the_manufacturing_warnings() {
    let thin = SheetRule {
        bend_radius: 0.8,
        ..rule()
    };
    let p = flatten((&thin, &base(), &[flange("f1", "base", 2)])).unwrap();
    assert!(!p.warnings.is_empty());
}
