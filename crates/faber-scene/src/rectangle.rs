use faber_ports::Mesh;
use glam::{DMat4, DVec3, DVec4, Mat4};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RectMode {
    Window,
    Crossing,
}

/// Test a mesh against a rectangle in normalised device coordinates.
pub fn mesh_in_rect(
    mesh: &Mesh,
    transform: DMat4,
    view: Mat4,
    rect: [[f32; 2]; 2],
    mode: RectMode,
) -> bool {
    if mesh.triangles.is_empty() {
        return false;
    }
    let [[left, bottom], [right, top]] = rect.map(|p| p.map(f64::from));
    let planes = [
        DVec4::new(1., 0., 0., -left),
        DVec4::new(-1., 0., 0., right),
        DVec4::new(0., 1., 0., -bottom),
        DVec4::new(0., -1., 0., top),
        DVec4::Z,
        DVec4::new(0., 0., -1., 1.),
    ];
    let matrix = view.as_dmat4() * transform;
    let project = |v| matrix * DVec3::from(v).extend(1.);
    if mode == RectMode::Window {
        return mesh.vertices.iter().all(|&v| {
            let p = project(v);
            planes.iter().all(|n| n.dot(p) >= 0.)
        });
    }
    let points: Vec<_> = mesh.vertices.iter().copied().map(project).collect();
    mesh.triangles.iter().any(|tri| {
        let triangle = tri.map(|i| points[i as usize]);
        intersects(triangle, &planes)
    })
}

fn intersects(triangle: [DVec4; 3], planes: &[DVec4]) -> bool {
    // Each clip plane can add at most one vertex to a convex polygon.
    let mut polygon = [DVec4::ZERO; 9];
    let mut clipped = polygon;
    polygon[..3].copy_from_slice(&triangle);
    let mut count = 3;
    for &plane in planes {
        let vertices = &polygon[..count];
        if vertices.iter().all(|p| plane.dot(*p) >= 0.) {
            continue;
        }
        let mut previous = vertices[count - 1];
        let mut before = plane.dot(previous);
        let mut len = 0;
        for &point in vertices {
            let after = plane.dot(point);
            if (before > 0. && after < 0.) || (before < 0. && after > 0.) {
                clipped[len] = previous.lerp(point, before / (before - after));
                len += 1;
            }
            if after >= 0. {
                clipped[len] = point;
                len += 1;
            }
            previous = point;
            before = after;
        }
        if len == 0 {
            return false;
        }
        count = len;
        std::mem::swap(&mut polygon, &mut clipped);
    }
    true
}
