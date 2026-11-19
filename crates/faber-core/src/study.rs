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
    Ok(Study {
        kind: study.kind.map_numbers(&mut |s| number(s))?,
        part: study.part.clone(),
        material: study.material.clone(),
        resolution_mm,
        fixtures,
        loads,
        note: study.note.clone(),
        print_direction: study.print_direction.as_ref().map(direction).transpose()?,
    })
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
        let shape = local
            .shape
            .as_ref()
            .ok_or("A study face needs an exact solid.")?;
        crate::select::faces(self.kernel.as_ref(), shape, selector)
    }
}
