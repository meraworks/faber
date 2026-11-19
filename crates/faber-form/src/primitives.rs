use crate::{CageData, MAX_FACES};
use glam::DVec3;
use std::collections::BTreeMap;
use std::f64::consts::TAU;

fn positive(n: f64) -> bool {
    n.is_finite() && n > 0.
}
fn capacity(faces: u128) -> Result<(), String> {
    if faces > MAX_FACES as u128 {
        Err(format!(
            "The cage has {faces} faces. The limit is {MAX_FACES}."
        ))
    } else {
        Ok(())
    }
}
pub fn form_box(size: [f64; 3], segments: [u32; 3]) -> CageData {
    try_form_box(size, segments).unwrap_or_default()
}
pub fn try_form_box(size: [f64; 3], segments: [u32; 3]) -> Result<CageData, String> {
    if size.into_iter().any(|n| !positive(n)) || segments.contains(&0) {
        return Err("Box sides must be positive. Each side needs at least one segment.".into());
    }
    let [x, y, z] = segments.map(u128::from);
    capacity(2 * (x * y + y * z + z * x))?;
    let mut cage = CageData::default();
    let mut indices = BTreeMap::new();
    // Number the base first, then the top and the four sides.
    for axis in [2, 0, 1] {
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        for top in [false, true] {
            for i in 0..segments[u] {
                for j in 0..segments[v] {
                    let mut face = Vec::with_capacity(4);
                    for (a, b) in [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)] {
                        let mut key = [0; 3];
                        key[axis] = if top { segments[axis] } else { 0 };
                        key[u] = a;
                        key[v] = b;
                        face.push(*indices.entry(key).or_insert_with(|| {
                            let index = cage.vertices.len() as u32;
                            cage.vertices.push(std::array::from_fn(|k| {
                                let t = f64::from(key[k]) / f64::from(segments[k]);
                                (t - if k == 2 { 0. } else { 0.5 }) * size[k]
                            }));
                            index
                        }));
                    }
                    if !top {
                        face.reverse()
                    }
                    cage.faces.push(face);
                }
            }
        }
    }
    Ok(cage)
}
pub fn form_sphere(radius: f64, segments: u32) -> CageData {
    try_form_sphere(radius, segments).unwrap_or_default()
}
pub fn try_form_sphere(radius: f64, segments: u32) -> Result<CageData, String> {
    if !positive(radius) {
        return Err("The sphere radius must be positive and finite.".into());
    }
    let mut cage = try_form_box([2.; 3], [segments; 3])?;
    for p in &mut cage.vertices {
        p[2] -= 1.;
        *p = (DVec3::from(*p).normalize() * radius).to_array();
    }
    Ok(cage)
}
pub fn form_cylinder(radius: f64, height: f64, sides: u32, segments: u32) -> CageData {
    try_form_cylinder(radius, height, sides, segments).unwrap_or_default()
}
pub fn try_form_cylinder(
    radius: f64,
    height: f64,
    sides: u32,
    segments: u32,
) -> Result<CageData, String> {
    if !positive(radius) || !positive(height) || sides < 3 || segments == 0 {
        return Err(
            "The cylinder needs positive dimensions, at least three sides and one segment.".into(),
        );
    }
    capacity(u128::from(sides) * (u128::from(segments) + 2))?;
    let mut cage = CageData::default();
    for k in 0..=segments {
        for i in 0..sides {
            let theta = TAU * f64::from(i) / f64::from(sides);
            cage.vertices.push([
                radius * theta.cos(),
                radius * theta.sin(),
                (f64::from(k) / f64::from(segments)) * height,
            ]);
            if k < segments {
                let a = k * sides + i;
                let b = k * sides + (i + 1) % sides;
                cage.faces.push(vec![a, b, b + sides, a + sides]);
            }
        }
    }
    let bottom = cage.vertices.len() as u32;
    cage.vertices.extend([[0., 0., 0.], [0., 0., height]]);
    for i in 0..sides {
        let j = (i + 1) % sides;
        let top = segments * sides;
        cage.faces.push(vec![bottom, j, i]);
        cage.faces.push(vec![bottom + 1, top + i, top + j]);
    }
    Ok(cage)
}
pub fn form_torus(major: f64, minor: f64, sides: u32, rings: u32) -> CageData {
    try_form_torus(major, minor, sides, rings).unwrap_or_default()
}
pub fn try_form_torus(major: f64, minor: f64, sides: u32, rings: u32) -> Result<CageData, String> {
    if !positive(minor)
        || !positive(major)
        || major <= minor
        || !(major + minor).is_finite()
        || sides < 3
        || rings < 3
    {
        return Err("The torus needs two positive radii, a larger major radius and at least three divisions in each direction.".into());
    }
    capacity(u128::from(sides) * u128::from(rings))?;
    let mut cage = CageData::default();
    for i in 0..sides {
        for j in 0..rings {
            let a = TAU * f64::from(i) / f64::from(sides);
            let b = TAU * f64::from(j) / f64::from(rings);
            let r = major + minor * b.cos();
            cage.vertices
                .push([r * a.cos(), r * a.sin(), minor * b.sin()]);
            let k = (i + 1) % sides;
            let l = (j + 1) % rings;
            cage.faces.push(vec![
                i * rings + j,
                k * rings + j,
                k * rings + l,
                i * rings + l,
            ]);
        }
    }
    Ok(cage)
}
