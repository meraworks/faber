#![allow(dead_code)]
use faber_ports::Mesh;
use glam::DVec3;
use std::f64::consts::{PI, TAU};

pub fn cube(size: f64, n: usize) -> Mesh {
    let mut m = Mesh::default();
    for axis in 0..3 {
        for upper in [false, true] {
            let start = m.vertices.len() as u32;
            for j in 0..=n {
                for i in 0..=n {
                    let mut p = [0.; 3];
                    p[axis] = if upper { size } else { 0. };
                    p[(axis + 1) % 3] = size * i as f64 / n as f64;
                    p[(axis + 2) % 3] = size * j as f64 / n as f64;
                    m.vertices.push(p);
                }
            }
            for j in 0..n {
                for i in 0..n {
                    let a = start + (j * (n + 1) + i) as u32;
                    let c = a + (n + 2) as u32;
                    let mut pair = [[a, a + 1, c], [a, c, c - 1]];
                    if !upper {
                        for t in &mut pair {
                            t.swap(0, 1);
                        }
                    }
                    m.triangles.extend(pair);
                }
            }
        }
    }
    m
}
pub fn sphere(radius: f64, sectors: usize, rings: usize) -> Mesh {
    let mut m = Mesh::default();
    m.vertices.push([0., 0., radius]);
    for i in 0..rings {
        let (s, c) = (PI * (i + 1) as f64 / (rings + 1) as f64).sin_cos();
        for j in 0..sectors {
            let (y, x) = (TAU * j as f64 / sectors as f64).sin_cos();
            m.vertices
                .push([radius * s * x, radius * s * y, radius * c]);
        }
    }
    let south = m.vertices.len() as u32;
    m.vertices.push([0., 0., -radius]);
    let v = |i, j| 1 + (i * sectors + j % sectors) as u32;
    for j in 0..sectors {
        m.triangles.push([0, v(0, j), v(0, j + 1)]);
        for i in 0..rings - 1 {
            m.triangles.extend([
                [v(i, j), v(i + 1, j), v(i + 1, j + 1)],
                [v(i, j), v(i + 1, j + 1), v(i, j + 1)],
            ]);
        }
        m.triangles
            .push([v(rings - 1, j + 1), v(rings - 1, j), south]);
    }
    m
}
pub fn append(m: &mut Mesh, other: &Mesh, shift: [f64; 3]) {
    let base = m.vertices.len() as u32;
    m.vertices.extend(
        other
            .vertices
            .iter()
            .map(|&v| (DVec3::from(v) + DVec3::from(shift)).to_array()),
    );
    m.triangles
        .extend(other.triangles.iter().map(|t| t.map(|i| base + i)));
}
pub fn signed(m: &Mesh) -> f64 {
    m.triangles
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| DVec3::from(m.vertices[i as usize]));
            a.dot(b.cross(c)) / 6.
        })
        .sum()
}
