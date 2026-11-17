//! Saved section analyses: view plane and new ids.

use faber_model::{ProjectData, Scalar, SectionAnalysis, SectionPlane};
use faber_scene::Section;
use glam::DVec3;

/// Returns the view plane of a world plane, or None for a zero normal.
pub fn from_world(origin: [f64; 3], normal: [f64; 3], reverse: bool) -> Option<Section> {
    let normal = DVec3::from(normal).try_normalize()?;
    Some(Section {
        normal: normal.to_array(),
        offset: normal.dot(DVec3::from(origin)),
        reverse,
    })
}

/// Returns the view plane of a plane section. Returns None when the section needs the model.
pub fn plane(section: &SectionAnalysis<Scalar>) -> Option<Section> {
    let SectionPlane::Plane { origin, normal } = &section.plane else {
        return None;
    };
    let number = |value: &Scalar| match value {
        Scalar::Number(number) => Some(*number),
        Scalar::Expression(_) => None,
    };
    let origin = [
        number(&origin[0])?,
        number(&origin[1])?,
        number(&origin[2])?,
    ];
    let normal = [
        number(&normal[0])?,
        number(&normal[1])?,
        number(&normal[2])?,
    ];
    from_world(origin, normal, section.reverse)
}

/// Returns a section id that the project does not use.
pub fn new_id(data: &ProjectData) -> String {
    (1..)
        .map(|n| format!("section_{n}"))
        .find(|id| !data.sections.contains_key(id))
        .expect("A section id is available")
}

/// Builds a saved plane section from a view plane.
pub fn from_plane(view: Section) -> SectionAnalysis<Scalar> {
    let normal = DVec3::from(view.normal).normalize();
    SectionAnalysis {
        plane: SectionPlane::Plane {
            origin: (normal * view.offset).to_array().map(Scalar::Number),
            normal: normal.to_array().map(Scalar::Number),
        },
        reverse: view.reverse,
        visible: true,
        parts: Vec::new(),
        note: String::new(),
    }
}
