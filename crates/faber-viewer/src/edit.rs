use faber_model::ProjectData;
use faber_ports::Body;
use std::{collections::BTreeMap, path::Path};

pub fn load_project(root: &Path, name: Option<String>) -> Result<(ProjectData, Vec<Body>), String> {
    if let Some(name) = name {
        let project = faber_wiring::create(root, name)?;
        Ok((project.data, project.bodies))
    } else {
        faber_wiring::read(root)
    }
}

pub fn append_feature(
    root: &Path,
    expected: &ProjectData,
    part_id: &str,
    id: &str,
    geometry: faber_model::Geometry<faber_model::Scalar>,
    strict: bool,
) -> Result<(ProjectData, Vec<Body>), String> {
    use faber_model::{Edit, Feature, Geometry};
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Reload the feature editor.".into());
    }
    let mut part = expected
        .parts
        .get(part_id)
        .ok_or("Part is missing")?
        .clone();
    let (mut features, output) = match part.geometry {
        Geometry::History { features, output } => (features, output),
        geometry => (
            vec![Feature {
                id: "source".into(),
                geometry,
                suppressed: false,
            }],
            "source".into(),
        ),
    };
    let output = if matches!(
        geometry,
        Geometry::Plane { .. }
            | Geometry::PlaneOnFace { .. }
            | Geometry::Axis { .. }
            | Geometry::Point { .. }
            | Geometry::PlaneAtAngle { .. }
            | Geometry::Midplane { .. }
            | Geometry::PlaneThroughPoints { .. }
            | Geometry::AxisFromCylinder { .. }
            | Geometry::AxisFromPlanes { .. }
            | Geometry::PointAtCentre { .. }
    ) {
        output
    } else {
        id.into()
    };
    features.push(Feature {
        id: id.into(),
        geometry,
        suppressed: false,
    });
    part.geometry = Geometry::History { features, output };
    let operations = vec![Edit::PutPart {
        id: part_id.into(),
        part,
    }];
    if strict {
        project.batch_strict(operations)?;
    } else {
        project.batch(operations)?;
    }
    Ok((project.data, project.bodies))
}

pub fn strict_edit(
    root: &Path,
    expected: &ProjectData,
    operations: Vec<faber_model::Edit>,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Reload the editor.".into());
    }
    project.batch_strict(operations)?;
    Ok((project.data, project.bodies))
}

pub fn sections(
    root: &Path,
    expected: &ProjectData,
    operations: Vec<faber_model::Edit>,
    strict: bool,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Reload the analysis list.".into());
    }
    if strict {
        project.batch_strict(operations)?;
    } else {
        project.batch(operations)?;
    }
    Ok((project.data, project.bodies))
}

pub fn import(
    root: &Path,
    expected: &ProjectData,
    source: &Path,
    name: String,
    strict: bool,
    purchased: Option<faber_model::Purchased>,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Open the import form again.".into());
    }
    let id = (1..)
        .map(|n| format!("body_{n}"))
        .find(|id| {
            !project
                .data
                .parts
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(id))
        })
        .ok_or("No body id is available")?;
    project.import_with_record(id, name, source, strict, purchased)?;
    Ok((project.data, project.bodies))
}

pub fn place(
    root: &Path,
    expected: &ProjectData,
    id: &str,
    placement: faber_model::Placement<faber_model::Scalar>,
    strict: bool,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Open the placement form again.".into());
    }
    let mut part = expected.parts.get(id).ok_or("Part is missing")?.clone();
    part.placement = placement;
    let operations = vec![faber_model::Edit::PutPart {
        id: id.into(),
        part,
    }];
    if strict {
        project.batch_strict(operations)?;
    } else {
        project.batch(operations)?;
    }
    Ok((project.data, project.bodies))
}

pub fn create(
    root: &Path,
    expected: &ProjectData,
    part: faber_model::Part<faber_model::Scalar>,
    strict: bool,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Open a new sketch from the current design.".into());
    }
    let id = (1..)
        .map(|n| format!("body_{n}"))
        .find(|id| {
            !project
                .data
                .parts
                .keys()
                .any(|existing| existing.eq_ignore_ascii_case(id))
        })
        .ok_or("No body id is available")?;
    if strict {
        project.batch_strict(vec![faber_model::Edit::PutPart { id, part }])?;
    } else {
        project.put_part(id, part)?;
    }
    Ok((project.data, project.bodies))
}

pub fn feature(
    root: &Path,
    expected: &ProjectData,
    part: &str,
    id: &str,
    geometry: faber_model::Geometry<faber_model::Scalar>,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Reload the feature editor.".into());
    }
    project.edit_feature(part, id, geometry)?;
    Ok((project.data, project.bodies))
}

pub fn history(
    root: &Path,
    expected: &ProjectData,
    redo: bool,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Wait for the view to reload.".into());
    }
    let action = if redo {
        faber_ports::HistoryMove::Redo
    } else {
        faber_ports::HistoryMove::Undo
    };
    faber_wiring::history_move(root, &mut project, action)?;
    Ok((project.data, project.bodies))
}

pub fn parameters(
    root: &Path,
    expected: &ProjectData,
    values: BTreeMap<String, String>,
) -> Result<(ProjectData, Vec<Body>), String> {
    let mut project = faber_wiring::open(root, false, None)?;
    if project.data != *expected {
        return Err("Project changed. Reload the parameter editor.".into());
    }
    project.set_params(values)?;
    Ok((project.data, project.bodies))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_project_refuses_existing_data_and_open_preserves_it() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("new");
        let (data, bodies) = load_project(&root, Some("New design".into())).unwrap();
        assert_eq!(data.name, "New design");
        assert!(bodies.is_empty());
        let saved = faber_wiring::project_fingerprint(&root).unwrap();
        assert!(load_project(&root, Some("Replacement".into())).is_err());
        assert_eq!(faber_wiring::project_fingerprint(&root).unwrap(), saved);
        assert_eq!(load_project(&root, None).unwrap().0, data);
        let missing = directory.path().join("missing");
        assert!(load_project(&missing, None).is_err());
        assert!(!missing.exists());
    }

    #[test]
    fn placement_uses_expressions_and_preserves_data_on_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(dir.path(), true, Some("Placement GUI".into())).unwrap();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,30]}})).unwrap();
        project.put_part("left".into(), part.clone()).unwrap();
        let mut right = part;
        right.placement.translation[0] = faber_model::Scalar::Number(40.);
        project.put_part("right".into(), right).unwrap();
        project
            .set_params([("gap".into(), "100".into())].into())
            .unwrap();
        let expected = project.data.clone();
        drop(project);
        let overlap =
            serde_json::from_value(serde_json::json!({"translation":[0,0,0],"rotation":[0,0,0]}))
                .unwrap();
        assert!(place(dir.path(), &expected, "right", overlap, true).is_err());
        assert_eq!(faber_wiring::read(dir.path()).unwrap().0, expected);
        let invalid = serde_json::from_value(
            serde_json::json!({"translation":["unknown",0,0],"rotation":[0,0,0]}),
        )
        .unwrap();
        assert!(place(dir.path(), &expected, "right", invalid, false).is_err());
        assert_eq!(faber_wiring::read(dir.path()).unwrap().0, expected);
        let valid = serde_json::from_value(
            serde_json::json!({"translation":["gap",0,0],"rotation":[0,0,90]}),
        )
        .unwrap();
        let (data, bodies) = place(dir.path(), &expected, "right", valid, true).unwrap();
        let body = bodies.iter().find(|b| b.id == "right").unwrap();
        assert!((body.volume - 6000.).abs() < 1e-5);
        assert!((body.bounds[0][0] - 80.).abs() < 1e-5);
        assert!(
            place(dir.path(), &expected, "right", Default::default(), false)
                .unwrap_err()
                .contains("Project changed")
        );
        let (undone, _) = history(dir.path(), &data, false).unwrap();
        assert_eq!(undone.parts, expected.parts);
    }

    #[test]
    fn import_worker_refuses_stale_views_and_strict_interference() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Import GUI".into())).unwrap();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,10,10]}})).unwrap();
        project.put_part("box".into(), part).unwrap();
        let output = dir.path().join("out");
        faber_wiring::export_checked(
            &project.bodies,
            &project.data.contacts,
            &output,
            "step",
            false,
        )
        .unwrap();
        let stale = project.data.clone();
        project
            .set_params([("value".into(), "1".into())].into())
            .unwrap();
        let expected = project.data.clone();
        drop(project);
        let source = output.join("box.step");
        assert!(
            import(dir.path(), &stale, &source, "Copy".into(), false, None)
                .unwrap_err()
                .contains("Project changed")
        );
        assert!(import(dir.path(), &expected, &source, "Copy".into(), true, None).is_err());
        assert_eq!(faber_wiring::read(dir.path()).unwrap().0, expected);
        let (data, bodies) =
            import(dir.path(), &expected, &source, "Copy".into(), false, None).unwrap();
        assert_eq!(bodies.len(), 2);
        assert_eq!(data.parts["body_1"].name, "Copy");
    }

    #[test]
    fn pattern_wraps_a_primitive_and_keeps_undo_and_stale_view_checks() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Pattern GUI".into())).unwrap();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"placement":{"translation":[100,0,0]},"geometry":{"op":"box","size":[10,10,10]}})).unwrap();
        project.put_part("box".into(), part).unwrap();
        let expected = project.data.clone();
        drop(project);
        let geometry = serde_json::from_value(
            serde_json::json!({"op":"linear_pattern","source":"source","count":2,"step":[20,0,0]}),
        )
        .unwrap();
        let (data, bodies) =
            append_feature(dir.path(), &expected, "box", "pattern_1", geometry, false).unwrap();
        assert!((bodies[0].volume - 2000.).abs() < 1e-5);
        assert!((bodies[0].bounds[0][0] - 100.).abs() < 1e-5);
        let invalid = serde_json::from_value(serde_json::json!({"op":"linear_pattern","source":"pattern_1","count":0,"step":[20,0,0]})).unwrap();
        assert!(append_feature(dir.path(), &data, "box", "pattern_2", invalid, false).is_err());
        assert_eq!(faber_wiring::read(dir.path()).unwrap().0, data);
        let geometry = serde_json::from_value(
            serde_json::json!({"op":"linear_pattern","source":"source","count":2,"step":[20,0,0]}),
        )
        .unwrap();
        assert!(
            append_feature(dir.path(), &expected, "box", "pattern_2", geometry, false).is_err()
        );
        let (undone, bodies) = history(dir.path(), &data, false).unwrap();
        assert_eq!(undone.parts, expected.parts);
        assert!((bodies[0].volume - 1000.).abs() < 1e-5);
    }

    #[test]
    fn strict_gui_creation_keeps_the_saved_model_on_interference() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Strict GUI".into())).unwrap();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,10,10]}})).unwrap();
        project.put_part("box".into(), part.clone()).unwrap();
        let expected = project.data.clone();
        drop(project);
        assert!(create(dir.path(), &expected, part, true).is_err());
        assert_eq!(faber_wiring::read(dir.path()).unwrap().0, expected);
    }

    #[test]
    fn feature_edit_rebuilds_dependants_and_refuses_invalid_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let mut project =
            faber_wiring::open(dir.path(), true, Some("Feature edit".into())).unwrap();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"history","output":"placed","features":[{"id":"base","geometry":{"op":"box","size":[10,20,30]}},{"id":"placed","geometry":{"op":"transform","source":"base","placement":{"translation":[0,0,10]}}}]}})).unwrap();
        project.put_part("box".into(), part).unwrap();
        let expected = project.data.clone();
        drop(project);
        let geometry =
            serde_json::from_value(serde_json::json!({"op":"box","size":[20,20,30]})).unwrap();
        let (data, bodies) = feature(dir.path(), &expected, "box", "base", geometry).unwrap();
        assert!((bodies[0].volume - 12000.).abs() < 1e-6);
        assert!((bodies[0].bounds[0][2] - 10.).abs() < 1e-5);
        let invalid =
            serde_json::from_value(serde_json::json!({"op":"box","size":[-1,20,30]})).unwrap();
        assert!(feature(dir.path(), &data, "box", "base", invalid).is_err());
        assert_eq!(faber_wiring::read(dir.path()).unwrap().0, data);
    }

    #[test]
    fn history_actions_reopen_the_project_and_refuse_stale_views() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("History".into())).unwrap();
        project
            .set_params([("height".into(), "30".into())].into())
            .unwrap();
        let expected = project.data.clone();
        drop(project);
        let (undone, _) = history(dir.path(), &expected, false).unwrap();
        assert!(undone.params.is_empty());
        assert!(history(dir.path(), &expected, true).is_err());
        let (redone, _) = history(dir.path(), &undone, true).unwrap();
        assert_eq!(redone.params["height"], "30");
        assert!(redone.revision > expected.revision);
    }

    #[test]
    fn parameter_edits_rebuild_and_refuse_stale_or_invalid_changes() {
        let dir = tempfile::tempdir().unwrap();
        let mut project = faber_wiring::open(dir.path(), true, Some("Edit".into())).unwrap();
        project
            .set_params([("height".into(), "30".into())].into())
            .unwrap();
        let part: faber_model::Part<faber_model::Scalar> = serde_json::from_value(serde_json::json!({"name":"Box","colour":[100,140,170],"geometry":{"op":"box","size":[10,20,"height"]}})).unwrap();
        project.put_part("box".into(), part).unwrap();
        let expected = project.data.clone();
        drop(project);
        let (data, bodies) = parameters(
            dir.path(),
            &expected,
            [("height".into(), "40".into())].into(),
        )
        .unwrap();
        assert!((bodies[0].volume - 8000.).abs() < 1e-6);
        assert!(
            parameters(
                dir.path(),
                &expected,
                [("height".into(), "50".into())].into()
            )
            .is_err()
        );
        assert!(parameters(dir.path(), &data, [("height".into(), "-1".into())].into()).is_err());
        assert_eq!(faber_wiring::read(dir.path()).unwrap().0, data);
    }
}
