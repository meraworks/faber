use faber_kernel_occt::OcctKernel;
use faber_model::{CurvePath, CurveSegment, SplitKeep};
use faber_ports::{Kernel, Shape};
use std::f64::consts::PI;

fn face(shape: &Shape, check: impl Fn(&faber_ports::FaceInfo) -> bool) -> Shape {
    let index = OcctKernel
        .faces(shape)
        .unwrap()
        .into_iter()
        .find(check)
        .unwrap()
        .index;
    OcctKernel.face_shape(shape, index).unwrap()
}

fn panel(start: [f64; 3], segment: CurveSegment, span: [f64; 3]) -> Shape {
    let wire = OcctKernel
        .curve(&CurvePath {
            start,
            segments: vec![segment],
        })
        .unwrap();
    OcctKernel.extrude_shape(&wire, span).unwrap()
}

fn plane(z: f64, slope: f64, width: f64) -> Shape {
    panel(
        [-1., -1., z - slope],
        CurveSegment::Line {
            end: [width, -1., z + width * slope],
        },
        [0., 42., 0.],
    )
}

fn curved() -> Shape {
    panel(
        [-1., -1., 9.],
        CurveSegment::Arc {
            via: [10., -1., 13.],
            end: [21., -1., 9.],
        },
        [0., 22., 0.],
    )
}

fn square() -> Shape {
    let b = OcctKernel.box_shape([40., 40., 1.]).unwrap();
    face(&b, |f| f.normal.is_some_and(|n| n[2] > 0.9))
}

fn near(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1e-5 * expected.abs().max(1.),
        "{actual} differs from {expected}"
    );
}

fn area(shape: &Shape) -> f64 {
    OcctKernel
        .faces(shape)
        .unwrap()
        .iter()
        .map(|f| f.area)
        .sum()
}

#[test]
fn sketch_trim_keeps_the_inside_or_the_perforated_outside() {
    let surface = square();
    let cylinder = OcctKernel.cylinder_shape(10., 10.).unwrap();
    let disk = face(&cylinder, |f| f.normal.is_some_and(|n| n[2] > 0.9));
    let placement = faber_model::Placement {
        translation: [20., 20., -100.],
        ..Default::default()
    };
    let cutter = OcctKernel.transform_shape(&disk, &placement).unwrap();
    for (point, expected) in [
        ([20., 20., 1.], 100. * PI),
        ([2., 2., 1.], 1600. - 100. * PI),
    ] {
        let cut = OcctKernel
            .trim_with_sketch(&surface, &cutter, [0., 0., 1.], point)
            .unwrap();
        near(area(&cut), expected);
    }
}

#[test]
fn trim_by_another_surface_and_missed_tool() {
    let surface = square();
    let cutter = panel(
        [20., -1., -1.],
        CurveSegment::Line {
            end: [20., 41., -1.],
        },
        [0., 0., 4.],
    );
    near(
        area(
            &OcctKernel
                .trim_surface(&surface, &cutter, [1., 1., 1.])
                .unwrap(),
        ),
        800.,
    );
    assert!(
        OcctKernel
            .trim_with_plane(&surface, [41., 0., 0.], [1., 0., 0.], [1., 1., 1.])
            .is_err()
    );
}

#[test]
fn planar_extend_and_cylinder_extend_keep_the_surface() {
    let panel = square();
    near(
        area(&OcctKernel.extend_surface(&panel, 1, 5.).unwrap()),
        1800.,
    );
    let cyl = OcctKernel.cylinder_shape(10., 20.).unwrap();
    let side = face(&cyl, |f| f.cylinder.is_some());
    let ring = OcctKernel
        .edges(&side)
        .unwrap()
        .into_iter()
        .find(|e| e.radius.is_some() && e.centre[2] > 19.)
        .unwrap();
    let out = OcctKernel.extend_surface(&side, ring.index, 5.).unwrap();
    near(area(&out), 2. * PI * 10. * 25.);
    assert!(OcctKernel.faces(&out).unwrap().iter().all(|f| {
        f.cylinder
            .as_ref()
            .is_some_and(|c| (c.radius - 10.).abs() < 1e-6)
    }));
}

#[test]
fn split_by_tilted_and_curved_surfaces_and_refuse_a_miss() {
    let b = OcctKernel.box_shape([20.; 3]).unwrap();
    for tool in [plane(6., 0.3, 21.), curved()] {
        let pieces = OcctKernel.split_by_surface(&b, &tool).unwrap();
        assert_eq!(pieces.len(), 2);
        near(
            pieces
                .iter()
                .map(|s| OcctKernel.properties(s).unwrap().volume)
                .sum(),
            8000.,
        );
        let pos = OcctKernel
            .split_body_by_surface(&b, &tool, SplitKeep::Positive)
            .unwrap();
        let neg = OcctKernel
            .split_body_by_surface(&b, &tool, SplitKeep::Negative)
            .unwrap();
        let (p, n) = (
            OcctKernel.properties(&pos).unwrap(),
            OcctKernel.properties(&neg).unwrap(),
        );
        near(p.volume + n.volume, 8000.);
        // The swept edge and span set the face normal. Check both directions against it.
        let direction = OcctKernel.faces(&tool).unwrap()[0].normal;
        if let Some(normal) = direction {
            assert!((p.centre[2] - n.centre[2]) * normal[2] > 0.);
        }
        assert!(OcctKernel.intersect_solids(&pos, &neg).unwrap().is_none());
    }
    assert!(
        OcctKernel
            .split_by_surface(&b, &plane(42., 0., 21.))
            .is_err()
    );
}

#[test]
fn replace_top_with_lower_curve_or_higher_plane_and_check_coverage() {
    let b = OcctKernel.box_shape([20.; 3]).unwrap();
    let top = OcctKernel
        .faces(&b)
        .unwrap()
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.9))
        .unwrap()
        .index;
    let lower = OcctKernel.replace_face(&b, top, &curved()).unwrap();
    let faces = OcctKernel.faces(&lower).unwrap();
    assert_eq!(faces.iter().filter(|f| f.normal.is_some()).count(), 5);
    assert_eq!(faces.iter().filter(|f| f.normal.is_none()).count(), 1);
    assert!(OcctKernel.properties(&lower).unwrap().volume < 8000.);
    near(
        OcctKernel
            .properties(
                &OcctKernel
                    .replace_face(&b, top, &plane(24., 0., 21.))
                    .unwrap(),
            )
            .unwrap()
            .volume,
        9600.,
    );
    assert_eq!(
        OcctKernel
            .replace_face(&b, top, &plane(25., 0., 9.))
            .err()
            .unwrap(),
        format!("The surface does not cover face {top}. Extend the surface.")
    );
}

#[test]
fn replacing_a_cap_continues_its_sloping_neighbour() {
    let body = OcctKernel.cone_shape(15., 10., 20.).unwrap();
    let top = OcctKernel
        .faces(&body)
        .unwrap()
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.9))
        .unwrap()
        .index;
    let target = panel(
        [-20., -20., 24.],
        CurveSegment::Line {
            end: [20., -20., 24.],
        },
        [0., 40., 0.],
    );
    let out = OcctKernel.replace_face(&body, top, &target).unwrap();
    let expected = PI * 24. / 3. * (15. * 15. + 15. * 9. + 9. * 9.);
    near(OcctKernel.properties(&out).unwrap().volume, expected);
    assert_eq!(OcctKernel.faces(&out).unwrap().len(), 3);
}

#[test]
fn a_curved_body_face_can_be_replaced_again() {
    let b = OcctKernel.box_shape([20.; 3]).unwrap();
    let top = OcctKernel
        .faces(&b)
        .unwrap()
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.9))
        .unwrap()
        .index;
    let rounded = OcctKernel.replace_face(&b, top, &curved()).unwrap();
    let roof = OcctKernel
        .faces(&rounded)
        .unwrap()
        .into_iter()
        .find(|f| f.normal.is_none())
        .unwrap()
        .index;
    let flat = OcctKernel
        .replace_face(&rounded, roof, &plane(8., 0., 21.))
        .unwrap();
    near(OcctKernel.properties(&flat).unwrap().volume, 3200.);
    let raised = OcctKernel
        .replace_face(&rounded, roof, &plane(17., 0., 21.))
        .unwrap();
    near(OcctKernel.properties(&raised).unwrap().volume, 6800.);
}

#[test]
fn extending_a_disc_rim_preserves_its_analytic_circle() {
    let cylinder = OcctKernel.cylinder_shape(10., 10.).unwrap();
    let disc = face(&cylinder, |f| f.normal.is_some_and(|n| n[2] > 0.9));
    let extended = OcctKernel.extend_surface(&disc, 1, 5.).unwrap();
    near(area(&extended), 225. * PI);
    let edges = OcctKernel.edges(&extended).unwrap();
    assert_eq!(edges.len(), 1);
    near(edges[0].radius.unwrap(), 15.);
}

#[test]
fn splitting_a_compound_keeps_an_uncut_solid_on_its_side() {
    let lower = OcctKernel.box_shape([20.; 3]).unwrap();
    let isolated = OcctKernel
        .transform_shape(
            &lower,
            &faber_model::Placement {
                translation: [0., 0., 30.],
                ..Default::default()
            },
        )
        .unwrap();
    let compound = OcctKernel
        .combine(&lower, &isolated, faber_ports::Combine::Union)
        .unwrap();
    let tool = plane(10., 0., 21.);
    let positive = OcctKernel
        .split_body_by_surface(&compound, &tool, SplitKeep::Positive)
        .unwrap();
    let negative = OcctKernel
        .split_body_by_surface(&compound, &tool, SplitKeep::Negative)
        .unwrap();
    let mut volumes = [
        OcctKernel.properties(&positive).unwrap().volume,
        OcctKernel.properties(&negative).unwrap().volume,
    ];
    volumes.sort_by(f64::total_cmp);
    near(volumes[0], 4000.);
    near(volumes[1], 12000.);
}

#[test]
fn solid_tools_are_refused_for_surface_operations() {
    let solid = OcctKernel.box_shape([20.; 3]).unwrap();
    let small = OcctKernel
        .transform_shape(
            &OcctKernel.box_shape([5.; 3]).unwrap(),
            &faber_model::Placement {
                translation: [5., 5., 5.],
                ..Default::default()
            },
        )
        .unwrap();
    assert!(OcctKernel.split_by_surface(&solid, &small).is_err());
    assert!(
        OcctKernel
            .trim_with_plane(&solid, [10., 0., 0.], [1., 0., 0.], [5., 5., 5.])
            .is_err()
    );
}

#[test]
fn history_cuts_rebuild_in_a_rotated_sketch_plane_and_refuse_missing_inputs_atomically() {
    use faber_core::Reading;
    use faber_model::{Part, Scalar};
    use serde_json::{Value, json};
    let dir = tempfile::tempdir().unwrap();
    let mut project = faber_wiring::open(dir.path(), true, Some("Surface history".into())).unwrap();
    let feature = |id: &str, geometry: Value| json!({"id":id,"geometry":geometry});
    let recipe = |sketch: &str| -> Part<Scalar> {
        let features = [
            feature("box", json!({"op":"box","size":[20,20,20]})),
            feature(
                "start",
                json!({"op":"curve","path":{"start":[0,7,0],"segments":[{"kind":"line","end":[40,7,0]}]}}),
            ),
            feature(
                "panel",
                json!({"op":"surface_extrude","profile":"start","distance":40,"direction":[0,0,1]}),
            ),
            feature(
                "circle",
                json!({"op":"sketch","placement":{"rotation":[90,0,0],"translation":[0,-90,0]},"profile":{"outer":{"kind":"circle","centre":[20,20],"radius":10}}}),
            ),
            feature(
                "trimmed",
                json!({"op":"trim","surface":"panel","tool":sketch,"keep":{"near":[2,7,2]}}),
            ),
            feature(
                "extended",
                json!({"op":"extend","surface":"trimmed","edge":{"near_centre":[0,7,20],"tolerance":0.01,"expect":1},"distance":5}),
            ),
            feature(
                "section",
                json!({"op":"curve","path":{"start":[-1,-1,8],"segments":[{"kind":"line","end":[21,-1,8]}]}}),
            ),
            feature(
                "tool",
                json!({"op":"surface_extrude","profile":"section","distance":22,"direction":[0,1,0]}),
            ),
            feature(
                "split",
                json!({"op":"split_body","body":"box","tool":"tool","keep":"both"}),
            ),
            feature(
                "cap",
                json!({"op":"replace_face","body":"box","face":{"normal":[0,0,1],"tolerance":0.01,"expect":1},"surface":"tool"}),
            ),
        ];
        serde_json::from_value(json!({"name":"Cut fixture","colour":[40,90,120],"geometry":{"op":"history","features":features,"output":"cap"}})).unwrap()
    };
    project.put_part("sample".into(), recipe("circle")).unwrap();
    for (feature, expected) in [
        ("trimmed", 1600. - 100. * PI),
        ("extended", 1800. - 100. * PI),
    ] {
        let Reading::Surface { area_mm2, .. } =
            project.feature_info("sample", feature).unwrap().reading
        else {
            panic!("Expected a surface for {feature}")
        };
        near(area_mm2, expected);
    }
    for (feature, expected) in [("split", 8000.), ("cap", 3200.)] {
        let Reading::Solid { volume_mm3, .. } =
            project.feature_info("sample", feature).unwrap().reading
        else {
            panic!("Expected a solid for {feature}")
        };
        near(volume_mm3, expected);
    }
    let revision = project.data.revision;
    let manifest = dir.path().join("faber.json");
    let before = std::fs::read(&manifest).unwrap();
    assert!(
        project
            .put_part("sample".into(), recipe("missing_tool"))
            .is_err()
    );
    assert_eq!(project.data.revision, revision);
    assert_eq!(std::fs::read(&manifest).unwrap(), before);
    let Reading::Solid { volume_mm3, .. } = project.feature_info("sample", "cap").unwrap().reading
    else {
        panic!("Expected the saved cap")
    };
    near(volume_mm3, 3200.);
}

#[test]
fn replacement_beyond_a_cone_tip_refuses_to_add_a_cylindrical_wall() {
    let body = OcctKernel.cone_shape(15., 10., 20.).unwrap();
    let top = OcctKernel
        .faces(&body)
        .unwrap()
        .into_iter()
        .find(|f| f.normal.is_some_and(|n| n[2] > 0.9))
        .unwrap()
        .index;
    let beyond = panel(
        [-20., -20., 70.],
        CurveSegment::Line {
            end: [20., -20., 70.],
        },
        [0., 40., 0.],
    );
    assert!(OcctKernel.replace_face(&body, top, &beyond).is_err());
}

#[test]
fn trim_splits_on_an_existing_stitched_seam_and_refuses_an_outer_edge() {
    let panels = [0., 10.].map(|x| {
        panel(
            [x, 0., 0.],
            CurveSegment::Line {
                end: [x + 10., 0., 0.],
            },
            [0., 10., 0.],
        )
    });
    let sheet = OcctKernel.stitch(&panels, 0.001, false).unwrap().shape;
    for (point, x_min) in [([2., 2., 0.], 0.), ([18., 2., 0.], 10.)] {
        let piece = OcctKernel
            .trim_with_plane(&sheet, [10., 0., 0.], [1., 0., 0.], point)
            .unwrap();
        near(area(&piece), 100.);
        near(OcctKernel.properties(&piece).unwrap().bounds[0][0], x_min);
    }
    assert!(
        OcctKernel
            .trim_with_plane(&sheet, [0., 0., 0.], [1., 0., 0.], [2., 2., 0.])
            .is_err()
    );
    assert!(
        OcctKernel
            .trim_with_plane(&sheet, [0., 0., 0.], [0., 0., 1.], [2., 2., 0.])
            .is_err()
    );
    let short_tool = panel(
        [10., 0., -2.],
        CurveSegment::Line {
            end: [10., 5., -2.],
        },
        [0., 0., 4.],
    );
    assert!(
        OcctKernel
            .trim_surface(&sheet, &short_tool, [2., 2., 0.])
            .is_err()
    );
}
