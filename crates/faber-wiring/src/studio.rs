//! Render the visible bodies in their supplied world coordinates.
use faber_model::{ProjectData, view::Camera};
use faber_ports::{Body, Result};
use faber_render::studio::StudioOptions;
pub use faber_render::studio::Style;
use faber_scene::Surface;
use std::{collections::BTreeMap, sync::Arc, time::Instant};

#[derive(Clone, Debug)]
pub struct Options {
    pub style: Style,
    pub ground: bool,
    pub background: [u8; 3],
    pub exposure: f32,
    pub high: bool,
    pub samples: u32,
    pub seconds: Option<f64>,
}
impl Default for Options {
    fn default() -> Self {
        let studio = StudioOptions::default();
        Self {
            style: Style::Technical,
            ground: studio.ground,
            background: studio.background,
            exposure: studio.exposure,
            high: false,
            samples: 64,
            seconds: None,
        }
    }
}
pub struct Frame {
    pub rgba: Vec<u8>,
    pub samples_done: u32,
    pub seconds: f64,
}
pub fn surfaces(data: &ProjectData, bodies: &[Body]) -> Result<BTreeMap<String, Surface>> {
    bodies
        .iter()
        .map(|body| {
            let colour = if let Some(id) = faber_core::part_material(data, &body.id) {
                let mat = faber_core::materials::get(data, id)?;
                Surface {
                    roughness: mat.roughness as f32,
                    metallic: mat.metallic as f32,
                    ..Surface::from_colour(mat.colour)
                }
            } else {
                Surface::from_colour(body.colour)
            };
            Ok((body.id.clone(), colour))
        })
        .collect()
}
pub fn default_cam(bodies: &[Body], width: u32, height: u32) -> Camera {
    faber_scene::Camera::fit_image(bodies, width, height).to_named()
}
pub fn draw(
    data: &ProjectData,
    bodies: &[Body],
    camera: &Camera,
    options: &Options,
    width: u32,
    height: u32,
) -> Result<Frame> {
    camera.validate()?;
    if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
        return Err("Choose an image width and height between 1 and 4096 pixels.".into());
    }
    if !options.exposure.is_finite() || options.exposure <= 0. {
        return Err("Exposure must be positive and finite.".into());
    }
    let start = Instant::now();
    if options.high {
        let surfaces = surfaces(data, bodies)?;
        let floor = bodies.iter().map(|b| b.bounds[0][2]).min_by(f64::total_cmp);
        let sky = faber_trace::Sky {
            horizon: Surface::from_colour(options.background).base,
            ..Default::default()
        };
        let scene = faber_trace::TraceScene {
            bodies: bodies
                .iter()
                .map(|body| faber_trace::TraceBody {
                    mesh: Arc::new(body.mesh.clone()),
                    surface: surfaces[&body.id],
                })
                .collect(),
            camera: *camera,
            sky,
            ground: options.ground.then_some(floor).flatten(),
        };
        let traced = faber_trace::trace(
            &scene,
            &faber_trace::TraceOptions {
                width,
                height,
                samples: options.samples,
                seconds: options.seconds,
                ..Default::default()
            },
        )?;
        let rgba = faber_trace::tone_map(&traced.pixels, options.exposure)
            .into_iter()
            .flat_map(|[r, g, b]| [r, g, b, 255])
            .collect();
        return Ok(Frame {
            rgba,
            samples_done: traced.samples_done,
            seconds: traced.seconds,
        });
    }
    let cam = faber_scene::Camera::from_named(camera)?;
    let matrix = cam.matrix(width as f32 / height as f32);
    let rgba = match options.style {
        Style::Technical => {
            faber_render::studio::technical_pixels(bodies, matrix, cam.eye(), width, height)?
        }
        Style::Studio => faber_render::studio::pixels(
            bodies,
            &surfaces(data, bodies)?,
            matrix,
            cam.eye(),
            &StudioOptions {
                ground: options.ground,
                exposure: options.exposure,
                background: options.background,
            },
            width,
            height,
        )?,
    };
    Ok(Frame {
        rgba,
        samples_done: 1,
        seconds: start.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_view_uses_the_assigned_or_inherited_material_and_the_part_colour_otherwise() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/assembly-motion");
        let mut project = crate::read_snapshot(&path).unwrap();
        let id = project.bodies[0].id.clone();
        let base = &project.bodies[0];
        let mut copy = base.clone();
        copy.id = "copy".into();
        let material = "aluminium-6061-t6";
        let mut instance = project.data.parts[&id].clone();
        instance.geometry = faber_model::Geometry::Instance { of: id.clone() };
        instance.material = None;
        project.data.parts.insert("copy".into(), instance);
        project.data.parts.get_mut(&id).unwrap().material = Some(material.into());
        let m = faber_core::materials::get(&project.data, material).unwrap();
        let palette = surfaces(&project.data, &[base.clone(), copy.clone()]).unwrap();
        assert_eq!(palette[&id], palette["copy"]);
        assert_eq!(palette[&id].metallic, m.metallic as f32);
        assert_eq!(palette[&id].roughness, m.roughness as f32);
        assert_eq!(palette[&id].base, Surface::from_colour(m.colour).base);
        project.data.parts.get_mut(&id).unwrap().material = None;
        assert_eq!(
            surfaces(&project.data, std::slice::from_ref(&copy)).unwrap()["copy"],
            Surface::from_colour(copy.colour)
        );
    }
}
