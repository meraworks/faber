use faber_ports::{Body, Mesh};
use faber_render::studio::{self, StudioOptions};
use faber_scene::Surface;
use glam::Vec3;
use std::collections::BTreeMap;

fn bounds(m: &Mesh) -> [[f64; 3]; 2] {
    let mut b = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for p in &m.vertices {
        for i in 0..3 {
            b[0][i] = b[0][i].min(p[i]);
            b[1][i] = b[1][i].max(p[i]);
        }
    }
    b
}
fn body(mesh: Mesh) -> Body {
    Body {
        id: "part".into(),
        name: "Part".into(),
        group: String::new(),
        colour: [182, 108, 57],
        bounds: bounds(&mesh),
        mesh,
        volume: 1.,
        shape: None,
        face_of_triangle: vec![],
        edge_lines: Default::default(),
    }
}
fn sphere() -> Body {
    let mut mesh = Mesh::default();
    for j in 0..=24 {
        let p = std::f64::consts::PI * (j as f64 / 24. - 0.5);
        for k in 0..48 {
            let a = std::f64::consts::TAU * k as f64 / 48.;
            let n = [a.cos() * p.cos(), a.sin() * p.cos(), p.sin()];
            mesh.vertices.push(n);
            mesh.normals.push(n);
        }
    }
    for j in 0..24 {
        for k in 0..48 {
            let a = j * 48 + k;
            let b = j * 48 + (k + 1) % 48;
            if j > 0 {
                mesh.triangles.push([a, b, a + 48]);
            }
            if j < 23 {
                mesh.triangles.push([b, b + 48, a + 48]);
            }
        }
    }
    body(mesh)
}
const N: u32 = 160;
fn picture(bodies: &[Body], surface: Surface, ground: bool) -> Vec<u8> {
    let eye = Vec3::new(0., 0., 6.);
    let matrix = glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.01, 20.)
        * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let surfaces = bodies
        .iter()
        .map(|b| (b.id.clone(), surface))
        .collect::<BTreeMap<_, _>>();
    studio::pixels(
        bodies,
        &surfaces,
        matrix,
        eye,
        &StudioOptions {
            ground,
            background: [28, 34, 42],
            exposure: 1.,
        },
        N,
        N,
    )
    .unwrap()
}
fn lum(p: &[u8]) -> f32 {
    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.
}
fn rgb(p: &[u8], x: usize, y: usize) -> &[u8] {
    let i = 4 * (y * N as usize + x);
    &p[i..i + 3]
}

#[test]
fn studio_section_obeys_the_hatching_and_cut_colour_controls() {
    let mut gpu = faber_render::Gpu::headless().unwrap();
    let bodies = [sphere()];
    let list = studio::upload(&gpu.device, &bodies, &BTreeMap::new()).unwrap();
    let target = gpu.target(N, N);
    let plane = faber_scene::Section {
        normal: [0., 0., 1.],
        offset: 0.13,
        reverse: false,
    };
    let caps = faber_render::Gpu::caps(
        &gpu.device,
        &bodies,
        &[faber_scene::SectionIndex::new(&bodies[0].mesh)],
        &Default::default(),
        plane,
        &Default::default(),
    )
    .unwrap()
    .unwrap();
    let eye = Vec3::Z * 6.;
    let matrix = glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.01, 20.)
        * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let render = |gpu: &faber_render::Gpu| {
        studio::draw(
            gpu,
            &target,
            &list,
            matrix,
            eye,
            &StudioOptions::default(),
            None,
            Some(plane),
            Some(&caps),
            &Default::default(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        gpu.read(&target).unwrap()
    };
    gpu.hatch = false;
    gpu.cut_colour = Some([218, 35, 142]);
    let plain = render(&gpu);
    let centre = rgb(&plain, 80, 80);
    assert!(
        centre[0] > 200 && centre[1] < 50 && centre[2] > 125,
        "custom cut colour {centre:?}"
    );
    gpu.hatch = true;
    let striped = render(&gpu);
    let changed = (62..98)
        .flat_map(|y| (62..98).map(move |x| (x, y)))
        .filter(|&(x, y)| rgb(&plain, x, y)[0].abs_diff(rgb(&striped, x, y)[0]) > 20)
        .count();
    assert!(changed > 60, "changed {changed} cut pixels");
}

#[test]
fn sphere_has_a_highlight_and_metal_and_plastic_differ() {
    let sphere = sphere();
    let mut material = Surface::from_colour([182, 108, 57]);
    material.roughness = 0.22;
    let plastic = picture(std::slice::from_ref(&sphere), material, false);
    material.metallic = 1.;
    let metal = picture(&[sphere], material, false);
    let l: Vec<_> = plastic.as_chunks::<4>().0.iter().map(|p| lum(p)).collect();
    assert!(
        l.iter().any(|&v| v > 0.9),
        "peak {}",
        l.iter().copied().fold(0., f32::max)
    );
    assert!(l.iter().filter(|v| **v > 0.8).count() < l.len() / 10);
    let (mut sum, mut count) = (0u64, 0u64);
    for y in 40..120 {
        for x in 40..120 {
            if (x as isize - 80).pow(2) + (y as isize - 80).pow(2) < 38 * 38 {
                sum += rgb(&plastic, x, y)
                    .iter()
                    .zip(rgb(&metal, x, y))
                    .map(|(a, b)| u64::from(a.abs_diff(*b)))
                    .sum::<u64>();
                count += 3;
            }
        }
    }
    assert!(sum as f64 / count as f64 > 255. * 0.05);
}

#[test]
fn inside_corner_is_darker() {
    let mesh = Mesh {
        vertices: vec![
            [-1., 0., 0.],
            [1., 0., 0.],
            [1., 1.2, 0.],
            [-1., 1.2, 0.],
            [-1., 0., 1.2],
            [1., 0., 1.2],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3], [0, 4, 5], [0, 5, 1]],
        normals: vec![],
    };
    let pixels = picture(&[body(mesh)], Surface::from_colour([135; 3]), false);
    let near = lum(rgb(&pixels, 80, 76));
    let far = lum(rgb(&pixels, 80, 38));
    assert!(near < 0.9 * far, "inside {near}, open {far}");
}
#[test]
fn ground_shadow_is_below_the_part() {
    let material = Surface::from_colour([180; 3]);
    let elevated = body(Mesh {
        vertices: vec![
            [-0.5, -0.5, 1.],
            [0.5, -0.5, 1.],
            [0.5, 0.5, 1.],
            [-0.5, 0.5, 1.],
        ],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        normals: vec![],
    });
    let mut foot = body(Mesh {
        vertices: vec![[-1.5, -1.5, 0.], [-1.4, -1.5, 0.], [-1.5, -1.4, 0.]],
        triangles: vec![[0, 1, 2]],
        normals: vec![],
    });
    foot.id = "floor datum".into();
    // Two pins keep the ground position and light bounds equal in the reference.
    let pins = body(Mesh {
        vertices: vec![
            [-0.5, -0.5, 1.],
            [-0.49, -0.5, 1.],
            [-0.5, -0.49, 1.],
            [0.5, 0.5, 1.],
            [0.49, 0.5, 1.],
            [0.5, 0.49, 1.],
        ],
        triangles: vec![[0, 1, 2], [3, 4, 5]],
        normals: vec![],
    });
    let base = picture(&[foot.clone(), pins], material, true);
    let cast = picture(&[foot, elevated], material, true);
    let mut dark = 0;
    for y in 28..105 {
        for x in 65..135 {
            if (59..=101).contains(&x) && (59..=101).contains(&y) {
                continue;
            }
            if lum(rgb(&cast, x, y)) < 0.90 * lum(rgb(&base, x, y)) {
                dark += 1;
            }
        }
    }
    assert!(dark > 40, "{dark} ground pixels changed");
}
#[test]
fn part_without_material_uses_its_colour() {
    let s = Surface::from_colour([255, 0, 127]);
    assert_eq!(s.roughness, 0.6);
    assert_eq!(s.metallic, 0.);
    assert_eq!(s.base[0], 1.);
    assert_eq!(s.base[1], 0.);
    assert!((s.base[2] - 0.2122).abs() < 0.001);
}

#[test]
fn studio_differs_from_technical() {
    let b = sphere();
    let eye = Vec3::new(0., 0., 6.);
    let vp = glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.01, 20.)
        * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let plain = studio::technical_pixels(std::slice::from_ref(&b), vp, eye, N, N).unwrap();
    let lit = picture(&[b], Surface::from_colour([182, 108, 57]), false);
    let diff = (60..100)
        .flat_map(|x| (60..100).map(move |y| (x, y)))
        .map(|(x, y)| {
            rgb(&plain, x, y)
                .iter()
                .zip(rgb(&lit, x, y))
                .map(|(a, b)| u64::from(a.abs_diff(*b)))
                .sum::<u64>()
        })
        .sum::<u64>();
    assert!(diff > 40 * 40 * 3 * 12);
}

#[test]
fn a_bottom_view_shows_the_underside_with_the_ground_enabled() {
    let bodies = [sphere()];
    let eye = Vec3::new(0., 0., -6.);
    let projection = glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.01, 20.);
    let vp = projection * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
    let capture = |ground| {
        studio::pixels(
            &bodies,
            &BTreeMap::new(),
            vp,
            eye,
            &StudioOptions {
                ground,
                ..Default::default()
            },
            N,
            N,
        )
        .unwrap()
    };
    let on = capture(true);
    let off = capture(false);
    assert_eq!(
        rgb(&on, 80, 80),
        rgb(&off, 80, 80),
        "Ground obscures the underside"
    );
}

#[test]
fn orthographic_highlights_stay_in_place_when_the_eye_distance_changes() {
    let bodies = [sphere()];
    let projection = glam::camera::rh::proj::directx::orthographic(-2., 2., -2., 2., 0.01, 100.);
    let capture = |distance| {
        let eye = Vec3::new(0., 0., distance);
        let matrix = projection * glam::camera::rh::view::look_at_mat4(eye, Vec3::ZERO, Vec3::Y);
        studio::pixels(
            &bodies,
            &Default::default(),
            matrix,
            eye,
            &StudioOptions {
                ground: false,
                ..Default::default()
            },
            N,
            N,
        )
        .unwrap()
    };
    let close = capture(3.);
    let far = capture(40.);
    let changed = close
        .as_chunks::<4>()
        .0
        .iter()
        .zip(far.as_chunks::<4>().0)
        .filter(|(a, b)| a[..3].iter().zip(&b[..3]).any(|(x, y)| x.abs_diff(*y) > 2))
        .count();
    assert!(changed < 5, "{changed} pixels changed");
}
