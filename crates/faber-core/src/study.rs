//! Resolve study parameters in the same units as part recipes.
use crate::Project;
use faber_model::{
    FaceSelector, Scalar,
    study::{Fixture, Study, StudyLoad},
};
use faber_ports::Result;
use std::collections::BTreeMap;

pub fn resolve_study(study: &Study<Scalar>, values: &BTreeMap<String, f64>) -> Result<Study> {
    let number = |s: &Scalar| crate::number(s, values);
    let vector = |[x, y, z]: &[Scalar; 3]| {
        Ok::<_, String>([
            crate::number(x, values)?,
            crate::number(y, values)?,
            crate::number(z, values)?,
        ])
    };
    let direction = |raw: &[Scalar; 3]| {
        let v = glam::DVec3::from(vector(raw)?);
        v.try_normalize()
            .map(|n| n.to_array())
            .ok_or_else(|| "Study direction must be finite and non-zero.".to_string())
    };
    let face = |s: &FaceSelector<Scalar>| {
        let f = s.map_numbers(&mut |n| crate::number(n, values))?;
        crate::select::check_face_fields(&f)?;
        Ok::<_, String>(f)
    };
    let resolution_mm = study.resolution_mm.as_ref().map(number).transpose()?;
    if resolution_mm.is_some_and(|h| h <= 0.) {
        return Err("Study resolution must be greater than zero.".into());
    }
    let fixtures = study
        .fixtures
        .iter()
        .map(|f| {
            Ok(Fixture {
                kind: f.kind,
                face: face(&f.face)?,
            })
        })
        .collect::<Result<_>>()?;
    let loads = study
        .loads
        .iter()
        .map(|l| {
            Ok(match l {
                StudyLoad::Force { face: f, newtons } => StudyLoad::Force {
                    face: face(f)?,
                    newtons: vector(newtons)?,
                },
                StudyLoad::Pressure { face: f, mpa } => StudyLoad::Pressure {
                    face: face(f)?,
                    mpa: number(mpa)?,
                },
                StudyLoad::Gravity { direction: d } => StudyLoad::Gravity {
                    direction: direction(d)?,
                },
            })
        })
        .collect::<Result<_>>()?;
    let kind = study.kind.map_numbers(&mut |s| number(s))?;
    check_kind(&kind)?;
    Ok(Study {
        kind,
        part: study.part.clone(),
        material: study.material.clone(),
        resolution_mm,
        fixtures,
        loads,
        note: study.note.clone(),
        print_direction: study.print_direction.as_ref().map(direction).transpose()?,
    })
}

fn check_kind(kind: &faber_model::study::StudyKind) -> Result<()> {
    use faber_model::study::{StudyKind, ThermalCondition};
    match kind {
        StudyKind::Modal { modes } if !(1..=32).contains(modes) => {
            return Err("Choose from 1 to 32 modes.".into());
        }
        StudyKind::Shape {
            target_volume,
            min_member_mm,
            keep,
            symmetry,
        } => {
            if !(0.1..=0.9).contains(target_volume) {
                return Err("The target volume fraction must be from 0.1 to 0.9.".into());
            }
            if min_member_mm.is_some_and(|v| v <= 0.) {
                return Err("The minimum member size must be positive.".into());
            }
            for selector in keep {
                crate::select::check_face_fields(selector)?;
            }
            if symmetry.iter().any(|id| !crate::valid_id(id)) {
                return Err("Give the feature id of each symmetry plane.".into());
            }
        }
        StudyKind::Thermal { conditions, .. } => {
            for c in conditions {
                let selector = match c {
                    ThermalCondition::Temperature { face, .. }
                    | ThermalCondition::Power { face, .. } => face,
                    ThermalCondition::Convection { face, w_m2k, .. } => {
                        if *w_m2k <= 0. {
                            return Err("The convection coefficient must be positive.".into());
                        }
                        face
                    }
                };
                crate::select::check_face_fields(selector)?;
            }
        }
        _ => {}
    }
    Ok(())
}

impl Project {
    pub fn resolve_study(&self, study: &Study<Scalar>) -> Result<Study> {
        resolve_study(study, &self.params()?)
    }
    pub fn unplaced_faces_matching(&self, part: &str, selector: &FaceSelector) -> Result<Vec<u32>> {
        let local = self
            .locals
            .get(part)
            .ok_or_else(|| format!("Unknown part: {part}"))?;
        if let Some(shape) = &local.shape {
            return crate::select::faces(self.kernel.as_ref(), shape, selector);
        }
        crate::select::check_face_fields(selector)?;
        let mut found = Vec::new();
        for (i, t) in local.mesh.triangles.iter().enumerate() {
            let [a, b, c] = t.map(|n| glam::DVec3::from(local.mesh.vertices[n as usize]));
            let cross = (b - a).cross(c - a);
            let face = faber_ports::FaceInfo {
                index: i as u32 + 1,
                created_by: None,
                cylinder: None,
                normal: cross.try_normalize().map(|n| n.to_array()),
                centre: ((a + b + c) / 3.).to_array(),
                area: cross.length() / 2.,
            };
            if crate::select::face_failures(&face, selector, None).is_empty() {
                found.push(face.index);
            }
        }
        if found.len() != selector.expect {
            return Err(format!(
                "Mesh face selector found {}, expected {}.",
                found.len(),
                selector.expect
            ));
        }
        Ok(found)
    }
}
