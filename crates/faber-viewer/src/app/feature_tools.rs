//! Default recipes for path tools, scale and plastic features.

use super::ToolKind;
use faber_model::{Geometry, PathInput, Scalar, ScaleFactor};

/// The first recipe shown in the feature form.
pub(super) fn new_geometry(
    kind: &ToolKind,
    source: String,
    part: &Geometry<Scalar>,
) -> Result<Geometry<Scalar>, String> {
    let path = || {
        crate::feature_form::path_ids(part)
            .into_iter()
            .next()
            .map(PathInput::Feature)
            .ok_or_else(|| "Add a curve or helix feature to this part first.".to_owned())
    };
    let plane = || {
        crate::feature_form::plane_ids(part, "")
            .into_iter()
            .next()
            .ok_or_else(|| "Add a reference plane to this part first.".to_owned())
    };
    let line = || vec![[[0., 0.].map(Scalar::Number), [10., 0.].map(Scalar::Number)]];
    let face = || faber_model::FaceSelector {
        normal: Some([0., 0., 1.].map(Scalar::Number)),
        cylinder_radius: None,
        internal: None,
        created_by: None,
        near_centre: None,
        in_box: None,
        area: None,
        tolerance: 0.000001.into(),
        expect: 1,
    };
    Ok(match kind {
        ToolKind::Scale => Geometry::Scale {
            source,
            factor: ScaleFactor::Uniform(Scalar::Number(2.)),
            about: std::array::from_fn(|_| Scalar::Number(0.)),
        },
        ToolKind::Pipe => Geometry::Pipe {
            path: path()?,
            diameter: Scalar::Number(10.),
            wall: None,
        },
        ToolKind::PathPattern => Geometry::PathPattern {
            source,
            path: path()?,
            count: Scalar::Number(3.),
            spacing: None,
            orient: false,
        },
        ToolKind::Rib => Geometry::Rib {
            body: source,
            plane: plane()?,
            lines: line(),
            thickness: 2.0.into(),
            side: faber_model::RibSide::Symmetric,
            draft_deg: None,
        },
        ToolKind::Web => Geometry::Web {
            body: source,
            plane: plane()?,
            lines: line(),
            thickness: 2.0.into(),
            draft_deg: None,
        },
        ToolKind::Boss => Geometry::Boss {
            body: source,
            plane: plane()?,
            at: [0., 0.].map(Scalar::Number),
            diameter: 8.0.into(),
            height: 10.0.into(),
            hole_diameter: None,
            hole_depth: None,
            draft_deg: None,
            ribs: None,
            fillet: None,
        },
        ToolKind::SnapHook => Geometry::SnapHook {
            body: source,
            plane: plane()?,
            at: [0., 0.].map(Scalar::Number),
            angle_deg: 0.0.into(),
            length: 15.0.into(),
            width: 5.0.into(),
            thickness: 2.0.into(),
            tip_thickness: None,
            undercut: 1.5.into(),
            hook_length: 3.0.into(),
            lead_deg: None,
            return_deg: None,
            root_fillet: None,
        },
        ToolKind::Lip => Geometry::Lip {
            body: source,
            face: face(),
            loop_: faber_model::LipLoop::Outer,
            side: faber_model::LipSide::Inside,
            width: 1.0.into(),
            height: 2.0.into(),
        },
        ToolKind::Groove => Geometry::Groove {
            body: source,
            face: face(),
            loop_: faber_model::LipLoop::Outer,
            side: faber_model::LipSide::Inside,
            width: 1.0.into(),
            depth: 2.0.into(),
            clearance: 0.15.into(),
        },
        _ => return Err("Select a modelling tool.".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::super::Shell;
    use super::*;
    use eframe::egui;
    use egui_kittest::kittest::Queryable;
    use faber_model::{Feature, Part, Placement};
    use std::f64::consts::PI;
    use std::path::Path;
    use std::time::{Duration, Instant};

    /// A 10 x 20 x 30 box. Optionally adds a helix of radius 20 and pitch 10 with one turn.
    fn project(dir: &Path, helix: bool) {
        let mut features = vec![serde_json::json!({
            "id": "base",
            "geometry": {"op": "box", "size": [10, 20, 30]}
        })];
        if helix {
            features.push(serde_json::json!({
                "id": "path_1",
                "geometry": {"op": "helix", "radius": 20, "pitch": 10, "turns": 1}
            }));
        }
        let features: Vec<Feature<Scalar>> = features
            .into_iter()
            .map(|value| serde_json::from_value(value).unwrap())
            .collect();
        let mut project = faber_wiring::open(dir, true, Some("Feature test".into())).unwrap();
        let part = Part {
            component: None,
            material: None,
            name: "Body".into(),
            geometry: Geometry::History {
                features,
                output: "base".into(),
            },
            placement: Placement::default(),
            colour: [130, 170, 180],
            group: String::new(),
            note: String::new(),
        };
        project.put_part("body".into(), part).unwrap();
    }

    fn open(dir: &Path) -> egui_kittest::Harness<'static, Shell> {
        let (data, bodies) = faber_wiring::read(dir).unwrap();
        let root = dir.to_path_buf();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1440., 920.))
            .wgpu_setup(crate::gpu_setup())
            .build_eframe(move |cc| Shell::new(cc, root, data, bodies, None));
        for _ in 0..5 {
            harness.step();
        }
        harness.state_mut().selected = Some("body".into());
        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness
    }

    fn wait_for_save(harness: &mut egui_kittest::Harness<'_, Shell>) {
        harness.step();
        let deadline = Instant::now() + Duration::from_secs(60);
        while harness.state().saving {
            harness.step();
            assert!(Instant::now() < deadline, "Feature did not save");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            harness.state().error.is_none(),
            "{:?}",
            harness.state().error
        );
    }

    fn set_field(harness: &mut egui_kittest::Harness<'_, Shell>, path: &str, text: &str) {
        let form = harness.state_mut().feature_draft.as_mut().unwrap();
        form.fields
            .iter_mut()
            .find(|(field, _, _)| field == path)
            .unwrap_or_else(|| panic!("no field {path}"))
            .1 = text.into();
        harness.step();
    }

    /// Applies the open form and returns the saved body volume.
    fn apply(harness: &mut egui_kittest::Harness<'_, Shell>, dir: &Path) -> f64 {
        harness.get_by_label("Apply feature").click();
        wait_for_save(harness);
        assert!(harness.state().feature_draft.is_none());
        let (_, bodies) = faber_wiring::read(dir).unwrap();
        bodies.iter().find(|b| b.id == "body").unwrap().volume
    }

    fn saved_feature(dir: &Path, id: &str) -> serde_json::Value {
        let (data, _) = faber_wiring::read(dir).unwrap();
        let Geometry::History { features, .. } = &data.parts["body"].geometry else {
            panic!("body is not a history");
        };
        let feature = features.iter().find(|f| f.id == id).unwrap();
        serde_json::to_value(&feature.geometry).unwrap()
    }

    #[test]
    fn new_pipe_without_a_path_is_refused() {
        let geometry: Geometry<Scalar> =
            serde_json::from_value(serde_json::json!({"op":"box","size":[1,1,1]})).unwrap();
        let error = new_geometry(&ToolKind::Pipe, "base".into(), &geometry).unwrap_err();
        assert_eq!(error, "Add a curve or helix feature to this part first.");
    }

    #[test]
    fn plastic_feature_tools_open_from_the_feature_list() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), false);
        let mut harness = open(dir.path());
        harness.get_by_label("Plastic features").click();
        for _ in 0..3 {
            harness.step();
        }
        for label in ["Rib", "Web", "Boss", "Snap hook", "Lip", "Groove"] {
            assert!(harness.query_by_label(label).is_some(), "Missing {label}");
        }
    }

    fn copy_snap_box(dir: &Path) {
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/snap-box");
        std::fs::create_dir(dir.join("parts")).unwrap();
        for file in [
            "faber.json",
            "placement.json",
            "parts/body.json",
            "parts/lid.json",
        ] {
            std::fs::copy(example.join(file), dir.join(file)).unwrap();
        }
    }

    #[test]
    fn plastic_feature_snap_box_fits_the_bed_and_closes() {
        let dir = tempfile::tempdir().unwrap();
        copy_snap_box(dir.path());
        let mut project = faber_wiring::open(dir.path(), false, None).unwrap();
        let report = faber_wiring::print::print_check(&project, None, [0., 0., 1.]).unwrap();
        assert!(
            report.fits
                && report
                    .findings
                    .iter()
                    .all(|f| f.severity != faber_print::Severity::Error)
        );
        assert!(
            report.bed_contact_mm2 > 4000.,
            "Both parts must touch the bed."
        );
        let mut lid = project.data.parts["lid"].clone();
        lid.placement.translation = [0., 40., 23.7].map(Scalar::Number);
        lid.placement.rotation = [180., 0., 0.].map(Scalar::Number);
        project
            .batch_strict(vec![faber_model::Edit::PutPart {
                id: "lid".into(),
                part: lid,
            }])
            .unwrap();
        let interference =
            faber_wiring::interference(&project.bodies, 0.01, &project.data.contacts).unwrap();
        assert!(
            interference.violations.is_empty(),
            "{:?}",
            interference.violations
        );
        let gap = project.measure_distance("body", "lid").unwrap().distance_mm;
        assert!(gap > 0.05 && gap < 0.3, "Lid clearance is {gap} mm.");
    }

    #[test]
    fn plastic_boss_form_adds_a_boss_to_the_snap_box() {
        let dir = tempfile::tempdir().unwrap();
        copy_snap_box(dir.path());
        let before = faber_wiring::read(dir.path()).unwrap().1;
        let volume = before.iter().find(|b| b.id == "body").unwrap().volume;
        let mut harness = open(dir.path());
        harness.get_by_label("Plastic features").scroll_to_me();
        for _ in 0..4 {
            harness.step();
        }
        harness.get_by_label("Plastic features").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Boss").click();
        for _ in 0..4 {
            harness.step();
        }
        harness.get_by_label("Reference plane").click();
        for _ in 0..3 {
            harness.step();
        }
        // The project tree also has a row for the floor plane.
        harness.query_all_by_label("floor").last().unwrap().click();
        harness.step();
        set_field(&mut harness, "/at/0", "30");
        set_field(&mut harness, "/at/1", "9");
        set_field(&mut harness, "/diameter", "wall * 2");
        set_field(&mut harness, "/height", "3");
        let after = apply(&mut harness, dir.path());
        assert!((after - volume - PI * 4. * 3.).abs() < 0.02);
        let saved = saved_feature(dir.path(), "boss_1");
        assert_eq!(saved["diameter"], "wall * 2");
        assert_eq!(saved["plane"], "floor");
    }

    #[test]
    fn path_ids_lists_only_curve_and_helix_features() {
        let part: Geometry<Scalar> = serde_json::from_value(serde_json::json!({
            "op":"history",
            "features":[
                {"id":"base","geometry":{"op":"box","size":[1,1,1]}},
                {"id":"path_1","geometry":{"op":"helix","radius":5,"pitch":2,"turns":1}}
            ],
            "output":"base"
        }))
        .unwrap();
        assert_eq!(crate::feature_form::path_ids(&part), ["path_1"]);
    }

    #[test]
    fn scale_form_creates_and_edits_the_factor() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), false);
        let mut harness = open(dir.path());
        harness.get_by_label("Scale").click();
        for _ in 0..3 {
            harness.step();
        }
        set_field(&mut harness, "/factor", "3");
        let volume = apply(&mut harness, dir.path());
        assert!((volume - 6000. * 27.).abs() < 1e-3, "{volume}");
        let saved = saved_feature(dir.path(), "scale_1");
        assert_eq!(saved["op"], "scale");
        assert_eq!(saved["factor"], 3.);
        assert_eq!(saved["about"], serde_json::json!([0., 0., 0.]));

        harness.state_mut().last_poll = Instant::now();
        harness.step();
        harness.get_by_label("scale_1").click();
        for _ in 0..3 {
            harness.step();
        }
        harness.get_by_label("Factor per axis").click();
        harness.step();
        set_field(&mut harness, "/factor/0", "2");
        set_field(&mut harness, "/factor/1", "3");
        set_field(&mut harness, "/factor/2", "4");
        let volume = apply(&mut harness, dir.path());
        assert!((volume - 6000. * 24.).abs() < 1e-3, "{volume}");
        assert_eq!(saved_feature(dir.path(), "scale_1")["factor"][2], 4.);
    }

    #[test]
    fn pipe_form_uses_the_helix_path_and_saves_a_tube() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), true);
        let mut harness = open(dir.path());
        harness.get_by_label("Pipe").click();
        for _ in 0..3 {
            harness.step();
        }
        let form = harness.state().feature_draft.as_ref().unwrap();
        assert!(
            !form.fields.iter().any(|(path, _, _)| path == "/path"),
            "the path is a drop-down, not a text field"
        );
        harness.get_by_label("Hollow tube").click();
        harness.step();
        set_field(&mut harness, "/wall", "1");
        let volume = apply(&mut harness, dir.path());
        let saved = saved_feature(dir.path(), "pipe_1");
        assert_eq!(saved["op"], "pipe");
        assert_eq!(saved["path"], "path_1");
        assert_eq!(saved["wall"], 1.);
        // Annulus of outer radius 5 and inner radius 4, along one turn of the helix.
        let length = ((2. * PI * 20.).powi(2) + 10f64.powi(2)).sqrt();
        let expected = PI * (25. - 16.) * length;
        assert!(
            (volume - expected).abs() < expected * 0.02,
            "{volume} against {expected}"
        );
    }

    #[test]
    fn path_pattern_form_saves_count_and_spacing() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path(), true);
        let mut harness = open(dir.path());
        harness.get_by_label("Path pattern").click();
        for _ in 0..3 {
            harness.step();
        }
        set_field(&mut harness, "/count", "3");
        harness.get_by_label("Set spacing").click();
        harness.step();
        set_field(&mut harness, "/spacing", "50");
        let volume = apply(&mut harness, dir.path());
        let saved = saved_feature(dir.path(), "path_pattern_1");
        assert_eq!(saved["op"], "path_pattern");
        assert_eq!(saved["path"], "path_1");
        assert_eq!(saved["count"], 3.);
        assert_eq!(saved["spacing"], 50.);
        assert!((volume - 3. * 6000.).abs() < 3. * 6000. * 0.02, "{volume}");
    }
}
