#![allow(dead_code)]
use faber_model::{Geometry, Profile, machining::*};
use faber_ports::{Kernel, Mesh};
pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<[f64; 2]> {
    vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}
pub fn part(holes: Vec<Vec<[f64; 2]>>, floor: f64) -> Mesh {
    let profile = Profile {
        outer: rect(0., 0., 40., 30.),
        holes,
    };
    let g = if floor > 0. {
        Geometry::Pocket {
            profile,
            height: 10.,
            floor,
        }
    } else {
        Geometry::Extrude {
            profile,
            height: 10.,
        }
    };
    let mut mesh = faber_geometry::NativeKernel.build(&g).unwrap();
    for p in &mut mesh.vertices {
        p[2] -= 10.;
    }
    mesh
}
pub fn tool(diameter: f64) -> Tool {
    Tool {
        kind: ToolKind::Flat,
        diameter,
        corner_radius: 0.,
        flute_length: 20.,
        flutes: 2,
        feed_mm_min: 300.,
        plunge_mm_min: 60.,
        spindle_rpm: 10000.,
        number: 1,
    }
}
pub fn op(kind: OperationKind) -> Operation {
    Operation {
        id: "cut".into(),
        setup: "setup".into(),
        tool: "mill".into(),
        heights: Heights {
            clearance: 8.,
            retract: 4.,
            top: 0.,
            bottom: Some(-10.),
        },
        stepdown: Some(3.),
        kind,
    }
}
pub fn distance(p: [f64; 2], a: [f64; 3], b: [f64; 3]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx * dx + dy * dy;
    let t = if len > 0. {
        ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len
    } else {
        0.
    }
    .clamp(0., 1.);
    (p[0] - a[0] - dx * t).hypot(p[1] - a[1] - dy * t)
}
