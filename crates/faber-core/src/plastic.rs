//! Solid attachments on a named plane. Dimensions are in millimetres.

use faber_model::{Geometry, Placement, SketchLoop, SketchProfile};
use faber_ports::{Combine, EdgeInfo, Kernel, Result, Shape};
use glam::DVec3;
use serde::Serialize;
use std::collections::BTreeSet;

const EPS: f64 = 1e-6;

fn positive(name: &str, value: f64) -> Result<()> {
    if value.is_finite() && value > 0. {
        Ok(())
    } else {
        Err(format!("The {name} must be finite and above zero."))
    }
}

fn frame(plane: &Placement, at: [f64; 2], angle: f64) -> Result<Placement> {
    if !at.into_iter().chain([angle]).all(f64::is_finite) {
        return Err("The attachment position and angle must be finite.".into());
    }
    let axes = crate::rotation_matrix(plane.rotation);
    Ok(Placement {
        translation: (DVec3::from_array(plane.translation) + axes * DVec3::new(at[0], at[1], 0.))
            .to_array(),
        rotation: crate::euler_from_matrix(axes * crate::rotation_matrix([0., 0., angle])),
    })
}

fn attach(kernel: &dyn Kernel, body: &Shape, addition: &Shape, frame: &Placement) -> Result<Shape> {
    let placed = kernel.transform_shape(addition, frame)?;
    let joined = kernel.combine(body, &placed, Combine::Union)?;
    let before = kernel.summary_properties(body)?;
    let after = kernel.summary_properties(&joined)?;
    if !after.valid
        || !after.volume.is_finite()
        || after.solids > before.solids
        || after.volume <= before.volume
    {
        return Err("The attachment must join the body and add material.".into());
    }
    Ok(joined)
}

/// Round the root edges that touch an upward face of the host body.
fn round_root(
    kernel: &dyn Kernel,
    joined: Shape,
    frame: &Placement,
    radius: Option<f64>,
    on_outline: impl Fn(&EdgeInfo, [f64; 3]) -> bool,
) -> Result<Shape> {
    let Some(radius) = radius else {
        return Ok(joined);
    };
    if radius == 0. {
        return Ok(joined);
    }
    positive("root fillet radius", radius)?;
    let axes = crate::rotation_matrix(frame.rotation);
    let origin = DVec3::from_array(frame.translation);
    let local =
        |point: [f64; 3]| (axes.transpose() * (DVec3::from_array(point) - origin)).to_array();
    let host_faces: BTreeSet<_> = kernel
        .faces(&joined)?
        .into_iter()
        .filter(|face| {
            face.normal
                .is_some_and(|n| DVec3::from_array(n).dot(axes.z_axis) > 1. - EPS)
                && local(face.centre)[2].abs() < EPS
        })
        .map(|face| face.index)
        .collect();
    let touching: BTreeSet<_> = kernel
        .edge_faces(&joined)?
        .into_iter()
        .filter(|pair| host_faces.contains(&pair.face))
        .map(|pair| pair.edge)
        .collect();
    let edges: Vec<_> = kernel
        .edges(&joined)?
        .into_iter()
        .filter(|edge| {
            let p = local(edge.centre);
            touching.contains(&edge.index) && p[2].abs() < EPS && on_outline(edge, p)
        })
        .map(|e| e.index)
        .collect();
    if edges.is_empty() {
        return Err("The root fillet needs a base edge on the body face.".into());
    }
    kernel.fillet(&joined, &edges, radius)
}

pub(crate) fn boss(
    kernel: &dyn Kernel,
    body: &Shape,
    plane: &Placement,
    geometry: &Geometry,
) -> Result<Shape> {
    let Geometry::Boss {
        at,
        diameter,
        height,
        hole_diameter,
        hole_depth,
        draft_deg,
        ribs,
        fillet,
        ..
    } = geometry
    else {
        return Err("A boss recipe is required.".into());
    };
    positive("boss diameter", *diameter)?;
    positive("boss height", *height)?;
    let frame = frame(plane, *at, 0.)?;
    let radius = diameter / 2.;
    let draft = draft_deg.unwrap_or(0.);
    if draft.abs() >= 90. || !draft.is_finite() {
        return Err("The boss draft must be between -90 and 90 degrees.".into());
    }
    let top_radius = radius - height * draft.to_radians().tan();
    positive("boss top radius", top_radius)?;
    if hole_depth.is_some() && hole_diameter.is_none() {
        return Err("A boss hole depth needs a hole diameter.".into());
    }
    let bore = if let Some(hole) = hole_diameter {
        positive("boss hole diameter", *hole)?;
        let depth = hole_depth.unwrap_or(*height);
        positive("boss hole depth", depth)?;
        let end_radius = top_radius + depth.min(*height) * draft.to_radians().tan();
        let limit = diameter.min(2. * top_radius.min(end_radius));
        if *hole >= limit {
            return Err(format!(
                "Boss hole {hole} mm must be less than the boss diameter {limit} mm."
            ));
        }
        let up = crate::rotation_matrix(frame.rotation).z_axis;
        let start = DVec3::from_array(frame.translation) + (height - depth) * up;
        Some((
            *hole,
            depth,
            Placement {
                translation: start.to_array(),
                rotation: frame.rotation,
            },
        ))
    } else {
        None
    };
    let mut addition = if draft == 0. {
        kernel.cylinder_shape(radius, *height)?
    } else {
        kernel.cone_shape(radius, top_radius, *height)?
    };
    let mut rib_angles = vec![];
    if let Some(ribs) = ribs {
        if !(1. ..=1000.).contains(&ribs.count) || ribs.count.fract() != 0. {
            return Err("Boss rib count must be an integer from 1 to 1000.".into());
        }
        for (name, v) in [
            ("rib length", ribs.length),
            ("rib thickness", ribs.thickness),
            ("rib height", ribs.height),
        ] {
            positive(name, v)?;
        }
        if ribs.height > *height || ribs.thickness >= *diameter {
            return Err(
                "Keep the ribs within the boss height and below its diameter in thickness.".into(),
            );
        }
        let rib = kernel.box_shape([radius + ribs.length, ribs.thickness, ribs.height])?;
        let rib = kernel.transform_shape(
            &rib,
            &Placement {
                translation: [0., -ribs.thickness / 2., 0.],
                ..Default::default()
            },
        )?;
        for i in 0..ribs.count as u32 {
            let angle = 360. * f64::from(i) / ribs.count;
            rib_angles.push(angle.to_radians().sin_cos());
            let rotated = kernel.transform_shape(
                &rib,
                &Placement {
                    rotation: [0., 0., angle],
                    ..Default::default()
                },
            )?;
            addition = kernel.combine(&addition, &rotated, Combine::Union)?;
        }
    }
    let mut joined = attach(kernel, body, &addition, &frame)?;
    if let Some((hole, depth, placement)) = bore {
        joined = crate::hole::build(kernel, &joined, hole, depth, &placement, None)?;
    }
    round_root(kernel, joined, &frame, *fillet, |edge, [x, y, _]| {
        if let Some(r) = edge.radius {
            return (r - radius).abs() < EPS && x.hypot(y) <= radius + EPS;
        }
        ribs.as_ref().is_some_and(|rib| {
            rib_angles.iter().any(|&(sin, cos)| {
                let radial = x * cos + y * sin;
                let side = (y * cos - x * sin).abs();
                radial >= 0.
                    && radial <= radius + rib.length + EPS
                    && side <= rib.thickness / 2. + EPS
                    && ((side - rib.thickness / 2.).abs() < EPS
                        || (radial - radius - rib.length).abs() < EPS)
            })
        })
    })
}

pub(crate) fn snap_hook(
    kernel: &dyn Kernel,
    body: &Shape,
    plane: &Placement,
    geometry: &Geometry,
) -> Result<Shape> {
    let Geometry::SnapHook {
        at,
        angle_deg,
        length,
        width,
        thickness,
        tip_thickness,
        undercut,
        hook_length,
        lead_deg,
        return_deg,
        root_fillet,
        ..
    } = geometry
    else {
        return Err("A snap hook recipe is required.".into());
    };
    let tip = tip_thickness.unwrap_or(*thickness);
    for (name, v) in [
        ("hook length", *length),
        ("hook width", *width),
        ("root thickness", *thickness),
        ("tip thickness", tip),
        ("undercut", *undercut),
        ("hook head length", *hook_length),
    ] {
        positive(name, v)?;
    }
    let taper_slope = (tip - thickness) / (2. * length);
    let run = |angle: f64, slope: f64| -> Result<f64> {
        if !(angle > 0. && angle <= 90.) {
            return Err(
                "The lead and return angles must be above zero and at most 90 degrees.".into(),
            );
        }
        if angle == 90. {
            return Ok(0.);
        }
        // Find the intersection with the tapered beam at the specified flank angle.
        let difference = angle.to_radians().tan() + slope;
        positive("angle slope relative to the tapered beam", difference)?;
        Ok(undercut / difference)
    };
    let lead = run(lead_deg.unwrap_or(30.), taper_slope)?;
    let back = run(return_deg.unwrap_or(90.), -taper_slope)?;
    if *hook_length > *length || lead + back > *hook_length + 1e-10 {
        return Err("The head must fit on the beam and contain both hook ramps.".into());
    }
    let front = |z: f64| (thickness + (tip - thickness) * (z / length)) * 0.5;
    let start = length - hook_length;
    let shoulder = start + back;
    let crest = length - lead;
    let mut points = vec![
        [-thickness / 2., 0.],
        [thickness / 2., 0.],
        [front(start), start],
        [front(shoulder) + undercut, shoulder],
        [front(crest) + undercut, crest],
        [tip / 2., *length],
        [-tip / 2., *length],
    ];
    points.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-10 && (a[1] - b[1]).abs() < 1e-10);
    let profile = SketchProfile {
        outer: SketchLoop::Polygon {
            points,
            corner_radius: None,
        },
        holes: vec![],
    };
    let side = kernel.sketch(
        &profile,
        &Placement {
            translation: [0., width / 2., 0.],
            rotation: [90., 0., 0.],
        },
    )?;
    let beam = kernel.extrude_shape(&side, [0., -width, 0.])?;
    let frame = frame(plane, *at, *angle_deg)?;
    let joined = attach(kernel, body, &beam, &frame)?;
    round_root(kernel, joined, &frame, *root_fillet, |edge, [x, y, _]| {
        edge.direction.is_some()
            && x.abs() <= thickness / 2. + EPS
            && y.abs() <= width / 2. + EPS
            && ((x.abs() - thickness / 2.).abs() < EPS || (y.abs() - width / 2.).abs() < EPS)
    })
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct HookCheck {
    pub deflection_mm: f64,
    pub strain: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strain_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    pub formula: &'static str,
    pub method: &'static str,
    pub resolution: &'static str,
    pub limits: Vec<String>,
}

/// Calculate root strain for a uniform cantilever under an imposed tip deflection.
pub fn hook_check(length: f64, thickness: f64, deflection: f64, limit: Option<f64>) -> HookCheck {
    let strain = 1.5 * (thickness / length) * (deflection / length);
    let mut limits=vec!["Assumes small elastic deflection and constant rectangular section. Taper, root fillet, creep and repeated loading change the result.".into()];
    if limit.is_none() {
        limits.push("No strain limit is known for this material.".into());
    }
    HookCheck {
        deflection_mm: deflection,
        strain,
        strain_limit: limit,
        ok: limit.map(|l| strain <= l),
        formula: "strain = 1.5 * thickness * deflection / length^2",
        method: "Elastic cantilever beam under a tip load.",
        resolution: "Closed-form estimate from the beam dimensions.",
        limits,
    }
}

pub fn checked_hook(
    length: f64,
    thickness: f64,
    deflection: f64,
    limit: Option<f64>,
) -> Result<HookCheck> {
    positive("beam length", length)?;
    positive("beam thickness", thickness)?;
    if deflection < 0. || !deflection.is_finite() {
        return Err("The undercut must be finite and at least zero.".into());
    }
    if let Some(limit) = limit {
        positive("snap strain limit", limit)?;
    }
    let check = hook_check(length, thickness, deflection, limit);
    if !check.strain.is_finite() {
        return Err("The beam dimensions exceed the strain calculation range.".into());
    }
    Ok(check)
}
