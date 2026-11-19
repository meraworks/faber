use faber_core::explode::offsets;
use faber_model::motion::MotionStudy;
use faber_ports::Body;
use std::path::Path;

fn block(id: &str, centre: [f64; 3], half: [f64; 3]) -> Body {
    Body {
        id: id.into(),
        name: id.into(),
        group: String::new(),
        colour: [0; 3],
        mesh: Default::default(),
        edge_lines: Default::default(),
        face_of_triangle: vec![],
        volume: 8. * half.iter().product::<f64>(),
        bounds: [
            std::array::from_fn(|i| centre[i] - half[i]),
            std::array::from_fn(|i| centre[i] + half[i]),
        ],
        shape: None,
    }
}

#[test]
fn single_part_does_not_move() {
    assert!(
        offsets(
            &[block("fixed", [0.; 3], [10.; 3])],
            &Default::default(),
            1.
        )
        .unwrap()
        .is_empty()
    );
}

#[test]
fn factor_zero_is_the_assembly() {
    let b = [
        block("base", [0.; 3], [10.; 3]),
        block("lid", [0., 0., 9.], [4.; 3]),
    ];
    assert!(offsets(&b, &Default::default(), 0.).unwrap().is_empty());
    assert!(offsets(&b, &Default::default(), f64::NAN).is_err());
}

#[test]
fn stacked_plates_move_along_z() {
    let b = [
        block("base", [0.; 3], [10.; 3]),
        block("p1", [0., 0., 10.], [9., 9., 1.]),
        block("p2", [0., 0., 11.], [9., 9., 1.]),
    ];
    let values = offsets(&b, &Default::default(), 1.).unwrap();
    assert_eq!(&values[0].vector[..2], &[0., 0.]);
    assert_eq!(&values[1].vector[..2], &[0., 0.]);
    assert!(10. + values[0].vector[2] - 1. > 10.);
    assert!(11. + values[1].vector[2] - 1. > 11. + values[0].vector[2]);
}
#[test]
fn screw_moves_along_its_axis() {
    let b = [
        block("fixed", [0.; 3], [10.; 3]),
        block("screw", [8., 0., 0.], [3.; 3]),
    ];
    let motion:MotionStudy=serde_json::from_value(serde_json::json!({"joints":{"bolt":{"kind":"slider","parts":["screw"],"origin":[8,0,0],"axis":[0,1,0]}}})).unwrap();
    let v = offsets(&b, &motion, 1.).unwrap()[0].vector;
    assert_eq!([v[0], v[2]], [0.; 2]);
    assert!(v[1].abs() > 13.);
}

#[test]
fn factor_one_separates_all_bounds_and_leaves_placements_on_disk_unchanged() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assembly-motion");
    let before = std::fs::read(path.join("placement.json")).unwrap();
    let (data, b) = faber_wiring::read(&path).unwrap();
    let shifts = offsets(&b, &data.motion, 1.).unwrap();
    let bounds: Vec<_> = b
        .iter()
        .map(|body| {
            let v = shifts
                .iter()
                .find(|o| o.part == body.id)
                .map_or([0.; 3], |o| o.vector);
            body.bounds
                .map(|p| std::array::from_fn::<_, 3, _>(|i| p[i] + v[i]))
        })
        .collect();
    for (i, a) in bounds.iter().enumerate() {
        for b in &bounds[..i] {
            assert!(
                (0..3).any(|j| a[1][j] < b[0][j] || a[0][j] > b[1][j]),
                "{a:?} touches {b:?}"
            );
        }
    }
    assert_eq!(before, std::fs::read(path.join("placement.json")).unwrap());
}
