//! Frames for construction references. Each function turns reference inputs into a plane or an axis.

use crate::placement::{euler_from_matrix, rotation_matrix};
use crate::select;
use faber_model::{EdgeSelector, FaceSelector, Placement};
use faber_ports::{Kernel, Result, Shape};
use glam::{DMat3, DVec3};
use std::f64::consts::PI;

/// Two unit vectors that differ by less than this are parallel.
const PARALLEL: f64 = 1e-6;

/// The origin and unit normal of a plane placement.
pub(crate) fn plane_of(placement: &Placement) -> (DVec3, DVec3) {
    let frame = rotation_matrix(placement.rotation);
    (DVec3::from(placement.translation), frame.z_axis.normalize())
}

/// A placement at `origin` with the given normal. Local X lies along `hint`.
/// The hint must not be parallel to the normal.
fn frame(origin: DVec3, normal: DVec3, hint: DVec3) -> Placement {
    let z = normal.normalize();
    let x = (hint - z * hint.dot(z)).normalize();
    let y = z.cross(x);
    Placement {
        translation: origin.to_array(),
        rotation: euler_from_matrix(DMat3::from_cols(x, y, z)),
    }
}

/// A plane through an axis. It turns `angle` degrees about the axis from the reference plane.
pub(crate) fn angled(
    axis: ([f64; 3], [f64; 3]),
    reference: (DVec3, DVec3),
    angle: f64,
) -> Result<Placement> {
    if !angle.is_finite() || angle.abs() > 1e6 {
        return Err("Angle must be finite and within 1000000 degrees".into());
    }
    let origin = DVec3::from(axis.0);
    let direction = DVec3::from(axis.1).normalize();
    let (point, normal) = reference;
    if normal.dot(direction).abs() > PARALLEL || (origin - point).dot(normal).abs() > 1e-6 {
        return Err("The axis must lie in the reference plane.".into());
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    let turned = normal * cos + direction.cross(normal) * sin;
    Ok(frame(origin, turned, direction))
}

/// The plane halfway between two parallel planes. It keeps the frame of the first plane.
pub(crate) fn midplane(first: &Placement, second: &Placement) -> Result<Placement> {
    let (a, n1) = plane_of(first);
    let (b, n2) = plane_of(second);
    if n1.cross(n2).length() > PARALLEL {
        return Err("The two planes must be parallel.".into());
    }
    Ok(Placement {
        translation: ((a + b) / 2.).to_array(),
        rotation: first.rotation,
    })
}

/// The plane through three points. Its X axis lies along the first edge.
pub(crate) fn through_points(points: [DVec3; 3]) -> Result<Placement> {
    let [a, b, c] = points;
    let (u, v) = (b - a, c - a);
    let scale = u.length() * v.length();
    let normal = u.cross(v);
    if scale < 1e-12 || normal.length() <= 1e-9 * scale {
        return Err("The three points must be distinct and not on one line.".into());
    }
    Ok(frame(a, normal, u))
}

/// The line where two planes cross. The point on the line is nearest to the world origin.
pub(crate) fn plane_axis(
    first: (DVec3, DVec3),
    second: (DVec3, DVec3),
) -> Result<([f64; 3], [f64; 3])> {
    let (a, n1) = first;
    let (b, n2) = second;
    let cross = n1.cross(n2);
    if cross.length() < PARALLEL {
        return Err("The two planes must not be parallel.".into());
    }
    let c = n1.dot(n2);
    let (d1, d2) = (n1.dot(a), n2.dot(b));
    let det = 1. - c * c;
    let point = ((d1 - c * d2) * n1 + (d2 - c * d1) * n2) / det;
    Ok((point.to_array(), cross.normalize().to_array()))
}

/// The axis of the one cylindrical face that the selector matches.
pub(crate) fn cylinder_axis(
    kernel: &dyn Kernel,
    shape: &Shape,
    selector: &FaceSelector,
) -> Result<([f64; 3], [f64; 3])> {
    let matched = select::faces(kernel, shape, selector)?;
    if matched.len() != 1 {
        return Err("Select exactly one cylinder face.".into());
    }
    let face = kernel
        .faces(shape)?
        .into_iter()
        .find(|face| face.index == matched[0])
        .ok_or("Selected face is missing")?;
    let cylinder = face
        .cylinder
        .ok_or("The selected face must be a cylinder.")?;
    Ok((cylinder.start, cylinder.axis))
}

/// The centre of the one full circular edge that the selector matches.
pub(crate) fn circle_centre(
    kernel: &dyn Kernel,
    shape: &Shape,
    selector: &EdgeSelector,
) -> Result<[f64; 3]> {
    let matched = select::edges(kernel, shape, selector)?;
    if matched.len() != 1 {
        return Err("Select exactly one circular edge.".into());
    }
    let edge = kernel
        .edges(shape)?
        .into_iter()
        .find(|edge| edge.index == matched[0])
        .ok_or("Selected edge is missing")?;
    let radius = edge.radius.ok_or("The selected edge must be a circle.")?;
    let circumference = 2. * PI * radius;
    if (edge.length - circumference).abs() > 1e-6 * circumference {
        return Err("The selected edge must be a full circle.".into());
    }
    Ok(edge.centre)
}
