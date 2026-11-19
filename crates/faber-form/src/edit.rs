//! Edit a closed control cage. Each operation uses the result of the preceding one.
mod mirror;
mod patch;
mod rings;
mod stitch;

use crate::Cage;
use faber_model::{CageData, Crease, SymmetryPlane};
use faber_ports::Result;
use glam::{DQuat, DVec3};
use mirror::Mirrors;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

type Data = CageData<f64>;
type Edge = [u32; 2];
fn edge(a: u32, b: u32) -> Edge {
    [a.min(b), a.max(b)]
}
fn perimeter(f: &[u32]) -> impl Iterator<Item = Edge> + '_ {
    f.iter()
        .copied()
        .zip(f.iter().copied().cycle().skip(1))
        .map(|(a, b)| [a, b])
}
fn point(d: &Data, v: u32) -> DVec3 {
    d.vertices[v as usize].into()
}
fn add(d: &mut Data, p: DVec3) -> u32 {
    let i = d.vertices.len() as u32;
    d.vertices.push(p.to_array());
    i
}
fn area_normal(d: &Data, f: &[u32]) -> DVec3 {
    let a = point(d, f[0]);
    (1..f.len() - 1)
        .map(|i| (point(d, f[i]) - a).cross(point(d, f[i + 1]) - a))
        .sum()
}

pub(crate) fn check_face_geometry(d: &Data, id: usize) -> Result<()> {
    let f = &d.faces[id];
    for (i, &v) in f.iter().enumerate() {
        for &w in &f[..i] {
            if point(d, v) == point(d, w) {
                return Err(format!("Face {id} has coincident vertices {w} and {v}."));
            }
        }
    }
    let origin = point(d, f[0]);
    let mut has_area = false;
    for i in 1..f.len() - 1 {
        let area = (point(d, f[i]) - origin).cross(point(d, f[i + 1]) - origin);
        if !area.is_finite() {
            return Err(format!("Face {id} exceeds the numeric range."));
        }
        has_area |= area != DVec3::ZERO;
    }
    if !has_area {
        return Err(format!("Face {id} has no area."));
    }
    Ok(())
}
fn crease_map(d: &Data) -> BTreeMap<Edge, f64> {
    d.creases
        .iter()
        .map(|c| (edge(c.edge[0], c.edge[1]), c.sharpness))
        .collect()
}
fn set_creases(d: &mut Data, creases: BTreeMap<Edge, f64>) {
    d.creases = creases
        .into_iter()
        .filter(|(_, s)| *s > 0.)
        .map(|(edge, sharpness)| Crease { edge, sharpness })
        .collect();
}
fn check_index(id: u32, n: usize, kind: &str) -> Result<()> {
    if id as usize >= n {
        Err(format!("Unknown cage {kind} {id}."))
    } else {
        Ok(())
    }
}
fn check_edges(c: &Cage, edges: &[Edge]) -> Result<()> {
    for &[a, b] in edges {
        if c.links
            .binary_search_by_key(&edge(a, b), |e| e.vertices)
            .is_err()
        {
            return Err(format!("Unknown cage edge {a}-{b}."));
        }
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Select {
    Vertices(Vec<u32>),
    Edges(Vec<[u32; 2]>),
    Faces(Vec<u32>),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FormOp {
    Move {
        select: Select,
        by: [f64; 3],
    },
    Scale {
        select: Select,
        factor: [f64; 3],
        about: Option<[f64; 3]>,
    },
    Rotate {
        select: Select,
        axis: [f64; 3],
        degrees: f64,
        about: Option<[f64; 3]>,
    },
    Extrude {
        faces: Vec<u32>,
        distance: f64,
    },
    Inset {
        faces: Vec<u32>,
        distance: f64,
    },
    InsertLoop {
        edge: [u32; 2],
        at: f64,
    },
    Crease {
        edges: Vec<[u32; 2]>,
        sharpness: f64,
    },
    Bridge {
        a: u32,
        b: u32,
    },
    Weld {
        a: u32,
        b: u32,
    },
    Delete {
        faces: Vec<u32>,
    },
    Flatten {
        select: Select,
        plane: [f64; 4],
    },
}

impl Select {
    fn vertices(&self, c: &Cage) -> Result<BTreeSet<u32>> {
        let d = c.data();
        let ids: BTreeSet<_> = match self {
            Self::Vertices(ids) => {
                for &id in ids {
                    check_index(id, d.vertices.len(), "vertex")?;
                }
                ids.iter().copied().collect()
            }
            Self::Edges(es) => {
                check_edges(c, es)?;
                es.iter().flatten().copied().collect()
            }
            Self::Faces(fs) => {
                let mut vs = BTreeSet::new();
                for &id in fs {
                    check_index(id, d.faces.len(), "face")?;
                    vs.extend(&d.faces[id as usize]);
                }
                vs
            }
        };
        if ids.is_empty() {
            return Err("Select at least one cage element.".into());
        }
        Ok(ids)
    }
}

fn transform(c: &mut Cage, select: &Select, op: &FormOp, m: &Mirrors) -> Result<()> {
    let ids = select.vertices(c)?;
    let centre = ids.iter().map(|&i| point(c.data(), i)).sum::<DVec3>() / ids.len() as f64;
    let about = |a: Option<[f64; 3]>| a.map_or(centre, DVec3::from);
    let map: Box<dyn Fn(DVec3) -> DVec3> = match *op {
        FormOp::Move { by, .. } => Box::new(move |p| p + DVec3::from(by)),
        FormOp::Scale {
            factor, about: a, ..
        } => {
            let o = about(a);
            Box::new(move |p| o + (p - o) * DVec3::from(factor))
        }
        FormOp::Rotate {
            axis,
            degrees,
            about: a,
            ..
        } => {
            let u = DVec3::from(axis)
                .try_normalize()
                .ok_or("Choose a finite rotation axis with a direction.")?;
            let rot = DQuat::from_axis_angle(u, degrees.to_radians());
            let o = about(a);
            Box::new(move |p| o + rot * (p - o))
        }
        FormOp::Flatten {
            plane: [x, y, z, offset],
            ..
        } => {
            let n = DVec3::new(x, y, z);
            let u = n
                .try_normalize()
                .ok_or("Choose a finite plane normal with a direction.")?;
            let h = offset / n.length();
            Box::new(move |p| p + (h - p.dot(u)) * u)
        }
        _ => unreachable!(),
    };
    let mut done = BTreeSet::new();
    for v in ids {
        if done.contains(&v) {
            continue;
        }
        let mut p = map(point(c.data(), v));
        if !p.is_finite() {
            return Err("The vertex transform must be finite.".into());
        }
        for (axis, mask) in [(0, 1), (1, 2), (2, 4)] {
            if m.maps.iter().any(|r| r.mask == mask && r.vertex(v) == v) {
                p[axis] = 0.;
            }
        }
        for r in &m.maps {
            let id = r.vertex(v);
            done.insert(id);
            c.vertices_mut()[id as usize] = r.reflect(p).to_array();
        }
    }
    let dirty: BTreeSet<_> = done
        .iter()
        .flat_map(|&id| c.vertex_faces[id as usize].iter().copied())
        .collect();
    for face in dirty {
        check_face_geometry(c.data(), face)?;
    }
    Ok(())
}

/// Check that each active plane has matching vertices, faces and creases.
pub fn check_symmetry(cage: &Cage, symmetry: &[SymmetryPlane]) -> Result<()> {
    Mirrors::new(cage.data(), symmetry).map(|_| ())
}

pub fn apply(cage: &Cage, ops: &[FormOp], symmetry: &[SymmetryPlane]) -> Result<Cage> {
    execute(cage, ops, symmetry, None)
}

/// Return the source index of each surviving vertex. New and welded vertices have no source.
pub fn apply_with_origins(
    cage: &Cage,
    ops: &[FormOp],
    symmetry: &[SymmetryPlane],
) -> Result<(Cage, Vec<Option<usize>>)> {
    let mut origins = (0..cage.data().vertices.len()).map(Some).collect();
    let edited = execute(cage, ops, symmetry, Some(&mut origins))?;
    Ok((edited, origins))
}

fn execute(
    cage: &Cage,
    ops: &[FormOp],
    symmetry: &[SymmetryPlane],
    mut origins: Option<&mut Vec<Option<usize>>>,
) -> Result<Cage> {
    let mut current = cage.clone();
    let mut mirrors = Mirrors::new(current.data(), symmetry)?;
    for op in ops {
        if let FormOp::Move { select, .. }
        | FormOp::Scale { select, .. }
        | FormOp::Rotate { select, .. }
        | FormOp::Flatten { select, .. } = op
        {
            transform(&mut current, select, op, &mirrors)?;
            mirrors = Mirrors::new(current.data(), symmetry)?;
            continue;
        }
        let mut d = current.to_data();
        match op {
            FormOp::Move { select, .. }
            | FormOp::Scale { select, .. }
            | FormOp::Rotate { select, .. }
            | FormOp::Flatten { select, .. } => {
                unreachable!("Transform already applied to {select:?}")
            }
            FormOp::Extrude { faces, distance } | FormOp::Inset { faces, distance } => {
                let selected = mirrors.faces(&d, faces)?;
                patch::raise(
                    &mut d,
                    &selected,
                    *distance,
                    matches!(op, FormOp::Inset { .. }),
                )?;
            }
            FormOp::InsertLoop { edge: seed, at } => {
                check_edges(&current, &[*seed])?;
                rings::insert(&mut d, &mirrors, *seed, *at)?;
            }
            FormOp::Crease { edges, sharpness } => {
                if edges.is_empty() || !(0. ..=10.).contains(sharpness) {
                    return Err("Select edges and a sharpness from 0 to 10.".into());
                }
                check_edges(&current, edges)?;
                let mut cs = crease_map(&d);
                for &[a, b] in edges {
                    for r in &mirrors.maps {
                        cs.insert(edge(r.vertex(a), r.vertex(b)), *sharpness);
                    }
                }
                set_creases(&mut d, cs);
            }
            FormOp::Bridge { a, b } => stitch::bridge(&mut d, &mirrors, *a, *b)?,
            FormOp::Weld { a, b } => {
                stitch::weld(&mut d, &mirrors, *a, *b, origins.as_deref_mut())?
            }
            FormOp::Delete { faces } => {
                let drop = mirrors.faces(&d, faces)?;
                let mut i = 0;
                d.faces.retain(|_| {
                    i += 1;
                    !drop.contains(&(i - 1))
                });
                stitch::compact(&mut d, origins.as_deref_mut());
            }
        }
        if let Some(map) = origins.as_mut() {
            map.resize(d.vertices.len(), None);
        }
        current = Cage::build(d).map_err(|e| {
            if matches!(op, FormOp::Delete { .. }) {
                "The cage is not closed after this operation.".into()
            } else {
                e
            }
        })?;
        mirrors = Mirrors::new(current.data(), symmetry)?;
    }
    Ok(current)
}
