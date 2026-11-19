//! Stock and toolpaths share one depth buffer with the verified cut surface.
use super::*;
use std::collections::BTreeMap;

pub const RAPID: [u8; 3] = [247, 196, 47];
pub const FEED: [u8; 3] = [34, 117, 222];
pub const CUT: [u8; 3] = [153, 166, 151];
pub const GOUGE: [u8; 3] = [242, 48, 42];

pub struct Path {
    pub id: String,
    pub segments: Vec<Segment>,
}
pub struct Segment {
    pub from: [f64; 3],
    pub to: [f64; 3],
    pub rapid: bool,
}
pub struct Scene {
    pub part: Mesh,
    pub cut: Mesh,
    pub stock: Mesh,
    pub cut_colours: Vec<[u8; 3]>,
    pub paths: Vec<Path>,
    pub bounds: [[f64; 3]; 2],
}
pub struct Display {
    list: DrawList,
    ids: Vec<String>,
    bounds: [[f64; 3]; 2],
}

pub(super) fn strip(vertices: &mut Vec<Vertex>, from: [f64; 3], to: [f64; 3], colour: [f32; 3]) {
    let a = from.map(|x| x as f32);
    let b = to.map(|x| x as f32);
    if a == b {
        return;
    }
    let v = |position, normal, kind| Vertex {
        position,
        normal,
        colour,
        kind,
    };
    vertices.extend([
        v(a, b, -6.),
        v(a, b, -7.),
        v(b, a, -6.),
        v(b, a, -6.),
        v(b, a, -7.),
        v(a, b, -6.),
    ]);
}
impl Scene {
    pub fn upload(&self, device: &wgpu::Device) -> Result<Display> {
        if self.cut.triangles.len() != self.cut_colours.len() {
            return Err("The cut colour count differs from the stock triangles.".into());
        }
        let segments: usize = self.paths.iter().map(|p| p.segments.len()).sum();
        let count =
            (self.part.triangles.len() + self.cut.triangles.len() + self.stock.triangles.len()) * 3
                + (segments + 12) * 6;
        if count as u64 > device.limits().max_buffer_size / size_of::<Vertex>() as u64 {
            return Err(
                "The toolpaths exceed the display buffer size. Select fewer operations.".into(),
            );
        }
        let mut vertices = Vec::with_capacity(count);
        let mut ranges = BTreeMap::new();
        let mut end = 0;
        let mut record = |id: String, count: usize| {
            ranges.insert(id, [end, count as u32]);
            end = count as u32;
        };
        append(&mut vertices, &self.part, [116, 142, 157], -4.);
        record("0:part".into(), vertices.len());
        // Stock cells are flat terraces. Keep their actual face normals at steps.
        for (tri, rgb) in self.cut.triangles.iter().zip(&self.cut_colours) {
            let xyz = tri.map(|i| glam::DVec3::from(self.cut.vertices[i as usize]));
            let normal = (xyz[1] - xyz[0])
                .cross(xyz[2] - xyz[0])
                .normalize_or_zero()
                .as_vec3()
                .to_array();
            let colour = rgb.map(|v| (f32::from(v) / 255.).powf(2.2));
            let kind = if *rgb == GOUGE { -3. } else { -4. };
            vertices.extend(xyz.map(|p| Vertex {
                position: p.as_vec3().to_array(),
                normal,
                colour,
                kind,
            }));
        }
        record("1:cut".into(), vertices.len());
        let ids = self
            .paths
            .iter()
            .enumerate()
            .map(|(n, path)| {
                for s in &path.segments {
                    let rgb = if s.rapid { RAPID } else { FEED };
                    strip(
                        &mut vertices,
                        s.from,
                        s.to,
                        rgb.map(|v| (f32::from(v) / 255.).powf(2.2)),
                    );
                }
                let key = format!("2:{n:08}");
                record(key.clone(), vertices.len());
                key
            })
            .collect();
        append(&mut vertices, &self.stock, [140, 188, 181], -5.);
        let v = &self.stock.vertices;
        if v.len() != 8 {
            return Err("The stock box needs eight corners.".into());
        }
        for i in 0..8 {
            for bit in [1, 2, 4] {
                if i & bit == 0 {
                    strip(&mut vertices, v[i], v[i | bit], [0.18, 0.31, 0.33]);
                }
            }
        }
        record("3:blank".into(), vertices.len());
        let list = DrawList {
            marks: FaceMarks::new(device, vertices.len(), Default::default()),
            field: None,
            cap_meshes: Default::default(),
            count: vertices.len() as u32,
            ranges,
            buffer: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Machining setup"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        };
        Ok(Display {
            list,
            ids,
            bounds: self.bounds,
        })
    }
    pub fn save(&self, path: &std::path::Path, width: u32, height: u32) -> Result<()> {
        if !(64..=4096).contains(&width) || !(64..=4096).contains(&height) {
            return Err("Image size must be between 64 and 4096 pixels.".into());
        }
        with_headless(|gpu| {
            let d = self.upload(&gpu.device)?;
            let target = gpu.target(width, height);
            let camera = d.camera(width as f32 / height as f32);
            d.draw(gpu, &target, camera, true, None);
            image::RgbaImage::from_raw(width, height, gpu.read(&target)?)
                .ok_or("Invalid image size.")?
                .save(path)
                .map_err(|e| e.to_string())
        })
    }
}
impl Display {
    pub fn camera(&self, aspect: f32) -> Camera {
        let [lo, hi] = self.bounds.map(|v| glam::Vec3::from(v.map(|x| x as f32)));
        Camera {
            target: (lo + hi) * 0.5,
            yaw: -55.,
            pitch: 32.,
            distance: (hi - lo).length().max(1.) * 1.85 / aspect.clamp(0.01, 1.),
        }
    }
    pub fn draw(
        &self,
        gpu: &Gpu,
        target: &Target,
        camera: Camera,
        simulate: bool,
        operation: Option<usize>,
    ) {
        let mut hidden = BTreeSet::new();
        if simulate {
            hidden.insert("3:blank".into());
            hidden.insert("0:part".into());
        } else {
            hidden.insert("1:cut".into());
        }
        if let Some(active) = operation {
            for (i, id) in self.ids.iter().enumerate() {
                if active != i {
                    hidden.insert(id.clone());
                }
            }
        }
        gpu.draw_visible_view(
            target,
            &self.list,
            camera,
            None,
            None,
            None,
            &Default::default(),
            &hidden,
        );
    }
}
