#[test]
fn insert_table_is_plausible() {
    for designation in ["M2-insert", "M3-insert", "M4-insert", "M5-insert"] {
        let row = faber_std::insert_hole(designation).unwrap();
        assert_eq!(row.designation, designation);
        assert!(
            row.hole_diameter > 0. && row.depth > 0. && row.hole_diameter < row.least_boss_diameter
        );
    }
    assert!(faber_std::insert_hole("missing").is_none());
    let m3 = faber_std::insert_hole("M3-insert").unwrap();
    assert!((m3.hole_diameter - 3.99).abs() < 1e-8);
    assert!((m3.depth - 6.74).abs() < 1e-8);
    assert!((m3.least_boss_diameter - 9.5).abs() < 1e-8);
}

#[test]
fn snap_limit_uses_a_printed_specimen_and_can_be_unknown() {
    for id in ["abs", "asa", "pa12", "pc", "petg", "pla"] {
        let row = faber_std::materials::material(id).unwrap();
        let estimate = 0.5 * row.yield_mpa * row.layer_strength_factor / row.youngs_mpa;
        assert!((row.snap_strain_limit.unwrap() - estimate).abs() < 1e-9);
    }
    for id in ["tpu-95a", "steel-s235"] {
        assert_eq!(
            faber_std::materials::material(id)
                .unwrap()
                .snap_strain_limit,
            None
        );
    }
}
