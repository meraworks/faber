//! Known values from the metric standard tables.

use faber_std::{Fit, metric, sizes};

const EPS: f64 = 1e-9;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn pitch_m3_m6_m8_match_iso_261() {
    assert!(close(metric("M3").unwrap().pitch(), 0.5));
    assert!(close(metric("M6").unwrap().pitch(), 1.0));
    assert!(close(metric("M8").unwrap().pitch(), 1.25));
}

#[test]
fn clearance_m3_fine_medium_coarse_match_iso_273() {
    let m3 = metric("M3").unwrap();
    assert!(close(m3.clearance(Fit::Fine), 3.2));
    assert!(close(m3.clearance(Fit::Medium), 3.4));
    assert!(close(m3.clearance(Fit::Coarse), 3.6));
}

#[test]
fn clearance_m6_and_m8_medium_match_iso_273() {
    assert!(close(metric("M6").unwrap().clearance(Fit::Medium), 6.6));
    assert!(close(metric("M8").unwrap().clearance(Fit::Medium), 9.0));
}

#[test]
fn tap_drill_m3_and_m6_match_nominal_minus_pitch() {
    assert!(close(metric("M3").unwrap().tap_drill(), 2.5));
    assert!(close(metric("M6").unwrap().tap_drill(), 5.0));
}

#[test]
fn socket_head_m3_and_m6_match_iso_4762() {
    let m3 = metric("M3").unwrap().socket_head();
    assert!(close(m3.dk, 5.5));
    assert!(close(m3.k, 3.0));
    let m6 = metric("M6").unwrap().socket_head();
    assert!(close(m6.dk, 10.0));
    assert!(close(m6.k, 6.0));
}

#[test]
fn counterbore_m3_uses_practice_diameter_and_depth_with_allowance() {
    let cb = metric("M3").unwrap().counterbore().unwrap();
    assert!(close(cb.diameter, 6.5));
    assert!(close(cb.depth, 3.0 + 0.2));
}

#[test]
fn counterbore_absent_for_sizes_without_practice_value() {
    assert!(metric("M2").unwrap().counterbore().is_none());
}

#[test]
fn countersink_m3_matches_iso_10642_with_ninety_degrees() {
    let cs = metric("M3").unwrap().countersink().unwrap();
    assert!(close(cs.head_diameter, 6.72));
    assert!(close(cs.angle, 90.0));
}

#[test]
fn countersink_absent_for_sizes_not_tabled() {
    assert!(metric("M2").unwrap().countersink().is_none());
}

#[test]
fn lookup_is_case_insensitive() {
    let upper = metric("M3").unwrap();
    let lower = metric("m3").unwrap();
    assert_eq!(upper.name(), lower.name());
    assert!(close(lower.nominal(), 3.0));
}

#[test]
fn unknown_size_m7_is_none() {
    assert!(metric("M7").is_none());
    assert!(metric("").is_none());
}

#[test]
fn table_has_twelve_sizes_in_increasing_order() {
    let table = sizes();
    assert_eq!(table.len(), 12);
    for pair in table.windows(2) {
        assert!(pair[0].nominal() < pair[1].nominal());
        assert!(pair[0].pitch() < pair[1].pitch());
    }
}

#[test]
fn clearance_rows_increase_fine_medium_coarse() {
    for row in sizes() {
        let fine = row.clearance(Fit::Fine);
        let medium = row.clearance(Fit::Medium);
        let coarse = row.clearance(Fit::Coarse);
        assert!(fine < medium, "{}: fine not below medium", row.name());
        assert!(medium < coarse, "{}: medium not below coarse", row.name());
        assert!(
            row.nominal() < fine,
            "{}: clearance not above nominal",
            row.name()
        );
    }
}
