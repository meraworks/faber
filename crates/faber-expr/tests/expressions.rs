use faber_expr::evaluate;

fn value(source: &str) -> Result<f64, String> {
    evaluate(source, |name| match name {
        "width" => Ok(30.),
        _ => Err(format!("Unknown parameter: {name}")),
    })
}

#[test]
fn dependency_check_reads_both_branches_without_dividing_by_a_placeholder() {
    let names = faber_expr::dependencies("if(size > 0, width / size, sqrt(height)) + pi").unwrap();
    assert_eq!(
        names.into_iter().collect::<Vec<_>>(),
        ["height", "size", "width"]
    );
    assert!(faber_expr::dependencies("if(size, width, 2 +)").is_err());
    assert!(faber_expr::dependencies("unknown_function(width)").is_err());
}

#[test]
fn comparisons_drive_conditional_dimensions_after_arithmetic() {
    for (source, expected) in [
        ("if(width >= 30, 4, 2)", 4.),
        ("width + 2 > 31", 1.),
        ("width < 30", 0.),
        ("width <= 30", 1.),
        ("width == 30", 1.),
        ("width != 30", 0.),
        ("2^3 == 8", 1.),
    ] {
        assert_eq!(value(source).unwrap(), expected, "{source}");
    }
    for source in ["1 < 2 < 3", "1 = 1", "2 ! 3", "2 >=", "3 <> 4"] {
        assert!(value(source).is_err(), "{source}");
    }
}

#[test]
fn conditional_evaluates_only_the_selected_branch_but_checks_all_syntax() {
    for (source, expected) in [
        ("if(1, 4, 1/0)", 4.),
        ("if(0, sqrt(-1), 5)", 5.),
        ("if(1, if(0, missing, 6), missing)", 6.),
        ("if(width != 0, 60 / width, 0)", 2.),
    ] {
        assert_eq!(value(source).unwrap(), expected, "{source}");
    }
    for source in [
        "if(1, 4, 2+)",
        "if(0, sin(1,2), 5)",
        "if(1,4,what(2))",
        "if(1, 4)",
        "if(0, 4, missing)",
    ] {
        assert!(value(source).is_err(), "{source}");
    }
}

#[test]
fn guarded_parameter_dependencies_change_with_the_condition() {
    let mut expressions = std::collections::BTreeMap::from([
        ("divisor".into(), "0".into()),
        ("height".into(), "if(divisor == 0, 10, 60 / divisor)".into()),
    ]);
    assert_eq!(faber_expr::resolve(&expressions).unwrap()["height"], 10.);
    expressions.insert("divisor".into(), "3".into());
    assert_eq!(faber_expr::resolve(&expressions).unwrap()["height"], 20.);
    expressions.insert("height".into(), "if(divisor == 0, 10, height)".into());
    assert!(
        faber_expr::resolve(&expressions)
            .unwrap_err()
            .contains("cycle")
    );
}

#[test]
fn arithmetic_uses_precedence_names_and_right_associative_powers() {
    for (source, expected) in [
        ("width / 2 + 3 * 4", 27.),
        ("2^3^2", 512.),
        ("-2^2", -4.),
        ("2^-2", 0.25),
        ("(width - 10) / 2", 10.),
        ("1e-3 + .5", 0.501),
    ] {
        assert!(
            (value(source).unwrap() - expected).abs() < 1e-10,
            "{source}"
        );
    }
}

#[test]
fn functions_use_degrees_and_check_arguments() {
    for (source, expected) in [
        ("sin(30) + cos(60)", 1.),
        ("tan(45)", 1.),
        ("sqrt(9) + abs(-2)", 5.),
        ("min(2, 3) + max(4, 8)", 10.),
        ("round(2.7)", 3.),
        ("if(1, 4, 5)", 4.),
    ] {
        assert!(
            (value(source).unwrap() - expected).abs() < 1e-10,
            "{source}"
        );
    }
}

#[test]
fn malformed_input_and_non_finite_results_are_refused() {
    for source in [
        "", "1 2", "1+", "(2", "sqrt(-1)", "1/0", "missing", "sin(1,2)", "what(2)", "1e999", "2**3",
    ] {
        assert!(value(source).is_err(), "{source}");
    }
}

#[test]
fn parameters_resolve_dependencies_and_refuse_cycles() {
    use std::collections::BTreeMap;
    let mut expressions = BTreeMap::from([
        ("wall".into(), "2.4".into()),
        ("inner".into(), "width - 2 * wall".into()),
        ("width".into(), "300".into()),
    ]);
    let values = faber_expr::resolve(&expressions).unwrap();
    assert!((values["inner"] - 295.2).abs() < 1e-10);
    expressions.insert("width".into(), "400".into());
    assert!((faber_expr::resolve(&expressions).unwrap()["inner"] - 395.2).abs() < 1e-10);
    expressions.insert("width".into(), "inner".into());
    assert!(
        faber_expr::resolve(&expressions)
            .unwrap_err()
            .contains("cycle")
    );
    expressions.insert("width".into(), "unknown".into());
    assert!(faber_expr::resolve(&expressions).is_err());
}

#[test]
fn pi_is_a_constant_with_its_full_value() {
    assert!((value("pi").unwrap() - std::f64::consts::PI).abs() < 1e-12);
    assert!((value("2 * pi").unwrap() - std::f64::consts::TAU).abs() < 1e-12);
}

#[test]
fn constant_names_are_case_sensitive_and_suggest_pi() {
    for source in ["PI", "Pi"] {
        let error = value(source).unwrap_err();
        assert!(error.contains("Did you mean `pi`"), "{source}: {error}");
    }
    let error = value("missing").unwrap_err();
    assert!(!error.contains("Did you mean"), "{error}");
}

#[test]
fn inverse_trigonometry_returns_degrees() {
    for (source, expected) in [
        ("asin(0.5)", 30.),
        ("acos(0.5)", 60.),
        ("atan(1)", 45.),
        ("atan(-1)", -45.),
    ] {
        assert!(
            (value(source).unwrap() - expected).abs() < 1e-10,
            "{source}"
        );
    }
}

#[test]
fn atan2_returns_degrees_in_the_correct_quadrant() {
    for (source, expected) in [
        ("atan2(1, -1)", 135.),
        ("atan2(1, 0)", 90.),
        ("atan2(-1, 1)", -45.),
        ("atan2(0, 0)", 0.),
    ] {
        assert!(
            (value(source).unwrap() - expected).abs() < 1e-10,
            "{source}"
        );
    }
}

#[test]
fn asin_and_acos_refuse_arguments_outside_minus_one_to_one() {
    for (source, function) in [("asin(2)", "asin"), ("acos(-1.5)", "acos")] {
        let error = value(source).unwrap_err();
        assert!(error.contains(function), "{source}: {error}");
        assert!(error.contains("-1 to 1"), "{source}: {error}");
    }
}

#[test]
fn unknown_function_error_lists_the_accepted_functions() {
    let error = value("atn(1)").unwrap_err();
    assert!(error.contains("Unknown function `atn`"), "{error}");
    for name in [
        "sin(1)", "cos(1)", "tan(1)", "asin(1)", "acos(1)", "atan(1)", "atan2(2)", "sqrt(1)",
        "abs(1)", "round(1)", "min(2)", "max(2)", "if(3)",
    ] {
        assert!(error.contains(name), "{name} missing from: {error}");
    }
}

#[test]
fn wrong_argument_count_error_lists_the_accepted_functions() {
    let error = value("atan2(1)").unwrap_err();
    assert!(error.contains("atan2"), "{error}");
    assert!(error.contains("atan2(2)"), "{error}");
}

#[test]
fn every_listed_function_accepts_its_argument_count() {
    for (source, count) in [
        ("sin", 1),
        ("cos", 1),
        ("tan", 1),
        ("asin", 1),
        ("acos", 1),
        ("atan", 1),
        ("atan2", 2),
        ("sqrt", 1),
        ("abs", 1),
        ("round", 1),
        ("min", 2),
        ("max", 2),
    ] {
        let arguments = vec!["0.5"; count].join(",");
        let result = value(&format!("{source}({arguments})"));
        assert!(result.is_ok(), "{source}: {result:?}");
    }
}

#[test]
fn reserved_constant_names_are_refused_as_parameters() {
    use std::collections::BTreeMap;
    let expressions = BTreeMap::from([("pi".into(), "3".into())]);
    let error = faber_expr::resolve(&expressions).unwrap_err();
    assert!(error.contains("reserved"), "{error}");
    assert!(error.contains("pi"), "{error}");
}

#[test]
fn constant_names_do_not_clash_with_other_parameter_names() {
    use std::collections::BTreeMap;
    let expressions = BTreeMap::from([
        ("PI".into(), "3".into()),
        ("e".into(), "2".into()),
        ("half".into(), "pi / 2".into()),
    ]);
    let values = faber_expr::resolve(&expressions).unwrap();
    assert_eq!(values["PI"], 3.);
    assert_eq!(values["e"], 2.);
    assert!((values["half"] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
}
