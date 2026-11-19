use faber_model::ProjectData;
use faber_model::machining::{
    Heights, Holes, Machining, Operation, OperationKind, Origin, Selection, Setup, Side, Stock,
    Tool, ToolKind,
};
use faber_ports::program::Dialect;
use faber_wiring::machining::{calculate, write_program};
use serde_json::json;

fn tool(kind: ToolKind, diameter: f64, number: u32) -> Tool {
    Tool {
        kind,
        diameter,
        number,
        corner_radius: 0.,
        flute_length: 20.,
        flutes: 2,
        feed_mm_min: 360.,
        plunge_mm_min: 90.,
        spindle_rpm: 8000.,
    }
}
fn operation(id: &str, tool: &str, kind: OperationKind, bottom: f64) -> Operation {
    Operation {
        id: id.into(),
        setup: "vertical".into(),
        tool: tool.into(),
        heights: Heights {
            clearance: 7.,
            retract: 3.,
            top: 0.,
            bottom: Some(bottom),
        },
        stepdown: Some(3.),
        kind,
    }
}
fn plate() -> (tempfile::TempDir, ProjectData) {
    let dir = tempfile::tempdir().unwrap();
    let mut p = faber_wiring::open(dir.path(), true, Some("Mill fixture".into())).unwrap();
    let mut features = vec![
        json!({"id":"stock","geometry":{"op":"box","size":[48,36,10]}}),
        json!({"id":"cut_block","geometry":{"op":"box","size":[20,14,5]}}),
        json!({"id":"pocket_tool","geometry":{"op":"transform","source":"cut_block","placement":{"translation":[14,11,6]}}}),
        json!({"id":"pocket_floor","geometry":{"op":"difference","left":"stock","right":"pocket_tool"}}),
    ];
    let mut source = "pocket_floor".to_owned();
    for (i, [x, y]) in [[5, 5], [43, 5], [5, 31], [43, 31]].into_iter().enumerate() {
        let id = format!("hole_{i}");
        features.push(json!({"id":id,"geometry":{"op":"hole","source":source,"diameter":5,"depth":10,"placement":{"translation":[x,y,0]}}}));
        source = id;
    }
    let plate = serde_json::from_value(json!({
        "name":"Mill plate",
        "colour":[66,100,140],
        "geometry":{"op":"history","features":features,"output":source}
    }))
    .unwrap();
    p.put_part::<faber_model::Scalar>("plate".into(), plate)
        .unwrap();
    let mut data = p.data.clone();
    data.machining = Machining {
        tools: [
            ("flat".into(), tool(ToolKind::Flat, 4., 1)),
            ("drill".into(), tool(ToolKind::Drill, 5., 2)),
        ]
        .into(),
        setups: [(
            "vertical".into(),
            Setup {
                part: "plate".into(),
                up: [0., 0., 1.],
                origin: Origin::StockTopCorner,
                stock: Stock::Offset {
                    sides: 2.,
                    top: 1.,
                    bottom: 1.,
                },
            },
        )]
        .into(),
        operations: vec![
            operation("face", "flat", OperationKind::Face { stepover: 2. }, -1.),
            operation(
                "pocket",
                "flat",
                OperationKind::Pocket {
                    floor: Selection::Z(-5.),
                    stepover: 2.,
                    stock_to_leave: 0.,
                },
                -5.,
            ),
            operation(
                "outside",
                "flat",
                OperationKind::Contour {
                    z: Selection::Z(-3.),
                    side: Side::Outside,
                    stock_to_leave: 0.,
                },
                -11.,
            ),
            operation(
                "bore",
                "drill",
                OperationKind::Drill {
                    holes: Holes::Auto,
                    peck: Some(2.),
                },
                -11.,
            ),
        ],
    };
    (dir, data)
}

#[test]
fn plate_with_pocket_and_holes() {
    let (d, data) = plate();
    let run = calculate(d.path(), &data, "vertical", None).unwrap();
    assert_eq!(
        run.reports
            .iter()
            .map(|r| r.operation.as_str())
            .collect::<Vec<_>>(),
        ["face", "pocket", "outside", "bore"]
    );
    for r in &run.reports {
        assert!(
            r.gouge_mm <= 0.02,
            "{} gouged {} mm",
            r.operation,
            r.gouge_mm
        );
        assert_eq!(r.rapid_collisions, 0, "{}", r.operation);
        assert!(r.cut_length_mm > 0. && r.minutes > 0.);
    }
    let path = d.path().join("plate.nc");
    write_program(&run, &data, &path, Dialect::Grbl, false).unwrap();
    let program = std::fs::read_to_string(path).unwrap();
    let z_min = program
        .lines()
        .filter(|l| l.starts_with("G0 ") || l.starts_with("G1 "))
        .flat_map(str::split_whitespace)
        .filter_map(|s| s.strip_prefix('Z')?.parse::<f64>().ok())
        .fold(f64::INFINITY, f64::min);
    assert!((z_min + 11.).abs() < 1e-6, "{z_min}");
    assert!((z_min - run.stock.min[2] - 1.).abs() < 1e-6);
    assert!(program.lines().next_back().unwrap().contains("M2"));
}

#[test]
fn gouge_blocks_the_program_and_forced_output_has_a_warning() {
    let (d, mut data) = plate();
    data.machining.operations = vec![operation(
        "bad",
        "drill",
        OperationKind::Drill {
            holes: Holes::List(vec![[10., 12.]]),
            peck: None,
        },
        -6.,
    )];
    let run = calculate(d.path(), &data, "vertical", None).unwrap();
    assert!(run.verification.gouge_mm > 2.);
    let path = d.path().join("forced.nc");
    assert!(write_program(&run, &data, &path, Dialect::Grbl, false).is_err());
    assert!(!path.exists());
    write_program(&run, &data, &path, Dialect::Grbl, true).unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    let first = text.lines().next().unwrap().to_lowercase();
    assert!(first.contains("gouge"), "{first}");
}
