//! Use the assembly pose when measuring a surface in world coordinates.
use crate::surface::{Analysis, DEFAULT_CHORD_MM, Request, analyse_in_frame};
use faber_ports::{Body, FaceMesh, Mesh, Result};
use glam::{DMat4, DVec3, Mat4};
use std::{borrow::Cow, collections::BTreeMap};

pub fn analyse_posed(
    bodies: &[Body],
    part: &str,
    request: &Request,
    poses: &BTreeMap<String, Mat4>,
) -> Result<Analysis> {
    for transform in poses.values() {
        let [a, b, c, d] = transform.to_cols_array_2d();
        let [x, y, z] = [a, b, c].map(|v| DVec3::new(v[0].into(), v[1].into(), v[2].into()));
        let tol = 1e-4;
        if !transform.is_finite()
            || [a[3], b[3], c[3], d[3] - 1.].iter().any(|v| v.abs() > 1e-5)
            || [
                x.length_squared() - 1.,
                y.length_squared() - 1.,
                z.length_squared() - 1.,
                x.dot(y),
                y.dot(z),
                z.dot(x),
                x.cross(y).dot(z) - 1.,
            ]
            .iter()
            .any(|v| v.abs() > tol)
        {
            return Err("Surface analysis needs a rigid assembly pose.".into());
        }
    }
    analyse_in_frame(bodies, part, request, DEFAULT_CHORD_MM, poses)
}

pub(crate) fn transform_mesh(mesh: &mut Mesh, pose: Mat4) {
    let pose = DMat4::from_cols_array(&pose.to_cols_array().map(f64::from));
    for p in &mut mesh.vertices {
        *p = pose.transform_point3(DVec3::from_array(*p)).to_array();
    }
    for n in &mut mesh.normals {
        *n = pose.transform_vector3(DVec3::from_array(*n)).to_array();
    }
}

pub(crate) fn world_faces<'a>(faces: &'a FaceMesh, pose: Option<&Mat4>) -> Cow<'a, FaceMesh> {
    if let Some(&transform) = pose
        && transform != Mat4::IDENTITY
    {
        let mut world = faces.clone();
        transform_mesh(&mut world.mesh, transform);
        Cow::Owned(world)
    } else {
        Cow::Borrowed(faces)
    }
}
