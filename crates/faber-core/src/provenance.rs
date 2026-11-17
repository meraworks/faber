use faber_ports::{FaceOrigins, Kernel, Result, Shape};

pub(crate) fn track(
    kernel: &dyn Kernel,
    shape: Shape,
    inputs: &[Shape],
    feature: &str,
) -> Result<Shape> {
    let Some(links) = kernel.face_sources(&shape, inputs)? else {
        return Ok(shape);
    };
    let mut origins = FaceOrigins::new();
    for link in links {
        let input = inputs
            .get(link.input)
            .ok_or("Face history has an invalid input")?;
        let Some(source) = input
            .face_origins()
            .and_then(|origins| origins.get(&link.source_face))
        else {
            return Ok(shape);
        };
        origins
            .entry(link.face)
            .or_default()
            .extend(source.iter().cloned());
    }
    for face in kernel.faces(&shape)? {
        origins
            .entry(face.index)
            .or_insert_with(|| [feature.to_owned()].into());
    }
    Ok(shape.with_face_origins(origins))
}
