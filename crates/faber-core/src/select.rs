use faber_model::{EdgeSelector, FaceSelector};
use faber_ports::{EdgeInfo, FaceInfo, Kernel, Result, Shape};
use glam::DVec3;
use std::collections::{BTreeMap, BTreeSet};

const SHOWN_MATCHES: usize = 5;
const SHOWN_CANDIDATES: usize = 3;

/// One candidate with its key data and the filters that rejected it.
struct Item {
    index: u32,
    line: String,
    failed: Vec<&'static str>,
}

pub(crate) fn plane(
    kernel: &dyn Kernel,
    shape: &Shape,
    selector: &faber_model::FaceSelector,
    offset: f64,
) -> Result<faber_model::Placement> {
    if !offset.is_finite() || offset.abs() > 1e6 {
        return Err("Plane offset must be finite and within 1000000 mm".into());
    }
    let selected = faces(kernel, shape, selector)?;
    if selected.len() != 1 {
        return Err("A reference plane needs exactly one planar face".into());
    }
    let face = kernel
        .faces(shape)?
        .into_iter()
        .find(|face| face.index == selected[0])
        .ok_or("Selected face is missing")?;
    let normal = DVec3::from(face.normal.ok_or("A reference plane needs a planar face")?);
    if !normal.is_finite() || normal.length() < 1e-10 {
        return Err("Selected face has an invalid normal".into());
    }
    let z = normal.normalize();
    let seed = if z.dot(DVec3::X).abs() > 0.999999 {
        DVec3::Y
    } else {
        DVec3::X
    };
    let x = (seed - z * seed.dot(z)).normalize();
    let y = z.cross(x);
    let rotation = glam::DQuat::from_mat3(&glam::DMat3::from_cols(x, y, z));
    let (rz, ry, rx) = rotation.to_euler(glam::EulerRot::ZYX);
    let origin = DVec3::from(face.centre) + z * offset;
    if !origin.is_finite() || origin.abs().max_element() > 1e6 {
        return Err("Reference plane origin is outside the supported range".into());
    }
    Ok(faber_model::Placement {
        translation: origin.to_array(),
        rotation: [rx, ry, rz].map(f64::to_degrees),
    })
}

/// Return the face indices that match, or a count error with the closest candidates.
pub(crate) fn faces(
    kernel: &dyn Kernel,
    shape: &Shape,
    selector: &FaceSelector,
) -> Result<Vec<u32>> {
    check_face_selector(shape, selector)?;
    let items = face_items(shape, &kernel.faces(shape)?, selector)?;
    let matched = matched_indices(&items);
    if matched.len() != selector.expect {
        return Err(report("Face", selector.expect, &items));
    }
    Ok(matched)
}

/// Return the face infos that pass every filter. The expected count is not checked.
pub(crate) fn filter_faces(
    shape: &Shape,
    faces: Vec<FaceInfo>,
    selector: &FaceSelector,
) -> Result<Vec<FaceInfo>> {
    check_face_selector(shape, selector)?;
    let keep: BTreeSet<u32> = matched_indices(&face_items(shape, &faces, selector)?)
        .into_iter()
        .collect();
    Ok(faces
        .into_iter()
        .filter(|face| keep.contains(&face.index))
        .collect())
}

/// Return the edge indices that match, or a count error with the closest candidates.
pub(crate) fn edges(
    kernel: &dyn Kernel,
    shape: &Shape,
    selector: &EdgeSelector,
) -> Result<Vec<u32>> {
    check_edge_selector(selector)?;
    let items = edge_items(kernel, shape, &kernel.edges(shape)?, selector)?;
    let matched = matched_indices(&items);
    if matched.len() != selector.expect {
        return Err(report("Edge", selector.expect, &items));
    }
    Ok(matched)
}

/// Return the edge infos that pass every filter. The expected count is not checked.
pub(crate) fn filter_edges(
    kernel: &dyn Kernel,
    shape: &Shape,
    edges: Vec<EdgeInfo>,
    selector: &EdgeSelector,
) -> Result<Vec<EdgeInfo>> {
    check_edge_selector(selector)?;
    let keep: BTreeSet<u32> = matched_indices(&edge_items(kernel, shape, &edges, selector)?)
        .into_iter()
        .collect();
    Ok(edges
        .into_iter()
        .filter(|edge| keep.contains(&edge.index))
        .collect())
}

fn matched_indices(items: &[Item]) -> Vec<u32> {
    items
        .iter()
        .filter(|item| item.failed.is_empty())
        .map(|item| item.index)
        .collect()
}

fn report(kind: &str, expected: usize, items: &[Item]) -> String {
    let matched: Vec<&Item> = items.iter().filter(|item| item.failed.is_empty()).collect();
    let mut text = format!(
        "{kind} selector found {}, expected {expected}.",
        matched.len()
    );
    if matched.is_empty() {
        text.push_str("\nNo item matched.");
    } else {
        text.push_str(&format!("\nMatched items (showing up to {SHOWN_MATCHES}):"));
        for item in matched.iter().take(SHOWN_MATCHES) {
            text.push_str("\n  ");
            text.push_str(&item.line);
        }
        if matched.len() > SHOWN_MATCHES {
            text.push_str(&format!("\n  and {} more", matched.len() - SHOWN_MATCHES));
        }
    }
    if matched.len() < expected {
        let mut rejected: Vec<&Item> = items
            .iter()
            .filter(|item| !item.failed.is_empty())
            .collect();
        // A stable sort keeps the index order of items with equal failure counts.
        rejected.sort_by_key(|item| item.failed.len());
        text.push_str(&format!(
            "\nClosest rejected items (showing up to {SHOWN_CANDIDATES}):"
        ));
        for item in rejected.iter().take(SHOWN_CANDIDATES) {
            text.push_str(&format!(
                "\n  {} rejected by {}",
                item.line,
                item.failed.join(", ")
            ));
        }
    }
    text
}

fn number(value: f64) -> String {
    let value = if value.abs() < 0.0005 { 0. } else { value };
    format!("{value:.3}")
}

fn vector(value: [f64; 3]) -> String {
    format!(
        "[{}, {}, {}]",
        number(value[0]),
        number(value[1]),
        number(value[2])
    )
}

fn inside(point: [f64; 3], bounds: &[[f64; 3]; 2], tolerance: f64) -> bool {
    (0..3).all(|axis| {
        point[axis] >= bounds[0][axis] - tolerance && point[axis] <= bounds[1][axis] + tolerance
    })
}

fn check_box(bounds: &Option<[[f64; 3]; 2]>) -> Result<()> {
    if let Some([min, max]) = bounds
        && (0..3).any(|axis| min[axis] > max[axis])
    {
        return Err("in_box minimum must not exceed maximum".into());
    }
    Ok(())
}

/// Refuse a malformed face selector before any kernel query.
fn check_face_selector(shape: &Shape, selector: &FaceSelector) -> Result<()> {
    if selector.expect == 0 || !selector.tolerance.is_finite() || selector.tolerance <= 0. {
        return Err("Face selector needs a positive tolerance and expected count".into());
    }
    for value in selector
        .normal
        .iter()
        .flatten()
        .chain(selector.near_centre.iter().flatten())
        .chain(selector.in_box.iter().flatten().flatten())
        .chain(&selector.area)
    {
        if !value.is_finite() {
            return Err("Face selector contains a non-finite value".into());
        }
    }
    if selector
        .normal
        .map(DVec3::from)
        .is_some_and(|n| n.length() < 1e-10)
    {
        return Err("Face normal must be non-zero".into());
    }
    check_box(&selector.in_box)?;
    if selector.created_by.is_some() && shape.face_origins().is_none() {
        return Err("Face history is not available for this source".into());
    }
    Ok(())
}

fn face_items(shape: &Shape, faces: &[FaceInfo], selector: &FaceSelector) -> Result<Vec<Item>> {
    let normal = selector.normal.map(DVec3::from);
    let tolerance = selector.tolerance;
    Ok(faces
        .iter()
        .map(|face| {
            let mut failed = Vec::new();
            if let Some(creator) = &selector.created_by
                && !shape
                    .face_origins()
                    .and_then(|origins| origins.get(&face.index))
                    .is_some_and(|origins| origins.contains(creator))
            {
                failed.push("created_by");
            }
            if let Some(n) = normal
                && !face
                    .normal
                    .is_some_and(|v| DVec3::from(v).dot(n.normalize()) >= 1. - 1e-8)
            {
                failed.push("normal");
            }
            if let Some(p) = selector.near_centre
                && DVec3::from(p).distance(DVec3::from(face.centre)) > tolerance
            {
                failed.push("near_centre");
            }
            if let Some(bounds) = &selector.in_box
                && !inside(face.centre, bounds, tolerance)
            {
                failed.push("in_box");
            }
            if let Some(area) = selector.area
                && (face.area - area).abs() > tolerance
            {
                failed.push("area");
            }
            let normal_text = face
                .normal
                .map_or("normal -".to_string(), |n| format!("normal {}", vector(n)));
            Item {
                index: face.index,
                line: format!(
                    "face {} centre {} {} area {}",
                    face.index,
                    vector(face.centre),
                    normal_text,
                    number(face.area)
                ),
                failed,
            }
        })
        .collect())
}

/// Refuse a malformed edge selector before any kernel query.
fn check_edge_selector(selector: &EdgeSelector) -> Result<()> {
    if selector.expect == 0 || !selector.tolerance.is_finite() || selector.tolerance <= 0. {
        return Err("Edge selector needs a positive tolerance and expected count".into());
    }
    for value in selector
        .parallel
        .iter()
        .flatten()
        .chain(selector.near_centre.iter().flatten())
        .chain(selector.in_box.iter().flatten().flatten())
        .chain(&selector.length)
        .chain(&selector.radius)
    {
        if !value.is_finite() {
            return Err("Edge selector contains a non-finite value".into());
        }
    }
    if selector
        .parallel
        .map(DVec3::from)
        .is_some_and(|d| d.length() < 1e-10)
    {
        return Err("Edge direction must be non-zero".into());
    }
    check_box(&selector.in_box)
}

fn edge_items(
    kernel: &dyn Kernel,
    shape: &Shape,
    edges: &[EdgeInfo],
    selector: &EdgeSelector,
) -> Result<Vec<Item>> {
    let direction = selector.parallel.map(DVec3::from).map(DVec3::normalize);
    let between = if let Some([a, b]) = &selector.between {
        let left = faces(kernel, shape, a)?;
        let right = faces(kernel, shape, b)?;
        let mut adjacent = BTreeMap::<u32, Vec<u32>>::new();
        for link in kernel.edge_faces(shape)? {
            adjacent.entry(link.edge).or_default().push(link.face);
        }
        Some(
            adjacent
                .into_iter()
                .filter(|(_, faces)| {
                    left.iter().any(|a| {
                        faces.contains(a) && right.iter().any(|b| a != b && faces.contains(b))
                    })
                })
                .map(|(edge, _)| edge)
                .collect::<BTreeSet<_>>(),
        )
    } else {
        None
    };
    let of_face = if let Some(face) = &selector.of_face {
        let inner: BTreeSet<u32> = faces(kernel, shape, face)?.into_iter().collect();
        Some(
            kernel
                .edge_faces(shape)?
                .into_iter()
                .filter(|link| inner.contains(&link.face))
                .map(|link| link.edge)
                .collect::<BTreeSet<_>>(),
        )
    } else {
        None
    };
    let tolerance = selector.tolerance;
    Ok(edges
        .iter()
        .map(|edge| {
            let mut failed = Vec::new();
            if between
                .as_ref()
                .is_some_and(|set| !set.contains(&edge.index))
            {
                failed.push("between");
            }
            if of_face
                .as_ref()
                .is_some_and(|set| !set.contains(&edge.index))
            {
                failed.push("of_face");
            }
            if let Some(d) = direction
                && !edge
                    .direction
                    .is_some_and(|v| DVec3::from(v).dot(d).abs() >= 1. - 1e-8)
            {
                failed.push("parallel");
            }
            if let Some(p) = selector.near_centre
                && DVec3::from(p).distance(DVec3::from(edge.centre)) > tolerance
            {
                failed.push("near_centre");
            }
            if let Some(bounds) = &selector.in_box
                && !inside(edge.centre, bounds, tolerance)
            {
                failed.push("in_box");
            }
            if let Some(length) = selector.length
                && (edge.length - length).abs() > tolerance
            {
                failed.push("length");
            }
            if let Some(radius) = selector.radius
                && !edge.radius.is_some_and(|r| (r - radius).abs() <= tolerance)
            {
                failed.push("radius");
            }
            let mut line = format!("edge {} centre {}", edge.index, vector(edge.centre));
            if let Some(d) = edge.direction {
                line.push_str(&format!(" direction {}", vector(d)));
            }
            line.push_str(&format!(" length {}", number(edge.length)));
            if let Some(r) = edge.radius {
                line.push_str(&format!(" radius {}", number(r)));
            }
            Item {
                index: edge.index,
                line,
                failed,
            }
        })
        .collect())
}
