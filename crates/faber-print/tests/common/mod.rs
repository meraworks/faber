use faber_model::{Clearance, Printer};
use faber_ports::Mesh;
use glam::DVec3;

pub fn printer() -> Printer {
    Printer {
        name: "Test printer".into(),
        bed_mm: [256.; 3],
        nozzle_mm: 0.4,
        layer_mm: 0.2,
        min_wall_mm: 0.8,
        overhang_deg: 45.,
        bridge_mm: 10.,
        clearance_mm: Clearance {
            press: 0.,
            snug: 0.15,
            slide: 0.25,
            loose: 0.4,
        },
    }
}

/// Union of filled rectangular cells. Give each face its own vertices, as a tessellator does.
pub fn cells(ax: [&[f64]; 3], filled: impl Fn([usize; 3]) -> bool) -> Mesh {
    let mut mesh = Mesh {
        vertices: vec![],
        triangles: vec![],
        normals: vec![],
    };
    let n = ax.map(|v| v.len() - 1);
    for z in 0..n[2] {
        for y in 0..n[1] {
            for x in 0..n[0] {
                let p = [x, y, z];
                if !filled(p) {
                    continue;
                }
                for axis in 0..3 {
                    for positive in [false, true] {
                        let mut q = p;
                        if let Some(v) = p[axis].checked_add_signed(if positive { 1 } else { -1 })
                            && v < n[axis]
                        {
                            q[axis] = v;
                            if filled(q) {
                                continue;
                            }
                        }
                        let mut face = vec![];
                        for (u, v) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                            let mut node = p;
                            node[axis] += usize::from(positive);
                            node[(axis + 1) % 3] += u;
                            node[(axis + 2) % 3] += v;
                            face.push(std::array::from_fn(|i| ax[i][node[i]]));
                        }
                        if !positive {
                            face.reverse()
                        }
                        let s = mesh.vertices.len() as u32;
                        mesh.vertices.extend(face);
                        mesh.triangles
                            .extend([[s, s + 1, s + 2], [s, s + 2, s + 3]]);
                    }
                }
            }
        }
    }
    mesh
}

pub fn block(size: [f64; 3]) -> Mesh {
    cells([&[0., size[0]], &[0., size[1]], &[0., size[2]]], |_| true)
}

#[allow(dead_code)]
pub fn cylinder(radius: f64, h: f64) -> Mesh {
    let mut m = Mesh {
        vertices: vec![[0., 0., 0.], [0., 0., h]],
        triangles: vec![],
        normals: vec![],
    };
    let steps = 64u32;
    for i in 0..steps {
        let (s, c) = (std::f64::consts::TAU * f64::from(i) / f64::from(steps)).sin_cos();
        for z in [0., h] {
            m.vertices
                .push(DVec3::new(radius * c, radius * s, z).into());
        }
    }
    for i in 0..steps {
        let a = 2 + 2 * i;
        let b = 2 + 2 * ((i + 1) % steps);
        m.triangles.extend([
            [0, b, a],
            [1, a + 1, b + 1],
            [a, b, b + 1],
            [a, b + 1, a + 1],
        ]);
    }
    m
}
