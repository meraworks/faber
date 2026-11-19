use faber_model::view::Camera;
use faber_ports::Mesh;
use faber_scene::Surface;
use faber_trace::*;

use std::sync::Arc;

fn sphere() -> Mesh {
    let mut m = Mesh::default();
    let n = 64u32;
    let bands = 32u32;
    for j in 0..=bands {
        let t = std::f64::consts::PI * f64::from(j) / f64::from(bands);
        for i in 0..n {
            let p = std::f64::consts::TAU * f64::from(i) / f64::from(n);
            let v = [t.sin() * p.cos(), t.sin() * p.sin(), t.cos()];
            m.vertices.push(v);
            m.normals.push(v);
        }
    }
    for j in 0..bands {
        for i in 0..n {
            let a = j * n + i;
            let b = j * n + (i + 1) % n;
            if j != 0 {
                m.triangles.push([a, a + n, b]);
            }
            if j + 1 != bands {
                m.triangles.push([b, a + n, b + n]);
            }
        }
    }
    m
}
fn cube() -> Mesh {
    let vertices = vec![
        [-1., -1., 0.],
        [1., -1., 0.],
        [1., 1., 0.],
        [-1., 1., 0.],
        [-1., -1., 1.5],
        [1., -1., 1.5],
        [1., 1., 1.5],
        [-1., 1., 1.5],
    ];
    let mut triangles = vec![];
    for [a, b, c, d] in [
        [3, 2, 1, 0],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ] {
        triangles.extend([[a, b, c], [a, c, d]]);
    }
    Mesh {
        vertices,
        triangles,
        ..Default::default()
    }
}
fn camera() -> Camera {
    Camera {
        eye: [0., -5., 0.],
        target: [0.; 3],
        up: [0., 0., 1.],
        fov_deg: None,
        ortho_height: Some(2.5),
    }
}
fn setup() -> TraceScene {
    TraceScene {
        camera: camera(),
        bodies: vec![TraceBody {
            mesh: Arc::new(sphere()),
            surface: Surface {
                base: [0.5; 3],
                roughness: 1.,
                metallic: 0.,
            },
        }],
        sky: Sky::default(),
        ground: None,
    }
}
fn opts(n: u32) -> TraceOptions {
    TraceOptions {
        width: 48,
        height: 48,
        samples: n,
        ..Default::default()
    }
}
#[test]
fn furnace() {
    let mut s = setup();
    s.sky.horizon = [1.; 3];
    s.sky.zenith = [1.; 3];
    s.sky.sun_radiance = [0.; 3];
    let t = trace(&s, &opts(64)).unwrap();
    let mut count = 0.;
    let mut sum = 0.;
    for i in 0..t.pixels.len() {
        let x = (i % 48) as f64 + 0.5 - 24.;
        let y = (i / 48) as f64 + 0.5 - 24.;
        if x * x + y * y < 15. * 15. {
            sum += f64::from(t.pixels[i][0]);
            count += 1.;
        }
    }
    let mean = sum / count;
    assert!((mean - 0.5).abs() < 0.015, "Mean linear radiance {mean}");
}
#[test]
fn shadow_under_a_box() {
    let mut s = setup();
    s.bodies[0].mesh = Arc::new(cube());
    s.ground = Some(-0.005);
    s.camera = Camera {
        eye: [0., 0., 6.],
        target: [0.; 3],
        up: [0., 1., 0.],
        ortho_height: Some(8.),
        fov_deg: None,
    };
    s.sky.sun_direction = [-1., 0., 1.];
    s.sky.sun_radius_deg = 2.;
    s.sky.sun_radiance = [900.; 3];
    s.sky.horizon = [0.1; 3];
    s.sky.zenith = [0.1; 3];
    let t = trace(&s, &opts(32)).unwrap();
    let average = |cx: usize| {
        let mut v = 0.;
        for y in 20..28 {
            for x in cx..cx + 4 {
                v += t.pixels[y * 48 + x][0];
            }
        }
        v / 32.
    };
    assert!(
        average(32) < average(7) * 0.7,
        "Shadow {}, open {}",
        average(32),
        average(7)
    );
}
#[test]
fn same_seed_same_bytes() {
    let scene = setup();
    let o = opts(6);
    let run = |n| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .build()
            .unwrap()
            .install(|| trace(&scene, &o).unwrap().pixels)
    };
    assert_eq!(run(1), run(4));
}
#[test]
fn more_samples_less_noise() {
    let mut s = setup();
    s.sky.sun_radiance = [0.; 3];
    s.sky.horizon = [0.03; 3];
    s.sky.zenith = [1.5; 3];
    let var = |n| {
        let a = trace(&s, &opts(n)).unwrap();
        let b = trace(
            &s,
            &TraceOptions {
                seed: 807,
                ..opts(n)
            },
        )
        .unwrap();
        a.pixels
            .iter()
            .zip(&b.pixels)
            .map(|(a, b)| (a[0] - b[0]).powi(2))
            .sum::<f32>()
    };
    let low = var(8);
    let high = var(32);
    assert!(high < 0.6 * low, "8 samples {low}, 32 samples {high}");
}
#[test]
fn time_limit_returns_a_picture() {
    let t = trace(
        &setup(),
        &TraceOptions {
            seconds: Some(0.5),
            samples: 100000,
            width: 64,
            height: 64,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        (1..100000).contains(&t.samples_done)
            && t.pixels.len() == 4096
            && t.pixels.iter().flatten().all(|v| v.is_finite())
    );
}
#[test]
fn mirror_shows_the_sky_gradient() {
    let mut s = setup();
    s.bodies[0].surface = Surface {
        base: [1.; 3],
        roughness: 0.,
        metallic: 1.,
    };
    s.sky.zenith = [1., 0.1, 0.];
    s.sky.horizon = [0., 0.1, 1.];
    s.sky.sun_radiance = [0.; 3];
    let im = trace(&s, &opts(8)).unwrap();
    let upper = im.pixels[15 * 48 + 24];
    let lower = im.pixels[32 * 48 + 24];
    assert!(
        upper[0] > lower[0] + 0.45 && lower[2] > upper[2] + 0.45,
        "{upper:?} {lower:?}"
    );
}
#[test]
fn tone_map_preserves_black_and_linear_exposure_order() {
    let pix = [[0.; 3], [0.1, 0.5, 2.]];
    let a = tone_map(&pix, 1.);
    let b = tone_map(&pix, 2.);
    assert_eq!(a[0], [0; 3]);
    assert!(a[1][0] < a[1][1] && a[1][1] < a[1][2] && a[1][1] < b[1][1]);
}
#[test]
fn excess_radiance_returns_a_numeric_error() {
    let mut s = setup();
    s.bodies.clear();
    s.camera.eye = [0., 0., 2.];
    s.camera.target = [0.; 3];
    s.camera.up = [0., 1., 0.];
    s.ground = Some(0.);
    s.sky.sun_direction = [0., 0., 1.];
    s.sky.sun_radius_deg = 89.;
    s.sky.sun_radiance = [f32::MAX; 3];
    s.sky.horizon = [f32::MAX; 3];
    s.sky.zenith = [f32::MAX; 3];
    let result = trace(
        &s,
        &TraceOptions {
            width: 16,
            height: 16,
            samples: 3,
            ..Default::default()
        },
    );
    assert!(result.is_err(), "A linear pixel overflow must be reported.");
}
