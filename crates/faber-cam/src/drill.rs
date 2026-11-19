use crate::Toolpath;
use crate::path::{Writer, passes};
use faber_model::machining::{Holes, Operation, OperationKind, Tool};
use faber_ports::{FaceInfo, Result};
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    pub centre: [f64; 2],
    pub diameter: f64,
    pub top: f64,
    pub bottom: f64,
}
pub fn find_holes(faces: &[FaceInfo], up: [f64; 3]) -> Vec<Hole> {
    let Some(z) = DVec3::from(up).try_normalize() else {
        return vec![];
    };
    let axis = if z.x.abs() < 0.99 { DVec3::X } else { DVec3::Y };
    let x = (axis - axis.dot(z) * z).normalize();
    let y = z.cross(x);
    let mut holes = vec![];
    let mut split = BTreeMap::new();
    for face in faces {
        let Some(c) = &face.cylinder else { continue };
        let Some(direction) = DVec3::from(c.axis).try_normalize() else {
            continue;
        };
        if !c.internal || direction.dot(z).abs() < 1. - 1e-8 || c.radius <= 0. || c.length <= 0. {
            continue;
        }
        let area = TAU * c.radius * c.length;
        if !area.is_finite() || !face.area.is_finite() || face.area <= 0. {
            continue;
        }
        let p = DVec3::from(c.start);
        let a = p.dot(z);
        let b = a + direction.dot(z) * c.length;
        let hole = Hole {
            centre: [p.dot(x), p.dot(y)],
            diameter: 2. * c.radius,
            top: a.max(b),
            bottom: a.min(b),
        };
        let dimensions = [
            hole.centre[0],
            hole.centre[1],
            hole.diameter,
            hole.top,
            hole.bottom,
        ];
        if !dimensions.iter().all(|v| v.is_finite()) {
            continue;
        }
        if (face.area - area).abs() <= area * 1e-5 + 1e-7 {
            holes.push(hole);
        } else if face.area < area {
            // Some models split a complete bore along two or more seams.
            let key = dimensions.map(|v| (v * 1e7).round() as i64);
            let (.., combined_area, moment) =
                split.entry(key).or_insert((hole, 0., glam::DVec2::ZERO));
            let centroid = DVec3::from(face.centre) - p;
            *combined_area += face.area;
            *moment += glam::DVec2::new(centroid.dot(x), centroid.dot(y)) * face.area;
        }
    }
    for (hole, combined_area, moment) in split.into_values() {
        let area = TAU * 0.5 * hole.diameter * (hole.top - hole.bottom);
        if (combined_area - area).abs() <= area * 1e-5 + 1e-7
            && moment.length() <= area * (1e-6 + 1e-5 * hole.diameter)
        {
            holes.push(hole);
        }
    }
    holes.sort_by(|a, b| {
        a.centre[0]
            .total_cmp(&b.centre[0])
            .then(a.centre[1].total_cmp(&b.centre[1]))
            .then(a.bottom.total_cmp(&b.bottom))
    });
    holes.dedup_by(|a, b| a == b);
    holes
}
pub fn drill(holes: &[Hole], tool: &Tool, op: &Operation) -> Result<Toolpath> {
    let OperationKind::Drill {
        holes: select,
        peck,
    } = &op.kind
    else {
        return Err("Select a drill operation.".into());
    };
    if peck.is_some_and(|v| !v.is_finite() || v <= 0.) {
        return Err("The peck depth must be positive and finite.".into());
    }
    let mut w = Writer::new(tool, op)?;
    for hole in holes {
        if matches!(select, Holes::Auto)
            && (hole.diameter - tool.diameter).abs() > (0.02f64).max(tool.diameter * 0.001)
        {
            continue;
        }
        if let Holes::List(points) = select
            && !points
                .iter()
                .any(|&p| glam::DVec2::from(p).distance(hole.centre.into()) < 1e-6)
        {
            continue;
        }
        let top = op.heights.top.min(hole.top);
        let bottom = op.heights.bottom.unwrap_or(hole.bottom);
        if bottom >= top {
            return Err("The drill bottom must be below the hole top.".into());
        }
        for depth in passes(top, bottom, *peck)? {
            w.start(hole.centre)?;
            w.feed(hole.centre, depth, tool.plunge_mm_min)?;
        }
    }
    w.finish().map_err(|e| {
        if e == "The operation has no cutting moves." {
            "No holes match the drill.".into()
        } else {
            e
        }
    })
}
