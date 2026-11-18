use super::number;
use faber_ports::{Mesh, Result};
use glam::DVec3;

pub(super) fn lines(text: &str) -> impl Iterator<Item = &str> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
}

pub(super) fn face(mesh: &mut Mesh, indices: &[u32]) -> Result<()> {
    if indices.len() < 3 || indices.iter().any(|&i| i as usize >= mesh.vertices.len()) {
        return Err("Face needs at least three valid vertex indices".into());
    }
    if indices.len() == 3 {
        mesh.triangles.push([indices[0], indices[1], indices[2]]);
        return Ok(());
    }
    let points: Vec<_> = indices
        .iter()
        .map(|&i| DVec3::from(mesh.vertices[i as usize]))
        .collect();
    let normal: DVec3 = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a.cross(*b))
        .sum();
    if normal.length_squared() < 1e-24 {
        return Err("Polygon has no area".into());
    }
    let axis = normal.abs().max_element();
    let drop = if normal.x.abs() == axis {
        0
    } else if normal.y.abs() == axis {
        1
    } else {
        2
    };
    let coords: Vec<_> = points
        .iter()
        .flat_map(|p| (0..3).filter(move |&i| i != drop).map(move |i| p[i]))
        .collect();
    let triangles = earcutr::earcut(&coords, &[], 2).map_err(|e| e.to_string())?;
    if triangles.len() != (indices.len() - 2) * 3 {
        return Err("Cannot triangulate polygon".into());
    }
    for triangle in triangles.as_chunks::<3>().0 {
        let [a, b, c] = [
            points[triangle[0]],
            points[triangle[1]],
            points[triangle[2]],
        ];
        let mut face = [
            indices[triangle[0]],
            indices[triangle[1]],
            indices[triangle[2]],
        ];
        if (b - a).cross(c - a).dot(normal) < 0. {
            face.swap(1, 2);
        }
        mesh.triangles.push(face);
    }
    Ok(())
}

pub(super) fn obj(text: &str) -> Result<Mesh> {
    let mut mesh = Mesh::default();
    let (mut textures, mut normals) = (0usize, 0usize);
    for line in lines(text) {
        let fields: Vec<_> = line.split_whitespace().collect();
        match fields[0] {
            "v" => {
                if !matches!(fields.len(), 4 | 5 | 7) {
                    return Err("OBJ vertex needs XYZ, XYZ with weight, or XYZ with RGB".into());
                }
                for field in &fields[4..] {
                    number(field)?;
                }
                let mut p = [number(fields[1])?, number(fields[2])?, number(fields[3])?];
                if fields.len() == 5 {
                    let w = number(fields[4])?;
                    if w == 0. {
                        return Err("OBJ vertex weight must not be zero".into());
                    }
                    p = p.map(|x| x / w);
                }
                mesh.vertices.push(p);
            }
            "f" => {
                let indices = fields[1..]
                    .iter()
                    .map(|s| {
                        let fields: Vec<_> = s.split('/').collect();
                        if fields.len() > 3 || fields.last() == Some(&"") {
                            return Err("Invalid OBJ face reference".into());
                        }
                        let vertex = obj_index(fields[0], mesh.vertices.len())?;
                        if fields.len() > 1 && !fields[1].is_empty() {
                            obj_index(fields[1], textures)?;
                        }
                        if fields.len() == 3 {
                            obj_index(fields[2], normals)?;
                        }
                        Ok(vertex)
                    })
                    .collect::<Result<Vec<_>>>()?;
                face(&mut mesh, &indices)?;
            }
            "vt" | "vn" | "vp" => {
                if !(2..=4).contains(&fields.len()) || (fields[0] == "vn" && fields.len() != 4) {
                    return Err("Invalid OBJ attribute".into());
                }
                for value in &fields[1..] {
                    number(value)?;
                }
                if fields[0] == "vt" {
                    textures += 1;
                }
                if fields[0] == "vn" {
                    normals += 1;
                }
            }
            "o" | "g" | "s" | "usemtl" | "mtllib" => (),
            _ => return Err(format!("Unsupported OBJ statement: {}", fields[0])),
        }
    }
    Ok(mesh)
}

fn obj_index(text: &str, count: usize) -> Result<u32> {
    let i: i64 = text.parse().map_err(|_| "Invalid OBJ index")?;
    let resolved = if i < 0 {
        (count as i64).checked_add(i)
    } else {
        i.checked_sub(1)
    }
    .ok_or("OBJ index is out of range")?;
    if resolved < 0 || resolved >= count as i64 {
        return Err("OBJ index is out of range".into());
    }
    Ok(resolved as u32)
}

pub(super) fn off(text: &str) -> Result<Mesh> {
    let mut tokens = lines(text).flat_map(str::split_whitespace);
    if tokens.next() != Some("OFF") {
        return Err("Invalid OFF header".into());
    }
    let mut next = || {
        tokens
            .next()
            .ok_or_else(|| "OFF data is incomplete".to_string())
    };
    let vertices: usize = next()?.parse().map_err(|_| "Invalid OFF vertex count")?;
    let faces: usize = next()?.parse().map_err(|_| "Invalid OFF face count")?;
    let _: usize = next()?.parse().map_err(|_| "Invalid OFF edge count")?;
    if vertices > text.len() || faces > text.len() {
        return Err("OFF count exceeds input size".into());
    }
    let mut mesh = Mesh::default();
    for _ in 0..vertices {
        mesh.vertices
            .push([number(next()?)?, number(next()?)?, number(next()?)?]);
    }
    for _ in 0..faces {
        let count: usize = next()?.parse().map_err(|_| "Invalid OFF polygon size")?;
        if count > vertices {
            return Err("OFF polygon is too large".into());
        }
        let indices = (0..count)
            .map(|_| next()?.parse().map_err(|_| "Invalid OFF index".into()))
            .collect::<Result<Vec<u32>>>()?;
        face(&mut mesh, &indices)?;
    }
    if tokens.next().is_some() {
        return Err("Unexpected data after OFF faces".into());
    }
    Ok(mesh)
}
