//! Colour and display displacement for a static stress study.
use crate::study::{StressReport, StudyField};
use faber_ports::{Body, Result};
use image::{Rgba, RgbaImage};
use std::path::Path;

pub use faber_render::stress_colour as colour;

pub fn display(
    body: &Body,
    field: &StudyField,
    scale: f64,
    max_mpa: f32,
) -> Result<(Body, Vec<f32>)> {
    let n = body.mesh.vertices.len();
    if n == 0 || field.vertex_displacement.len() != n || field.vertex_von_mises.len() != n {
        return Err("Study values do not match the part mesh.".into());
    }
    if !scale.is_finite() || scale < 0. || !max_mpa.is_finite() || max_mpa < 0. {
        return Err("Display scale and maximum stress must be finite and non-negative.".into());
    }
    let mut shown = body.clone();
    let (stresses, displacement) = if let Some(surface) = &field.surface {
        let n = surface.mesh.vertices.len();
        if n == 0
            || n != surface.vertex_von_mises.len()
            || n != surface.vertex_displacement.len()
            || (!surface.face_of_triangle.is_empty()
                && surface.face_of_triangle.len() != surface.mesh.triangles.len())
            || surface
                .mesh
                .triangles
                .iter()
                .flatten()
                .any(|&i| i as usize >= n)
        {
            return Err("Study values do not match the display mesh.".into());
        }
        shown.mesh.clone_from(&surface.mesh);
        shown.face_of_triangle.clone_from(&surface.face_of_triangle);
        (&surface.vertex_von_mises, &surface.vertex_displacement)
    } else {
        (&field.vertex_von_mises, &field.vertex_displacement)
    };
    if stresses.iter().any(|v| !v.is_finite() || *v < 0.) {
        return Err("Stress values must be finite and non-negative.".into());
    }
    if scale != 0. {
        let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
        for (p, u) in shown.mesh.vertices.iter_mut().zip(displacement) {
            for c in 0..3 {
                p[c] += f64::from(u[c]) * scale;
                if !p[c].is_finite() {
                    return Err("Displacement is outside the display range.".into());
                }
                bounds[0][c] = bounds[0][c].min(p[c]);
                bounds[1][c] = bounds[1][c].max(p[c]);
            }
        }
        shown.bounds = bounds;
        shown.mesh.normals.clear();
        shown.edge_lines = Default::default();
        shown.shape = None;
    }
    Ok((
        shown,
        stresses
            .iter()
            .map(|v| {
                if max_mpa > 0. {
                    (*v / max_mpa).clamp(0., 1.)
                } else {
                    0.
                }
            })
            .collect(),
    ))
}

pub fn save(
    body: &Body,
    report: &StressReport,
    field: &StudyField,
    path: &Path,
    width: u32,
    height: u32,
    scale: f64,
) -> Result<()> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        return Err("Choose a PNG file for the stress map.".into());
    }
    let max = report.max_von_mises_mpa as f32;
    let (shown, ratios) = display(body, field, scale, max)?;
    let mut image =
        faber_render::stress_image(&shown, &ratios, width, height, faber_scene::View::Iso)?;
    legend(&mut image, max);
    save_png(&image, path)
}

pub(crate) fn save_png(image: &RgbaImage, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    image
        .write_to(file.as_file_mut(), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

// Five equal steps. The key stays readable at the minimum image size.
fn legend(image: &mut RgbaImage, max: f32) {
    let scale = if image.height() >= 280 && image.width() >= 400 {
        2
    } else {
        1
    };
    let left = 4;
    let pitch = 8 * scale;
    let top = image.height() - pitch * 6 - 2;
    let right = (left + 40 * scale).min(image.width());
    for y in top..image.height() {
        for x in 0..right {
            image.put_pixel(x, y, Rgba([248, 249, 249, 255]));
        }
    }
    label(image, left, top + scale, scale, "MPa");
    for i in 0..5 {
        let value = max * (4 - i) as f32 / 4.;
        let rgb = colour(value, max);
        let y = top + (i + 1) * pitch;
        for row in y..y + 6 * scale {
            for x in left..left + 4 * scale {
                image.put_pixel(x, row, Rgba([rgb[0], rgb[1], rgb[2], 255]));
            }
        }
        let text = if value != 0. && !(0.01..1000.).contains(&value) {
            format!("{value:.1e}")
        } else {
            format!("{value:.2}")
        };
        label(image, left + 6 * scale, y, scale, &text);
    }
}

// Three-column glyphs for a numeric legend.
fn label(image: &mut RgbaImage, mut x: u32, y: u32, scale: u32, text: &str) {
    for ch in text.bytes() {
        let rows = match ch {
            b'0' => [7, 5, 5, 5, 7],
            b'1' => [2, 6, 2, 2, 7],
            b'2' => [7, 1, 7, 4, 7],
            b'3' => [7, 1, 7, 1, 7],
            b'4' => [5, 5, 7, 1, 1],
            b'5' => [7, 4, 7, 1, 7],
            b'6' => [7, 4, 7, 5, 7],
            b'7' => [7, 1, 1, 1, 1],
            b'8' => [7, 5, 7, 5, 7],
            b'9' => [7, 5, 7, 1, 7],
            b'.' => [0, 0, 0, 0, 2],
            b'-' => [0, 0, 7, 0, 0],
            b'e' => [0, 7, 5, 6, 7],
            b'M' => [5, 7, 7, 5, 5],
            b'P' => [7, 5, 7, 4, 4],
            b'a' => [0, 6, 3, 5, 7],
            _ => [0; 5],
        };
        for (row, bits) in rows.into_iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let (px, py) = (x + col * scale + dx, y + row as u32 * scale + dy);
                            if px < image.width() && py < image.height() {
                                image.put_pixel(px, py, Rgba([30, 40, 48, 255]));
                            }
                        }
                    }
                }
            }
        }
        x += 4 * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use faber_ports::Mesh;

    #[test]
    fn png_interpolates_stress_before_applying_the_legend_colours() {
        let body = Body {
            id: "plate".into(),
            name: "Plate".into(),
            group: String::new(),
            colour: [120; 3],
            mesh: Mesh {
                vertices: vec![
                    [0., 0., 10.],
                    [20., 0., 10.],
                    [20., 20., 10.],
                    [0., 20., 10.],
                ],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                normals: vec![],
            },
            bounds: [[0., 0., 10.], [20., 20., 10.]],
            volume: 0.,
            shape: None,
            face_of_triangle: vec![],
            edge_lines: Default::default(),
        };
        let report = StressReport {
            study: "gradient".into(),
            part: "plate".into(),
            material: "steel-s235".into(),
            max_von_mises_mpa: 100.,
            max_displacement_mm: 0.,
            min_safety_factor: 2.35,
            layer_safety_factor: None,
            hot_spots: vec![],
            reaction_n: [0.; 3],
            mass_g: 0.,
            method: String::new(),
            resolution_mm: 1.,
            cells: 400,
            iterations: 1,
            residual: 0.,
            limits: vec![],
        };
        let field = StudyField {
            surface: None,
            vertex_von_mises: vec![0., 100., 100., 0.],
            vertex_displacement: vec![[0.; 3]; 4],
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gradient.png");
        save(&body, &report, &field, &path, 256, 256, 0.).unwrap();
        let img = image::open(path).unwrap().into_rgba8();
        let mut camera = faber_scene::Camera::fit(std::slice::from_ref(&body));
        camera.set_view(faber_scene::View::Iso);
        let matrix = camera.matrix(1.);
        for (x, value) in [(3., 15.), (10., 50.), (17., 85.)] {
            let point = matrix.project_point3(glam::Vec3::new(x, 10., 10.));
            let rgb = img
                .get_pixel(
                    ((point.x + 1.) * 128.) as u32,
                    ((1. - point.y) * 128.) as u32,
                )
                .0;
            let expected = colour(value, 100.);
            assert!(
                rgb[..3]
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 10),
                "Stress {value} MPa: {rgb:?}; legend: {expected:?}"
            );
        }
        let violet = img
            .pixels()
            .filter(|p| p[0] > 80 && p[2] > 80 && p[1].saturating_add(24) < p[0].min(p[2]))
            .count();
        assert_eq!(violet, 0, "The stress legend contains no violet.");
    }
}
