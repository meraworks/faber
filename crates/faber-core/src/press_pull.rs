//! Preview and commit a normal offset of one planar face.
use crate::Project;
use faber_model::{FaceSelector, Feature, Geometry, Part, ProjectData, Scalar};
use faber_ports::{Body, Result};

#[derive(Clone, Debug)]
pub struct PressFace {
    pub part: String,
    pub centre: [f64; 3],
    pub normal: [f64; 3],
    edit: FaceEdit,
    revision: u64,
}

#[derive(Clone, Debug)]
enum FaceEdit {
    Slab(Box<FaceSelector<Scalar>>),
    Offset(Vec<FaceSelector<Scalar>>),
}

fn part_with_features(
    data: &ProjectData,
    id: &str,
    additions: &[Feature<Scalar>],
) -> Result<Part<Scalar>> {
    let mut part = data
        .parts
        .get(id)
        .ok_or_else(|| format!("Unknown part: {id}"))?
        .clone();
    crate::pick_selector::editable_part(id, &part)?;
    let (mut features, mut output) = match part.geometry {
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
    for added in additions {
        if features.iter().any(|seen| seen.id == added.id) {
            return Err(format!("Feature {} already exists.", added.id));
        }
        features.push(added.clone());
        output.clone_from(&added.id);
    }
    part.geometry = Geometry::History { features, output };
    Ok(part)
}

pub fn preview_feature(
    project: &Project,
    part: &str,
    features: &[Feature<Scalar>],
) -> Result<Body> {
    feature_scene(project, part, features)?
        .into_iter()
        .find(|b| b.id == part)
        .ok_or_else(|| format!("Unknown part: {part}"))
}

fn feature_scene(project: &Project, part: &str, features: &[Feature<Scalar>]) -> Result<Vec<Body>> {
    let mut data = project.data.clone();
    data.parts.insert(
        part.into(),
        part_with_features(&project.data, part, features)?,
    );
    Ok(project.prepare(data, false)?.bodies)
}

impl PressFace {
    pub fn new(project: &Project, part: &str, index: u32) -> Result<Self> {
        Self::from_pick(project, part, &[index], None, false)
    }

    /// Set up an exact offset. The point is on the last selected face, in world coordinates.
    pub fn move_faces(
        project: &Project,
        part: &str,
        indices: &[u32],
        point: Option<[f64; 3]>,
    ) -> Result<Self> {
        Self::from_pick(project, part, indices, point, true)
    }

    fn from_pick(
        project: &Project,
        part: &str,
        indices: &[u32],
        point: Option<[f64; 3]>,
        exact: bool,
    ) -> Result<Self> {
        project.refuse_generated()?;
        let entry = project
            .data
            .parts
            .get(part)
            .ok_or_else(|| format!("Unknown part: {part}"))?;
        crate::pick_selector::editable_part(part, entry)?;
        let body = project
            .bodies
            .iter()
            .find(|b| b.id == part)
            .ok_or("Select a solid.")?;
        if body.shape.is_none() {
            return Err(format!(
                "Part {part} is a mesh. Convert it to a solid first."
            ));
        }
        let &index = indices.last().ok_or("Select one or more faces.")?;
        let local = project.unplaced_topology(part)?.1;
        let world = project.topology(part)?.1;
        let f = world
            .iter()
            .find(|f| f.index == index)
            .ok_or("Select a face of the part.")?;
        let centre = point.unwrap_or(f.centre);
        let normal = if let Some(n) = f.normal {
            n
        } else if !exact {
            return Err("Press pull needs a planar face.".into());
        } else if let Some(c) = &f.cylinder {
            let p = glam::DVec3::from(centre) - glam::DVec3::from(c.start);
            let axis = glam::DVec3::from(c.axis).normalize();
            let radial = (p - axis * p.dot(axis))
                .try_normalize()
                .ok_or("Click the cylindrical face.")?;
            (if c.internal { -radial } else { radial }).into()
        } else {
            // The display normal sets the handle direction. The kernel offsets the exact face.
            let p = glam::DVec3::from(centre);
            body.mesh
                .triangles
                .iter()
                .zip(&body.face_of_triangle)
                .filter(|(_, id)| **id == index)
                .filter_map(|(tri, _)| {
                    let [a, b, c] = tri.map(|i| glam::DVec3::from(body.mesh.vertices[i as usize]));
                    let n = (b - a).cross(c - a).try_normalize()?;
                    Some((((a + b + c) / 3. - p).length_squared(), n))
                })
                .min_by(|(a, _), (b, _)| a.total_cmp(b))
                .ok_or("Click the curved face.")?
                .1
                .into()
        };
        let selector = |index| crate::pick_selector::face_selector(&local, &[index]);
        let edit = if exact {
            FaceEdit::Offset(
                indices
                    .iter()
                    .copied()
                    .map(selector)
                    .collect::<Result<_>>()?,
            )
        } else {
            FaceEdit::Slab(Box::new(selector(index)?))
        };
        Ok(Self {
            part: part.into(),
            centre,
            normal,
            edit,
            revision: project.data.revision,
        })
    }

    fn features(&self, project: &Project, distance: f64) -> Result<Vec<Feature<Scalar>>> {
        if self.revision != project.data.revision {
            return Err("Project changed. Pick the face again.".into());
        }
        if !distance.is_finite() || distance == 0. {
            return Err("Enter a finite distance above or below zero.".into());
        }
        let p = project
            .data
            .parts
            .get(&self.part)
            .ok_or("The part was removed.")?;
        let (existing, source) = match &p.geometry {
            Geometry::History { features, output } => (features.as_slice(), output.as_str()),
            _ => (&[][..], "source"),
        };
        let next_id = |base: &str| {
            (1..)
                .map(|n| format!("{base}_{n}"))
                .find(|id| !existing.iter().any(|f| f.id == *id))
                .unwrap()
        };
        let selector = match &self.edit {
            FaceEdit::Offset(faces) => {
                return Ok(vec![Feature {
                    id: next_id("move_faces"),
                    suppressed: false,
                    geometry: Geometry::MoveFaces {
                        body: source.into(),
                        faces: faces.clone(),
                        distance: distance.into(),
                    },
                }]);
            }
            FaceEdit::Slab(selector) => selector.as_ref(),
        };
        let slab = next_id("face_offset");
        let result = next_id("press_pull");
        Ok(vec![
            Feature {
                id: slab.clone(),
                suppressed: false,
                geometry: Geometry::Thicken {
                    source: source.into(),
                    face: Some(selector.clone()),
                    thickness: Scalar::Number(distance),
                },
            },
            Feature {
                id: result,
                suppressed: false,
                geometry: if distance > 0. {
                    Geometry::Union {
                        left: source.into(),
                        right: slab,
                    }
                } else {
                    Geometry::Difference {
                        left: source.into(),
                        right: slab,
                    }
                },
            },
        ])
    }

    fn failure(
        &self,
        project: &Project,
        features: &[Feature<Scalar>],
        distance: f64,
        reason: String,
    ) -> String {
        if matches!(&self.edit, FaceEdit::Slab(_))
            && distance < 0.
            && let Ok(slab) = preview_feature(project, &self.part, &features[..1])
            && let Some(base) = project.bodies.iter().find(|b| b.id == self.part)
            && let (Some(a), Some(b)) = (&base.shape, &slab.shape)
            && let Ok(Some(common)) = project.kernel.intersect_solids(a, b)
            && let Ok(properties) = project.kernel.properties(&common)
            && (properties.volume - base.volume).abs() <= 1e-9 * base.volume
        {
            return "The cut removes the whole part.".into();
        }
        reason
    }
}

pub fn preview(project: &Project, face: &PressFace, distance: f64) -> Result<Body> {
    preview_scene(project, face, distance)?
        .into_iter()
        .find(|b| b.id == face.part)
        .ok_or("Part has no solid.".into())
}

/// Includes parts that depend on the changed solid.
pub fn preview_scene(project: &Project, face: &PressFace, distance: f64) -> Result<Vec<Body>> {
    let features = face.features(project, distance)?;
    feature_scene(project, &face.part, &features)
        .map_err(|reason| face.failure(project, &features, distance, reason))
}

pub fn apply(project: &mut Project, face: &PressFace, distance: f64, strict: bool) -> Result<()> {
    project.sync()?;
    let features = face.features(project, distance)?;
    let part = part_with_features(&project.data, &face.part, &features)?;
    project.refuse_generated()?;
    let mut next = project.data.clone();
    next.parts.insert(face.part.clone(), part);
    let result = project.apply_history(next, None, strict);
    result.map_err(|reason| face.failure(project, &features, distance, reason))
}
